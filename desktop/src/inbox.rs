//! Verified Relay inbox. SYNBKP01 backups are authenticated and decrypted locally.
//! Ciphertext hash is checked, plaintext is persisted without overwrite, and
//! sender is ACKed only once a verified copy is safely stored on this PC.
//! Other E2EE formats (including browser SYNCHRO1) are NOT supported.
use crate::{Settings, MAX_SIZE, backup_crypto};
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::{self, File, OpenOptions}, io::{Read, Write},
          path::{Path, PathBuf}, time::Duration};

#[derive(Deserialize)]
struct Inbound {
    id: String,
    filename: String,
    size_bytes: u64,
    sha256: String,
    status: String,
    #[serde(default)]
    encrypted: bool,
}

pub(super) fn is_ready(settings: &Settings) -> bool {
    settings.address.trim().starts_with("https://")
        && !settings.token.trim().is_empty()
        && !settings.receive_folder.trim().is_empty()
}

fn safe_name(name: &str) -> String {
    let mut out = name.chars()
        .filter(|c| !c.is_control() && !r#"<>:"/\|?*"#.contains(*c))
        .take(120)
        .collect::<String>();
    out = out.trim().trim_end_matches('.').to_string();
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    if out.is_empty() || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" |
        "COM1" | "COM2" | "COM3" | "COM4" | "COM5" | "COM6" | "COM7" | "COM8" | "COM9" |
        "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9") {
        "synchro-file.bin".to_owned()
    } else { out }
}
fn verified(path: &Path, expected: &str, expected_size: u64) -> bool {
    let Ok(mut file) = File::open(path) else { return false };
    if file.metadata().map(|meta|meta.len()!=expected_size).unwrap_or(true) { return false; }
    let mut digest=Sha256::new();
    let mut buffer=[0u8;64*1024];
    loop {
        let Ok(n)=file.read(&mut buffer) else { return false };
        if n==0 { break; }
        digest.update(&buffer[..n]);
    }
    format!("{:x}", digest.finalize()).eq_ignore_ascii_case(expected)
}

#[derive(Deserialize, Serialize)]
struct BackupReceipt {
    filename: String,
    ciphertext_sha256: String,
    plaintext_sha256: String,
    plaintext_size: u64,
}
fn receipt_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!(".synchro-{id}.receipt.json"))
}
fn check_receipt(dir: &Path, item: &Inbound) -> Option<bool> {
    let bytes=fs::read(receipt_path(dir,&item.id)).ok()?;
    let receipt: BackupReceipt=serde_json::from_slice(&bytes).ok()?;
    if !receipt.ciphertext_sha256.eq_ignore_ascii_case(&item.sha256) {
        return Some(false);
    }
    let target=dir.join(&receipt.filename);
    Some(verified(&target, &receipt.plaintext_sha256, receipt.plaintext_size))
}
fn save_receipt(dir: &Path, item: &Inbound, filename: &str,
                decrypted: &backup_crypto::DecryptResult) -> Result<(), String> {
    let receipt=BackupReceipt {
        filename:filename.to_owned(),ciphertext_sha256:item.sha256.clone(),
        plaintext_sha256:decrypted.plaintext_sha256.clone(),
        plaintext_size:decrypted.plain_size
    };
    let path=receipt_path(dir,&item.id);
    let bytes=serde_json::to_vec(&receipt).map_err(|e|e.to_string())?;
    if path.exists() {
        return if check_receipt(dir,item)==Some(true) {Ok(())}
               else {Err("Conflicting backup receipt on disk".into())};
    }
    let mut file=OpenOptions::new().create_new(true).write(true)
        .open(&path).map_err(|e|e.to_string())?;
    file.write_all(&bytes).map_err(|e|e.to_string())?;
    file.sync_all().map_err(|e|e.to_string())?;
    Ok(())
}
fn checked_response(response: Response) -> Result<Response,String> {
    if !response.status().is_success() {
        let code=response.status();
        let mut small=String::new();
        let _=response.take(256).read_to_string(&mut small);
        return Err(format!("Relay HTTP {code}: {small}"));
    }
    Ok(response)
}
fn ack(client: &Client, base: &str, token: &str, item: &Inbound) -> Result<(),String> {
    let response=client.post(format!("{base}/api/v1/transfers/{}/ack",item.id))
        .bearer_auth(token).json(&serde_json::json!({"sha256":item.sha256}))
        .send().map_err(|e|e.to_string())?;
    if response.status().is_success() || response.status().as_u16()==409 {
        return Ok(());
    }
    Err(format!("ACK HTTP {}",response.status()))
}

fn receive_one(client: &Client, base: &str, token: &str, dir: &Path,
               item: &Inbound, backup_key: &str)
    -> Result<bool,String>
{
    if item.id.len()!=36 || !item.id.bytes().all(|b|b.is_ascii_hexdigit()||b==b'-') {
        return Err("Untrusted relay transfer ID".into());
    }
    if item.size_bytes==0 || item.size_bytes>MAX_SIZE
        || item.sha256.len()!=64 || !item.sha256.bytes().all(|b|b.is_ascii_hexdigit()) {
        return Err("Invalid transfer metadata".into());
    }
    if item.encrypted {
        if !backup_crypto::valid_key(backup_key) {
            return Err("E2EE ключ Windows не введено — зашифровану копію залишено на Relay".into());
        }
        if check_receipt(dir,item)==Some(true) {
            ack(client,base,token,item)?;
            return Ok(false);
        }
        let filename=format!("{}-{}", &item.id[..8], safe_name(&item.filename));
        let target=dir.join(&filename);
        let nonce=rand::random::<u64>();
        let temporary=dir.join(format!(".synchro-{}-{nonce:016x}.part",item.id));
        let result=(|| -> Result<bool,String> {
            let mut output=OpenOptions::new().write(true).create_new(true)
                .open(&temporary).map_err(|e|e.to_string())?;
            let response=checked_response(client.get(
                format!("{base}/api/v1/transfers/{}/download",item.id))
                .bearer_auth(token).send().map_err(|e|e.to_string())?)?;
            let decrypted=backup_crypto::decrypt_stream(response,&mut output,item.size_bytes,backup_key)?;
            if !decrypted.ciphertext_sha256.eq_ignore_ascii_case(&item.sha256) {
                return Err("Encrypted download SHA-256 mismatch".into());
            }
            output.sync_all().map_err(|e|e.to_string())?;
            drop(output);
            let created=if target.exists() {
                if !verified(&target,&decrypted.plaintext_sha256,decrypted.plain_size) {
                    return Err("Existing target differs — refusing to overwrite".into());
                }
                false
            } else {
                // Hard link is exclusive and avoids rename-overwrite race.
                fs::hard_link(&temporary,&target).map_err(|e|e.to_string())?;
                true
            };
            save_receipt(dir,item,&filename,&decrypted)?;
            Ok(created)
        })();
        let _=fs::remove_file(&temporary);
        let created=result?;
        ack(client,base,token,item)?;
        return Ok(created);
    }
    let filename=safe_name(&item.filename);
    let mut target=dir.join(&filename);
    // Existing user files are immutable: never overwrite, even if transfer
    // names are identical. Stable fallback suffix makes ACK retries idempotent.
    if target.exists() && !verified(&target,&item.sha256,item.size_bytes) {
        let prefix=&item.id[..8];
        target=dir.join(format!("{prefix}-{filename}"));
        if target.exists() && !verified(&target,&item.sha256,item.size_bytes) {
            return Err(format!("File collision; choose a different receive directory: {filename}"));
        }
    }
    if verified(&target,&item.sha256,item.size_bytes) {
        ack(client,base,token,item)?;
        return Ok(false);
    }

    let temporary=dir.join(format!(".synchro-{}.part",item.id));
    let result=(|| -> Result<(),String> {
        let mut output=OpenOptions::new().write(true).create_new(true)
            .open(&temporary).map_err(|e|format!("Cannot create temp download: {e}"))?;
        let mut response=checked_response(client.get(format!("{base}/api/v1/transfers/{}/download",item.id))
            .bearer_auth(token).send().map_err(|e|e.to_string())?)?;
        let mut digest=Sha256::new();
        let mut count=0u64;
        let mut buffer=[0u8;64*1024];
        loop {
            let n=response.read(&mut buffer).map_err(|e|e.to_string())?;
            if n==0 { break; }
            count=count.saturating_add(n as u64);
            if count>item.size_bytes { return Err("Received more bytes than expected".into()); }
            output.write_all(&buffer[..n]).map_err(|e|e.to_string())?;
            digest.update(&buffer[..n]);
        }
        if count!=item.size_bytes
            || !format!("{:x}",digest.finalize()).eq_ignore_ascii_case(&item.sha256) {
            return Err("Download SHA-256 mismatch: file not published".into());
        }
        output.sync_all().map_err(|e|e.to_string())?;
        drop(output);
        if target.exists() {
            return Err("Destination changed during download; no overwrite".into());
        }
        fs::rename(&temporary,&target).map_err(|e|format!("Cannot publish file: {e}"))?;
        Ok(())
    })();
    if result.is_err() {let _=fs::remove_file(&temporary);}
    result?;
    // The file is already safely stored even if ACK fails. Next poll can retry ACK.
    ack(client,base,token,item)?;
    Ok(true)
}

/// Poll only when user has opted in. Called in a background thread; never in the UI thread.
pub(super) fn receive(settings: &Settings) -> Result<String,String> {
    if !is_ready(settings) { return Err("Select HTTPS server and a local folder to receive files".into()); }
    let base=crate::internet::origin(&settings.address)?;
    let folder=PathBuf::from(settings.receive_folder.trim());
    if !folder.is_dir() { return Err("Received-files directory does not exist".into()); }
    let token=settings.token.trim();
    let client=Client::builder().redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(180))
        .build().map_err(|e|e.to_string())?;
    let inbox=checked_response(client.get(format!("{base}/api/v1/inbox"))
        .bearer_auth(token).send().map_err(|e|e.to_string())?)?
        .json::<Vec<Inbound>>().map_err(|e|e.to_string())?;
    let mut downloaded=0usize;
    let mut already=0usize;
    let mut skipped=0usize;
    let mut first_issue=String::new();
    for item in inbox {
        if item.status!="ready" { continue; }
        match receive_one(&client,&base,token,&folder,&item,&settings.backup_key) {
            Ok(true) => downloaded+=1,
            Ok(false) => already+=1,
            Err(e) => {
                skipped+=1;
                if first_issue.is_empty() { first_issue=e; }
            }
        }
    }
    Ok(format!("ПК: нових {downloaded}, вже збережено {already}, не оброблено {skipped}. {first_issue}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn avoids_unsafe_windows_names() {
        assert_eq!(safe_name("../../photo?.jpg"), "....photo.jpg");
        assert_eq!(safe_name("CON.txt"), "synchro-file.bin");
        assert_eq!(safe_name("x\\y:foo.jpg"), "xyfoo.jpg");
        assert_eq!(safe_name(""), "synchro-file.bin");
    }
}

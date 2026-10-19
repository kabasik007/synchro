//! HTTPS Internet Mode sender. The relay's device token never goes into URLs.
//! Browser E2EE is not silently claimed here: native Rust encryption is pending.
use crate::{DeviceInfo, Event, Settings, MAX_SIZE, hex_digest};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::json;
use std::{fs::File, io::{Read, Seek, SeekFrom}, path::{Path, PathBuf},
          sync::mpsc::Sender, time::{Duration, SystemTime, UNIX_EPOCH}};
const CHUNK: u64 = 8 * 1024 * 1024;

#[derive(Deserialize)]
struct RelayDevice { id: String, name: String }
#[derive(Deserialize)]
struct RelayTransfer {
    id: String, recipient_id: String, filename: String, sha256: String,
    size_bytes: u64, status: String, expires_at: i64,
    #[serde(default)] encrypted: bool,
}
#[derive(Deserialize)]
struct Created { transfer: RelayTransfer }
#[derive(Deserialize)]
struct ChunkState { status: String, chunk_count: u64, uploaded_indices: Vec<u32> }

pub(super) fn origin(address: &str) -> Result<String, String> {
    let value=address.trim().trim_end_matches('/');
    let u=reqwest::Url::parse(value).map_err(|_| "Invalid HTTPS relay address".to_string())?;
    if u.scheme()!="https" || u.host_str().is_none() || !u.username().is_empty() ||
        u.password().is_some() || u.path()!="/" || u.query().is_some() || u.fragment().is_some() {
        return Err("Enter only a clean HTTPS origin (e.g. https://disk.sitezilla.net)".into());
    }
    Ok(value.to_string())
}

fn valid(settings: &Settings) -> Result<(), String> {
    origin(&settings.address)?;
    if settings.token.len()!=64 || !settings.token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Internet Mode needs a 64-character device token".into());
    }
    Ok(())
}

fn client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(150))
        .redirect(reqwest::redirect::Policy::none())
        .build().map_err(|e| e.to_string())
}
fn error(response: reqwest::blocking::Response) -> String {
    let status=response.status();
    let reason=response.text().unwrap_or_default();
    format!("Relay HTTP {status}: {}",reason.chars().take(200).collect::<String>())
}

pub(super) fn probe(settings: &Settings) -> Result<DeviceInfo, String> {
    valid(settings)?;
    let c=client()?;
    let response=c.get(format!("{}/api/v1/me",origin(&settings.address)?))
        .bearer_auth(&settings.token).send().map_err(|e| e.to_string())?;
    if !response.status().is_success(){return Err(error(response));}
    let self_device: RelayDevice=response.json().map_err(|e| e.to_string())?;
    Ok(DeviceInfo {
        device: format!("Synchro Internet · {}",self_device.name),
        app_version: "0.5.0".into(), sdk: 0,
    })
}

fn get_json<T: for<'de> Deserialize<'de>>(c: &Client, url: &str, token: &str) -> Result<T, String> {
    let response=c.get(url).bearer_auth(token).send().map_err(|e| e.to_string())?;
    if !response.status().is_success() {return Err(error(response));}
    response.json().map_err(|e|e.to_string())
}
fn ready(c: &Client, base: &str, token: &str, id: &str) -> Result<(), String> {
    for _ in 0..150 {
        let state: RelayTransfer=get_json(c,&format!("{base}/api/v1/transfers/{id}"),token)?;
        match state.status.as_str() {
            "ready" | "delivered" => return Ok(()),
            "finalizing" => std::thread::sleep(Duration::from_secs(2)),
            other => return Err(format!("Finalization returned {other}, retry upload")),
        }
    }
    Err("Server is still finalizing. Retry to check the existing transfer.".into())
}

pub(super) fn send(
    files: Vec<PathBuf>, settings: Settings, tx: Sender<Event>
) -> Result<String,String> {
    valid(&settings)?;
    let recipient=settings.recipient_id.trim();
    // A recipient is chosen by its UUID from the server admin/device list.
    if recipient.len()!=36 || !recipient.bytes().all(|b| b.is_ascii_hexdigit() || b==b'-') {
        return Err("Enter the recipient device UUID from the server".into());
    }
    let c=client()?;
    let base=origin(&settings.address)?;
    let token=settings.token.trim();
    let mut sent=0;
    for path in files {
        let name=path.file_name().and_then(|v|v.to_str())
            .ok_or_else(||"Invalid filename".to_string())?.to_string();
        let meta=path.metadata().map_err(|e|e.to_string())?;
        if !meta.is_file() || meta.len()==0 || meta.len()>MAX_SIZE {
            return Err(format!("{name}: file must be between 1 byte and 2 GiB"));
        }
        tx.send(Event::Status(format!("SHA-256: {name}"))).ok();
        let sha=hex_digest(&path)?;
        if !unchanged(&path,&meta) {
            return Err(format!("{name}: file changed while hashing; retry"));
        }
        // Resume across restarts: search this device's authenticated outbox for an
        // unexpired transfer with identical destination, file size and checksum.
        let outbox: Vec<RelayTransfer>=get_json(&c,&format!("{base}/api/v1/outbox"),token)?;
        let current=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        let existing=outbox.into_iter().find(|t|t.filename==name &&
            t.recipient_id==recipient && t.size_bytes==meta.len() &&
            t.sha256.eq_ignore_ascii_case(&sha) && !t.encrypted && t.expires_at>current &&
            matches!(t.status.as_str(),"uploading"|"finalizing"|"ready"));
        let id=if let Some(transfer)=existing {transfer.id} else {
            let response=c.post(format!("{base}/api/v1/transfers")).bearer_auth(token)
                .json(&json!({"to_device_id": recipient,"filename":name,
                              "size_bytes":meta.len(),"sha256":sha,"encrypted":false}))
                .send().map_err(|e|e.to_string())?;
            if !response.status().is_success(){return Err(error(response));}
            let new: Created=response.json().map_err(|e|e.to_string())?;
            new.transfer.id
        };
        let url=format!("{base}/api/v1/transfers/{id}");
        let checkpoint: ChunkState=get_json(&c,&format!("{url}/chunks"),token)?;
        if checkpoint.status=="uploading" {
            let have: std::collections::HashSet<u32>=checkpoint.uploaded_indices.into_iter().collect();
            for n in 0..checkpoint.chunk_count {
                let offset=n*CHUNK;
                let length=(meta.len()-offset).min(CHUNK);
                if !have.contains(&(n as u32)) {
                    if !unchanged(&path,&meta) {return Err(format!("{name}: source changed during upload"));}
                    let mut f=File::open(&path).map_err(|e|e.to_string())?;
                    f.seek(SeekFrom::Start(offset)).map_err(|e|e.to_string())?;
                    let reader=f.take(length);
                    let response=c.put(format!("{url}/chunks/{n}"))
                        .bearer_auth(token).header("Content-Type","application/octet-stream")
                        .body(reqwest::blocking::Body::sized(reader,length))
                        .send().map_err(|e|format!("{name}, chunk {n}: {e}"))?;
                    if !response.status().is_success(){return Err(error(response));}
                }
                tx.send(Event::Progress(path.clone(),(offset+length).min(meta.len()),meta.len())).ok();
            }
            let response=c.post(format!("{url}/finish")).bearer_auth(token)
                .send().map_err(|e|e.to_string())?;
            if !response.status().is_success(){return Err(error(response));}
        }
        ready(&c,&base,token,&id)?;
        if unchanged(&path,&meta) {
            tx.send(Event::FileSent(path.clone())).ok();
        } else {
            tx.send(Event::Status(format!("{name}: changed during transfer, remains queued"))).ok();
        }
        sent+=1;
    }
    Ok(format!("{sent} file(s) uploaded to Synchro Internet Mode (recipient inbox)"))
}
fn unchanged(path:&Path, original:&std::fs::Metadata)->bool {
    path.metadata().map(|now| now.len()==original.len() &&
        now.modified().ok()==original.modified().ok()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_clean_https_origins() {
        assert!(origin("https://disk.sitezilla.net").is_ok());
        assert!(origin("https://cloud.example.org/").is_ok());
        assert!(origin("http://public.example.org").is_err());
        assert!(origin("https://user:secret@example.org").is_err());
        assert!(origin("https://example.org/path").is_err());
    }
}

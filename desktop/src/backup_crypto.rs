//! Authenticated streaming Synchro Backup envelope "SYNBKP01".
//! Compatible with Android BackupCrypto.kt. This does not decrypt browser
//! SYNCHRO1 files and intentionally fails closed for unsupported formats.
use aes_gcm::{aead::{Aead, Payload}, Aes256Gcm, KeyInit, Nonce};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

const MAGIC: &[u8; 8] = b"SYNBKP01";
const HEADER: usize = 40;
const RECORD: u64 = 1024 * 1024;
const MAX_PLAIN: u64 = 2 * 1024 * 1024 * 1024 - 40000;

pub(crate) struct DecryptResult {
    pub ciphertext_sha256: String,
    pub plaintext_sha256: String,
    pub plain_size: u64,
}

pub(crate) fn valid_key(hex: &str) -> bool {
    hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(crate) fn generate_key() -> String {
    use rand::RngCore;
    let mut secret = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut secret);
    hex::encode(secret)
}

/// Verify full ciphertext hash separately from plaintext authentication.
/// Return error BEFORE caller publishes the temporary output file.
pub(crate) fn decrypt_stream<R: Read, W: Write>(
    mut source: R, mut output: W, encrypted_size: u64, key: &str
) -> Result<DecryptResult, String> {
    if !valid_key(key) { return Err("Потрібен 64-символьний E2EE-ключ".into()); }
    let master = hex::decode(key).map_err(|_| "Invalid E2EE hex")?;
    let mut header = [0u8; HEADER];
    source.read_exact(&mut header).map_err(|e| format!("Short E2EE header: {e}"))?;
    if &header[0..8] != MAGIC {
        return Err("Unsupported encrypted format: expected SYNBKP01".into());
    }
    let size = u64::from_be_bytes(header[32..40].try_into().unwrap());
    if size == 0 || size > MAX_PLAIN {
        return Err("Invalid E2EE plaintext length".into());
    }
    let records = size.div_ceil(RECORD);
    let expected = HEADER as u64 + size + records * 16;
    if expected != encrypted_size {
        return Err("Malformed E2EE length / record count".into());
    }

    let mut kdf = <Hmac<Sha256> as Mac>::new_from_slice(&master)
        .map_err(|_| "Invalid backup key")?;
    kdf.update(&header[8..24]);
    kdf.update(b"SynchroBackup-v1");
    let subkey = kdf.finalize().into_bytes();
    let cipher = Aes256Gcm::new_from_slice(&subkey).map_err(|_| "Invalid cipher key")?;
    let mut ciphertext_sha = Sha256::new();
    let mut plaintext_sha = Sha256::new();
    ciphertext_sha.update(header);
    let mut processed = 0u64;
    for index in 0..records {
        let length = (size - processed).min(RECORD) as usize;
        let mut block = vec![0u8; length + 16];
        source.read_exact(&mut block)
            .map_err(|e| format!("Incomplete encrypted record {index}: {e}"))?;
        ciphertext_sha.update(&block);
        let counter = (index as u32).to_be_bytes();
        let mut nonce = [0u8; 12];
        nonce[..8].copy_from_slice(&header[24..32]);
        nonce[8..].copy_from_slice(&counter);
        let mut aad = [0u8; 44];
        aad[..HEADER].copy_from_slice(&header);
        aad[HEADER..].copy_from_slice(&counter);
        let plain = cipher.decrypt(Nonce::from_slice(&nonce),
            Payload { msg: &block, aad: &aad })
            .map_err(|_| format!("E2EE authentication failed at record {index}; wrong key or tampering"))?;
        if plain.len() != length { return Err("Unexpected decrypted record length".into()); }
        output.write_all(&plain).map_err(|e| e.to_string())?;
        plaintext_sha.update(&plain);
        processed += length as u64;
    }
    let mut more = [0u8; 1];
    if source.read(&mut more).map_err(|e| e.to_string())? != 0 {
        return Err("Unexpected bytes after E2EE records".into());
    }
    Ok(DecryptResult {
        ciphertext_sha256: format!("{:x}", ciphertext_sha.finalize()),
        plaintext_sha256: format!("{:x}", plaintext_sha.finalize()),
        plain_size: processed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn encrypted(plain: &[u8], hex_key: &str) -> Vec<u8> {
        let master=hex::decode(hex_key).unwrap();
        let mut header=Vec::from(&MAGIC[..]);
        header.extend([1u8; 16]); // fixed test salt
        header.extend([2u8; 8]);  // fixed test prefix
        header.extend((plain.len() as u64).to_be_bytes());
        let mut kdf=<Hmac<Sha256> as Mac>::new_from_slice(&master).unwrap();
        kdf.update(&header[8..24]);
        kdf.update(b"SynchroBackup-v1");
        let key=kdf.finalize().into_bytes();
        let cipher=Aes256Gcm::new_from_slice(&key).unwrap();
        let mut result=header.clone();
        for (index, chunk) in plain.chunks(RECORD as usize).enumerate() {
            let counter=(index as u32).to_be_bytes();
            let mut nonce=[0u8;12];
            nonce[..8].copy_from_slice(&header[24..32]);
            nonce[8..].copy_from_slice(&counter);
            let mut aad=header.clone();
            aad.extend(counter);
            result.extend(cipher.encrypt(Nonce::from_slice(&nonce),
                Payload{msg:chunk,aad:&aad}).unwrap());
        }
        result
    }

    #[test]
    fn round_trip_and_no_plaintext_leak_in_ciphertext() {
        let key="ab".repeat(32);
        let plain=vec![0x5Au8; RECORD as usize + 42];
        let cipher=encrypted(&plain,&key);
        assert_eq!(cipher.len() as u64, 40+plain.len() as u64+32);
        let mut decoded=Vec::new();
        let result=decrypt_stream(Cursor::new(&cipher), &mut decoded,
            cipher.len() as u64,&key).unwrap();
        assert_eq!(decoded,plain);
        assert_eq!(result.plain_size,plain.len() as u64);
        assert_eq!(result.ciphertext_sha256,format!("{:x}",Sha256::digest(&cipher)));
    }
    #[test]
    fn rejects_tamper_wrong_key_and_unknown_format() {
        let key="ad".repeat(32);
        let mut cipher=encrypted(b"private photo",&key);
        assert!(decrypt_stream(Cursor::new(&cipher),Vec::new(),
            cipher.len() as u64,&"00".repeat(32)).is_err());
        cipher[40]^=1;
        assert!(decrypt_stream(Cursor::new(&cipher),Vec::new(),
            cipher.len() as u64,&key).is_err());
        cipher[0]=b'X';
        assert!(decrypt_stream(Cursor::new(&cipher),Vec::new(),
            cipher.len() as u64,&key).is_err());
    }
}

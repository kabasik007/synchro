# Synchro v0.6.0 preview — Android → Windows E2EE Backup

## Status

Source + automated CI, **not** an independently security-audited or
disaster-recovery-tested production archive.

### New functionality

- Selected Android folders (SAF) use streaming AES-256-GCM with 1 MiB
  authenticated records. File contents are encrypted on the phone before
  any bytes are sent through the owner-operated HTTPS Relay.
- E2EE envelope SYNBKP01: 8-byte magic; 16-byte per-file CSPRNG salt;
  8-byte per-file nonce prefix; 8-byte big-endian plaintext length.
  Key = HMAC-SHA256(master_256, salt || "SynchroBackup-v1").
  Each record = AES-256-GCM(key, nonce=prefix || record_index_u32_BE,
  plaintext, AAD=40-byte_header || record_index_u32_BE).
  Every record is independently authenticated; no full-video RAM buffer.
- Android Keystore encrypts the shared 32-byte backup key at rest on Android;
  Windows keeps the 64-hex backup key in memory for the current session only.
  Create and save the key on Windows, then paste into Android.
- Resume reuses salt/nonce only when the SAME file content and master key
  match the stored local checkpoint. Server stores only the full ciphertext
  SHA-256 and ciphertext chunks.
- Windows decrypts the stream, checks ciphertext SHA-256, verifies every
  AES-GCM record and publishes the plaintext without overwriting existing
  files, then ACKs only after the local verified file is saved. Idempotent
  disk receipt is validated against the plaintext file.
- Rust + Android unit tests compare both encryptors against a fixed external
  SHA-256 golden vector, and tests reject wrong keys / tampering.

### Explicit limits

- This protects file **contents** on Relay. Relay still sees filenames,
  encrypted sizes, device IDs, transfer timing and the sender/recipient.
- Native Windows → Android manual transfers and Wi-Fi/Tailscale LAN
  transfers are **not** E2EE. Browser's legacy SYNCHRO1 encryption is not
  recognized by the new native Windows backup decryptor.
- Local Windows backup copies are decrypted; protect the Windows disk with
  BitLocker or equivalent and keep offline/offsite backups.
- Default Relay retention is around 72 hours. If the PC does not fetch files
  before expiration, the Relay copy is deleted. This is not permanent storage.
- Keep your 64-character backup key in a trusted password manager and an
  offline recovery copy. A lost key cannot be reset to decrypt old backups.
- Photo folder access must be selected explicitly on Android. Nested folders
  are currently excluded. Jobs require unmetered Wi-Fi and may run late.
- Android KeyStore key loss (reinstallation/device reset) will require
  re-entering the backed-up master key.
- No independent penetration test or large multi-device end-to-end
  transfer test has been performed. Use non-sensitive files for pilot tests.

### Build and acceptance

On Windows PowerShell:

    cd "M:\projects\synchro"
    git pull --ff-only
    .\scripts\build-all.ps1 -NoPull

For Linux Relay: backup all storage before updating; build server natively.
The existing v0.5.1 Relay protocol supports the encrypted=true transfer
flag and does not need to know encryption keys.

Test two devices with harmless photos/video, offline Wi-Fi interruptions,
resumed chunks, tampered ciphertext, wrong recovery key, duplicate names,
PC restart before ACK, and relay expiry. Confirm no plaintext is stored in
the Relay objects directory. E2EE backup **must not** fall back to
unencrypted uploads on any cryptographic error.

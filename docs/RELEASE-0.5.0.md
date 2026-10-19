# Synchro v0.5 — Internet Mode source milestone

STATUS: pushed to main. NOT COMPILED, DEPLOYED, OR VERIFIED ON THE USER'S VPS.

## Implemented

- Server: sender-authorized GET /api/v1/transfers/{id}/chunks returns present 8MiB blocks; POST /finish returns 202 and runs server-side verification in a Tokio worker; GET transfer status transitions to ready.
- SQLite: online VACUUM INTO snapshots, integrity_check, atomic rename, seven most recent copies in the private /var/lib/synchro/backups directory; copies are made at startup and daily.
- Database migration: additive encrypted boolean on transfers, user_version 3, preserves old devices/transfers; server has no access to E2EE keys.
- Browser: AES-256-GCM with random 32-byte key and 12-byte nonce, ciphertext SHA-256, key copied and sent through a separate channel; resume after a network failure within the same open tab.
- Android: manually activated HTTPS inbox receiver with MediaStore pending files, digest verification, optional E2EE decrypt and explicit ACK; old LAN receiver remains.
- Windows: HTTPS sending with device token and destination UUID, outbox recovery after restarting the desktop app, per-chunk skip and asynchronous completion polling; existing LAN/Tailscale mode remains.

## Cryptographic format

Encrypted payload: 8 ASCII bytes SYNCHRO1, 12 bytes random IV, and AES-256-GCM encrypted file bytes with authentication tag. SHA-256 and size refer to the transmitted ciphertext envelope, not plaintext. Each encrypted file has its own 64-hex-character key. Filename, sizes, device IDs and other metadata remain visible to the relay.

Keys are not sent to the server and are not saved to browser storage. Recipients must receive the key separately, and the E2EE scheme currently has no authenticated device-pairing/key-exchange protocol. Closing the browser tab loses its unsaved key and encrypted-upload resume state. Android E2EE uses the browser envelope format, with a 64MiB memory cap.

IMPORTANT: Windows Internet Mode uploads bytes without E2EE. Do not send confidential files via the Windows native uploader until client-side encryption is implemented. The UI explicitly warns users about this.

## Validation / release checklist

Run on a Rust-enabled Linux staging host:

    bash deploy/verify-source.sh
    cd server && cargo fmt --check && cargo test && cargo clippy --all-targets -- -D warnings && cargo build --release
    cd .. && node --check web/app.js

Compile Windows Desktop with cd desktop && cargo test && cargo build --release. Build Android with cd android && ./gradlew :app:assembleDebug (JDK 17 + Android SDK).

Optional staging smoke test (creates/revokes two device registrations and uploads ~8MiB):

    SYNCHRO_TEST_URL=https://disk.sitezilla.net SYNCHRO_TEST_ADMIN_TOKEN=YOUR_SECRET python3 scripts/smoke-internet.py

Before upgrading, STOP the Synchro service, back up the entire private /var/lib/synchro directory and /etc/synchro/synchro.env outside webroot, and preserve all device tokens. Then pull main, build/test, and follow the existing hardened aaPanel bootstrap and non-root publisher in docs/RELEASE-0.4.2.md. Do not overwrite the nginx vhost without checking its existing custom directives.

Verify /health reports version 0.5.0; test upload interruption, resume and 8MiB+ finalization; test browser-to-Android E2EE using a separately communicated secret key. Verify SQLite backup integrity, file permissions, device revocation and reverse-proxy timeout behavior.

Restore caution: reverting to an old binary after SQLite user_version 3 migration may be unsupported. Roll back by restoring a matching pre-upgrade database plus the corresponding file blobs.

## Still not complete

Native Windows E2EE, Windows native receiving, Android native sending, unattended Android/FCM delivery, secure QR device pairing, encrypted transfers over 64MiB, cross-restart resume of browser E2EE, off-site blob backup, restore drills, load tests and real VPS compile/deploy/device acceptance tests.

SQLite backup is only metadata; file storage is not protected by these snapshots. Never call Synchro v0.5 production-ready or fully encrypted across all clients without the missing validation and security features.
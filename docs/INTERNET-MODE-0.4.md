# Synchro v0.4 — Internet Mode

**Project:** disk.sitezilla.net · Rust + aaPanel + Nginx + SQLite

**Status:** the initial server and web client have been committed to GitHub. They have NOT been compiled on the user's VPS or deployed. Windows/Android native v0.3.1 continue working in LAN/Tailscale mode and are not connected to this server yet.

## Delivered source

- Rust Axum/Tokio relay listening on loopback TCP 127.0.0.1:8787.
- Device enrollment by administrator only; separate randomly generated 256-bit device tokens. Only SHA-256 hashes of device tokens are stored in SQLite.
- Admin device revocation and WebSocket disconnect for revoked devices.
- Authenticated device list, upload initialization, transfer in immutable 8 MiB chunks, SHA-256 finalization, private recipient inbox, downloads, explicit receipt acknowledgement and sender outbox.
- Files can wait for the recipient while offline. Default expiry is 72 hours; default reserved storage quota is 5 GiB; single file max is 2 GiB and there are 20 unfinished uploads per sender.
- HTTPS reverse-proxy and authenticated WebSocket notifications. Tokens never go in URLs.
- Responsive browser at disk.sitezilla.net to enroll devices, send/download up to 64 MiB per file, verify SHA-256 with WebCrypto and confirm delivery.

**The browser ↔ browser flow is the first usable internet exchange**, even before native clients support server mode. Browser authentication is session-only and loses the token on refresh; store the device token in a password manager.

**Not delivered yet:** Bunny S3 adapter, client-side E2EE, FCM push, native Rust/Kotlin relay adapters, background upload resume, multi-user accounts and automatic direct P2P fallback. The server stores file contents unencrypted at rest; do not use with highly confidential files until E2EE is implemented.

## VPS structure

| Directory | Contents |
| --- | --- |
| /www/wwwroot/disk.sitezilla.net | Static web/index.html, web/app.css, web/app.js only |
| /opt/synchro/bin | Compiled synchro-server binary |
| /etc/synchro | Private synchro.env configuration |
| /var/lib/synchro/synchro.sqlite3 | SQLite registry, transfers |
| /var/lib/synchro/files | Chunks and ready file blobs |
| 127.0.0.1:8787 | Loopback-only HTTP API consumed by Nginx |

Never put API secrets, file storage or databases under /www/wwwroot.

## 1. Compile on Ubuntu VPS

Review these commands before running them as a sudo-enabled user. Install a current stable Rust toolchain, Git and basic C build tools first. Avoid altering your existing aaPanel Nginx or PHP installation.

    rustc --version
    cargo --version
    git clone https://github.com/kabasik007/synchro.git /opt/synchro-source
    cd /opt/synchro-source/server
    cargo test
    cargo clippy --all-targets
    cargo build --release

Then create a dedicated unprivileged service account and private data directories. If the user already exists, skip useradd.

    sudo useradd --system --home /var/lib/synchro --shell /usr/sbin/nologin synchro
    sudo install -d -m 0755 /opt/synchro/bin
    sudo install -d -o synchro -g synchro -m 0700 /var/lib/synchro
    sudo install -d -o synchro -g synchro -m 0700 /var/lib/synchro/files
    sudo install -d -o root -g root -m 0700 /etc/synchro
    sudo install -m 0755 target/release/synchro-server /opt/synchro/bin/synchro-server
    sudo install -m 0600 ../deploy/synchro.env.example /etc/synchro/synchro.env

Generate an administrator token using a trusted local source:

    openssl rand -hex 32

As root, immediately replace the example secret inside /etc/synchro/synchro.env with the generated **64 hexadecimal characters**. Keep this key private: do not place it in GitHub, messages, screenshots, Nginx arguments or a public website. The Rust service refuses the included non-hex example token. Recheck available disk space and the configured quota.

Install systemd service and start it:

    sudo install -m 0644 ../deploy/synchro-server.service /etc/systemd/system/synchro-server.service
    sudo systemctl daemon-reload
    sudo systemctl enable --now synchro-server
    sudo systemctl status synchro-server --no-pager
    curl -sS http://127.0.0.1:8787/health

Expected response: a JSON object with status=ok and version=0.4.0. If it fails, inspect logs with:

    sudo journalctl -u synchro-server -n 100 --no-pager

## 2. Connect disk.sitezilla.net to aaPanel

1. In aaPanel, create an HTTPS static website for **disk.sitezilla.net** with root /www/wwwroot/disk.sitezilla.net. You do not need PHP or an aaPanel SQL database for this static UI.
2. Copy only the contents of the repository's **web/** folder into the site root.
3. Open the HTTPS Nginx server block in aaPanel and insert the directives from **deploy/nginx-synchro.conf** inside that block. This snippet is not a standalone server block. If your aaPanel vhost already has a location /, merge/replace it to avoid duplicate-location errors. Keep existing SSL/ACME/Cloudflare real-IP settings.
4. Test and only then reload Nginx:

    sudo nginx -t && sudo systemctl reload nginx

5. Configure DNS and TLS for disk.sitezilla.net using your current Cloudflare domain setup. Optionally publish the Nginx site via an existing Cloudflare Tunnel; **do not** expose 8787 directly to the internet.
6. Verify HTTPS works for the static page and /health. Android and browsers need HTTPS for the WebCrypto digest API. Ensure your Cloudflare/Tunnel WebSocket option permits WSS.

The Nginx snippet sets a per-request 10 MiB limit because the client transfers chunks of 8 MiB. This avoids the Free/Pro per-request 100 MiB Cloudflare upload cap. It does not eliminate overall storage requirements or reverse-proxy connection timeouts on huge finalization operations; test a 1–10 MiB file first.

## 3. Enroll and transfer through browsers

Open https://disk.sitezilla.net on your Windows PC.

1. Expand the **Administrator setup** area.
2. Enter the secret administrator token from your private server env file.
3. Create a device named **Windows PC**, copy the generated **device token** once, and save it in a password manager.
4. Create another device named **Android Phone**; save its separate device token.
5. Enter the Windows device token in the main Connect area of the PC browser.
6. On the phone using Wi-Fi or 4G/5G, open the same HTTPS site, enter its Android device token and connect.
7. From Windows select **Android Phone**, choose a file up to 64 MiB, upload and finalize.
8. The phone receives a WebSocket notification when connected, or finds the new file by refreshing Inbox. It downloads the file and verifies SHA-256 before offering a browser save.
9. Only after saving tap **Delivered**. The server deletes the stored blob and preserves temporary metadata until pruning.

Reverse direction works identically. If the recipient is offline, the file waits in its inbox until expiry or explicit acknowledgement.

The browser keeps device and admin tokens in memory and never writes them to LocalStorage. Refreshing the tab will require reauthentication. A compromised browser or infected device can still expose a token, so treat tokens as secrets.

## Protocol

All private API endpoints require an Authorization header in the form **Bearer DEVICE_TOKEN**; admin endpoints require **Bearer ADMIN_TOKEN**.

| Method and route | Purpose |
| --- | --- |
| GET /health | Anonymous minimal health |
| POST /api/v1/admin/devices | Admin creates device and receives one-time token |
| POST /api/v1/admin/devices/{uuid}/revoke | Admin revokes device |
| POST /api/v1/admin/prune | Admin removes expired/delivered relay data |
| GET /api/v1/me | Authenticate device |
| GET /api/v1/devices | List available recipients |
| POST /api/v1/transfers | Initiate transfer using recipient id, filename, byte size, SHA-256 |
| PUT /api/v1/transfers/{uuid}/chunks/{index} | Immutable, fixed-size 8 MiB chunk; idempotent retries |
| POST /api/v1/transfers/{uuid}/finish | Assemble and verify SHA-256 |
| GET /api/v1/outbox | Sender's transfers for recovery |
| GET /api/v1/transfers/{uuid} | Transfer status for participants |
| GET /api/v1/inbox | Recipient's pending ready transfers |
| GET /api/v1/transfers/{uuid}/download | Authorized download with checksum header |
| POST /api/v1/transfers/{uuid}/ack | Acknowledge after SHA-256 verification |
| GET /ws | WebSocket event stream |

Example transfer-init request JSON:

    {
      "to_device_id": "RECIPIENT_UUID",
      "filename": "app-debug.apk",
      "size_bytes": 123456,
      "sha256": "REPLACE_WITH_REAL_64_DIGIT_SHA256"
    }

The server replies with a transfer ID, 8 MiB chunk size and count. Send chunk bodies in binary in numeric index order (retries are permitted). After sending all chunks call /finish, then the recipient can download. The final delivery acknowledgement expects JSON with a sha256 property matching the verified result.

WebSocket clients open wss://disk.sitezilla.net/ws and authenticate in their **first message**:

    {"type":"auth","token":"DEVICE_TOKEN"}

Never include device tokens in URLs.

## Next steps

1. Compile Rust server and validate two-browser transfer, abort, retry, storage quota, SHA-256 mismatch and wrong-token rejection.
2. Integrate the Internet Mode API into the existing Windows Rust Desktop and Kotlin Android clients; preserve their LAN/Tailscale paths.
3. Add a Bunny S3-compatible storage adapter with content expiration policies and encrypted-at-rest objects.
4. Add client-side end-to-end encryption and human-confirmed QR pairing before handling confidential files.
5. Add FCM wake-up plus Android-compliant background work rather than trying to run an unrestricted always-on receiver.

**Do not label the service production-ready until it builds, is deployed, and passes real-device tests.**

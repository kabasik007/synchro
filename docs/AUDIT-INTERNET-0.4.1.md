# Synchro Internet Mode — engineering audit & design decisions

**Date:** 2026-10-19  
**Baseline inspected:** \`main\`, \`194385e3a91483f778090fbdec4e368725ca94fa\` (v0.4.0)  
**Fix target:** v0.4.1 with aaPanel browser installer (see latest \`main\`)  
**Method:** static inspection of Rust Axum server, SQLite schema, browser client, Nginx, systemd and deployment scripts. Web JavaScript parse checks passed. **No Rust compiler was available in the audit environment, so no successful build or production deployment is claimed.**

## Critical findings & remediations

| ID | Severity | Observation | v0.4.1 status |
|---|---|---|---|
| IN-01 | P1 / compile risk | Raw \`axum::body::Body\` was being used as the last handler extractor. Axum documents \`Request\`/ \`Bytes\` as request body extractors; handler typing may fail. | Changed chunk handler to take \`axum::extract::Request\` and consume \`request.into_body()\`. **Needs cargo check**. |
| IN-02 | P1 / resource exhaustion | Chunk uploads buffer up to 8 MiB per request, but no concurrency or request-size cap. Many concurrent requests can exhaust a small VPS. | Bounded concurrent chunk buffers (3 permits); global 8 MiB + headers body cap using RequestBodyLimitLayer. |
| IN-03 | P1 / storage exhaustion | Quota reserved only logical file bytes. While finalizing, both chunks and assembled file consume space, potentially nearly double. | Reserve 2× file size for incomplete transfers; check filesystem free space with 128 MiB headroom. |
| IN-04 | P1 / cleanup | Pruner previously removed DB records even when a filesystem delete failed, creating untracked disk usage. | Keep DB metadata if object/chunk deletion fails, retry on next prune. |
| IN-05 | P1 / deployment | No browser installer and a hardcoded domain in Nginx CSP made domain moves awkward. | Added /install/ wizard, authenticated one-shot setup endpoints, Nginx CSP using $host and admin domain update API. |
| IN-06 | P1 / provisioning | User had to manually create devices using the main web admin form; no preflight check for SQLite/storage permissions. | Setup checks DB integrity, writable storage and actual free disk; creates Windows/Android tokens atomically and permanently locks the wizard. |
| IN-07 | P1 / credentials | The initial admin token must not be embedded in publicly served frontend source. | Bootstrap creates a private 64-hex secret in root-owned /etc/synchro/synchro.env; entered in browser only over HTTPS. |
| IN-08 | P2 / automation | Existing aaPanel installation required scattered manual shell steps. | Added guarded deploy/bootstrap-aapanel.sh to install the prebuilt binary, systemd unit and web files; it never rewrites the aaPanel vhost or exposes port 8787. |

## Remaining risks / unresolved

| ID | Severity | Follow-up |
|---|---|---|
| IN-09 | P1 / end-to-end privacy | Server stores **plaintext file bytes at rest**, and HTTPS is typically terminated by Nginx or Cloudflare. Implement client-side authenticated encryption (XChaCha20-Poly1305 or equivalent) and cryptographic device pairing before storing confidential data. |
| IN-10 | P1 / incomplete client work | Windows/Android *native* v0.3.1 apps are not wired to the new relay API. Current Internet Mode first works via two browser tabs, then add native HTTP/WebSocket clients. |
| IN-11 | P1 / long requests | Finalizing a 2 GiB upload hashes and assembles the whole file while one HTTP request remains open. Cloudflare/proxies can time out before completion. Move finalization to a background job with status polling/notifications. |
| IN-12 | P2 / abuse protection | Admin tokens are strong, but the service has no complete per-IP/device request rate limiter or anti-DoS quota for auth failures. Add Nginx limit_req / limit_conn, app-level limits and audit log without secrets. |
| IN-13 | P2 / database lifecycle | Schema is created automatically; no numbered migrations, automatic online backup or restore tests. Add migrations with PRAGMA user_version, SQLite online backup, WAL-aware restore, integrity checks. |
| IN-14 | P2 / idempotency | If browser loses the HTTP response to setup/finish after DB commit, one-time device tokens cannot be recovered. Admin can create replacement devices in the main web UI; expose a clear recovery workflow. |
| IN-15 | P2 / storage cleanup | Crashes can leave .partial files and stale assembling blobs. Expiry cleanup deletes transfer-owned chunks, but orphan sweeping across unused directories is not yet implemented. |
| IN-16 | P2 / synchronization | Sender may acknowledge a transfer simultaneously with the recipient's download, or expiry prune may overlap with finalization. Use explicit per-transfer lease/locks or DB state transactions; test these races. |
| IN-17 | P2 / browser | The browser currently hashes and buffers up to 64 MiB of file data in memory. Streaming hash/download, resumable uploads after refresh and richer progress are future work. |
| IN-18 | P2 / deployment | The Nginx snippet must be merged with aaPanel's **actual** site config; existing location blocks/rewrite snippets can conflict. Never replace an unknown working vhost wholesale. |
| IN-19 | P2 / VPS capacity | Even with quota reservation, on-disk operational logs, DB WAL, transient buffers and other aaPanel websites share storage. Keep generous headroom; do not set quota equal to total disk space. |
| IN-20 | P3 / product UX | Device token enrollment is administrator-driven, not automatic QR pairing; user-friendly phone/PC native discovery, client-side encryption and FCM are later milestones. |

## Built-in operational analytics

The administrator-only `GET /api/v1/admin/stats` endpoint now reports active devices, uploads, ready deliveries, delivered metadata rows, logical reserved storage, quota, real free disk, retention and saved public URL. The main web UI includes an on-demand panel for this data. It is **not a third-party tracking integration**, and no visit/personal usage analytics are collected automatically.

## Domain and aaPanel architecture

The domain is a deployment address, not a hard-coded app identity. The browser uses **relative same-origin API paths** and a WebSocket URL based on the currently opened hostname. Nginx CSP now follows the current Host header instead of fixing the old domain. The setup wizard can store a public_url in SQLite; a post-install admin API and UI can change that stored address.

**What changing the domain still requires:** update aaPanel website domain / Nginx server_name, configure DNS or Cloudflare Tunnel, obtain/install a valid HTTPS certificate, check CSP and request origins, and update any native clients holding the prior address. The application cannot safely manage those infrastructure steps itself from a web page.

**Recommended local directories**

- Public static pages: \`/www/wwwroot/<chosen-domain>/\` (only \`web/\` contents).
- Rust binary: \`/opt/synchro/bin/synchro-server\`.
- Private env: \`/etc/synchro/synchro.env\`, root-owned 0600.
- SQLite: \`/var/lib/synchro/synchro.sqlite3\` plus WAL/SHM.
- File chunks/blobs: \`/var/lib/synchro/files\`; later optional Bunny S3.

## DB choice: SQLite vs MariaDB

Use **SQLite** for the current single-owner Synchro:

- No DB service or MariaDB root credentials to configure in aaPanel.
- WAL allows readers and one writer, enough for a personal device registry and inbox queue.
- Small installation footprint, schema bootstrapped by Rust.
- Metadata only (devices, hashed access tokens, transfers, public_url, setup lock). The actual files live on disk or future S3.
- No PHP/database account needed in the aaPanel Website creation UI.

MariaDB becomes relevant only for high concurrency, public multi-tenancy, distributed workers, sophisticated admin reporting and replicas. Switching is not a drop-in toggle: a database adapter and migration would be required. Do not claim that the current Rust service can use MariaDB.

Back up SQLite consistently with SQLite online backup / VACUUM INTO, not a naïve copy of only the .sqlite3 file while WAL is active.

## Installer trust boundaries

The web wizard at \`https://YOUR-DOMAIN/install/\` is **not** an arbitrary-command system installer. It:
1. checks browser HTTPS, static files, /health and setup status;
2. asks for the administrator secret from the private VPS env file;
3. checks SQLite integrity and writable storage, real disk free space, configured quota and expiry;
4. records the chosen HTTPS origin and provisions two separate device tokens in **one SQLite transaction**;
5. marks installation complete, locking initial provisioning, and shows each token once.

A privileged bootstrap step must still install the Rust binary and systemd service and configure the existing aaPanel HTTPS site. The provided root-run shell bootstrap script checks the site root, preserves secrets on re-runs, and doesn't rewrite Nginx/SSL settings. This is safer than allowing an internet-accessible page to execute shell commands as root.

## Performance / capacity planning

The v0.4 relay uses 8 MiB transfer chunks. The web client caps files at 64 MiB; the server API accepts up to 2 GiB per file. During final assembly **chunks and finished bytes coexist**, so use ~2× the file size plus additional headroom for disk planning. The default 5 GiB quota is a *reservation policy*, not a measurement of the VPS total capacity; the installer reports both quota and filesystem free space.

Data path in Internet Mode: sender uploads → server stores chunks → server verifies SHA-256 and assembles blob → recipient downloads and verifies → recipient acknowledges → server frees storage. Offline recipient is supported; always-on native Android background synchronization is **not yet implemented**.

## Validation checklist before production

- Compile Rust \`cargo test\`, \`cargo clippy --all-targets\`, \`cargo build --release\` on a supported Linux build environment.
- Verify browser installer check state and single-use setup; check that wrong admin token is rejected and device tokens are never exposed after refresh.
- With two distinct device tokens, send a small ZIP from Windows Wi-Fi to Android cellular, then reverse direction; verify SHA-256 and ACK cleanup.
- Test 0 bytes, 1 byte, 8 MiB boundary and multiple chunks; corrupt one chunk, use wrong token, expire a transfer.
- Test DB/filesystem read-only, full disk, server restart during chunk upload, and installer interrupted after commit.
- Verify \`nginx -t\`, TLS certificate, WebSocket connectivity, /install/ and login on the final domain.
- Confirm port 8787 is bound only to 127.0.0.1, no backup/secrets appear under website root, and Nginx/PHP sites still work.

**Do not publish a "production-ready" claim until these checks succeed.**

# Synchro v0.4.2 — Security Hotfix (source release candidate)

**Baseline audit:** [Synchro v0.4.1 security findings](SECURITY-AUDIT-0.4.1-2026-10-19.md).  
**Status:** changes committed to main; **Rust compilation and actual deployment/transfer on aaPanel are NOT yet verified**. Do not expose to untrusted users until cargo build/test and end-to-end acceptance tests pass.

## Fixed in source

1. **SEC-01 — root bootstrap / webroot symlink trust boundary:** `deploy/bootstrap-aapanel.sh` **never copies files into the aaPanel public website directory as root**. It stages an allowlist of six static assets plus publisher script in root-owned `/opt/synchro/staged-web`. The explicit publisher `deploy/publish-web.sh` REFUSES root, rejects symlinked directories and targets, and atomically swaps a temporary file into each allowed website file.
2. **SEC-02/03 — PUT/finish/prune races:** a conservative process-local Tokio mutation mutex serializes file mutations and expiry cleanup. `PUT` revalidates the transfer status **after receiving the request body AND acquiring the lock**. Finishing and pruning hold the same gate while changing metadata and touching disk. For a single-owner VPS this is safer than a complex per-transfer lock implementation, at the expense of lower multi-file throughput.
3. **SEC-04 — DoS bounds:** max 32 WebSocket sessions, max four concurrent download streams, max three concurrent buffered 8-MiB chunk requests. Nginx has IP-scoped request/connection limits; two snippets are required: `deploy/nginx-http-limits.conf` goes INSIDE nginx `http {}` and `deploy/nginx-synchro.conf` goes INSIDE the intended HTTPS `server {}`. **Never use only one snippet.**
4. **SEC-06 — premature delivery ACK:** the web button is disabled until a session has downloaded the file and verified its SHA-256. Browser save is not technically guaranteed, so explicit user confirmation still applies. Server retains acknowledged blobs for **up to one hour (or until the original expiry)**, counts this retained space against quota, offers redownload during that grace interval, then cleans them up.
5. **P2 hardening included:** same-origin check for browser WebSocket handshakes (native clients without Origin still require device token), idempotent `/finish` for ready transfers, Nginx `proxy_buffering off` for protected file downloads, deny patterns for accidentally exposed secrets/backups, and `disable_symlinks on` for static site files.
6. **SQLite compatibility:** migration adds the `revoked` column if missing on older v0.4.0 databases, preserves existing devices, and records `PRAGMA user_version=2`.
7. **Regression tests:** Rust tests cover the mutation lock, acknowledged-file retention and legacy SQLite schema migration.

### Not yet fixed / remaining security risks

- **No E2EE:** all uploaded file data remains readable on the server disk; TLS to the VPS/Cloudflare is not end-to-end encryption. Do not use for secret documents.
- **ACK cannot prove local storage:** even after verified bytes are downloaded, a browser can fail its save dialog. Grace period reduces accidental loss but is not a formal delivery proof; an authorized recipient can deliberately confirm before downloading via direct API call.
- The **mutation mutex is process-local**: do not run multiple Synchro server instances against the same SQLite and object directory. Future per-transfer DB leases or distributed locks are needed for horizontally scaled deployment.
- Finalization still assembles the file synchronously inside an HTTP request; huge files can time out through Cloudflare/Nginx. Background finalization is planned for v0.4.3.
- No native Windows/Android Internet Mode client integration yet; browser-to-browser via HTTPS is still the first path.
- SQLite numbered schema migrations, coordinated online backup/restore, orphan scanning, streaming browser SHA for files above 64 MiB, Bunny S3, audit logging without tokens and production load testing remain planned.
- Nginx per-IP limits need accurate trusted source addresses when using Cloudflare Proxy/Tunnel. Do not blindly trust client-supplied X-Forwarded-For.

## Deployment: IMPORTANT new ordering

**Preparation in aaPanel:** create a static website with HTTPS and root at `/www/wwwroot/disk.sitezilla.net` (or another hostname). Remove aaPanel's default index page *only after confirming the folder belongs to Synchro*. Database selection: **none**, since SQLite is created by the Rust server.

Run as your SSH user on Ubuntu:

```bash
cd ~/synchro-source
git pull --ff-only
cd server
cargo test
cargo clippy --all-targets
cargo build --release
cd ..
```

Only after all commands pass, use the one-time privileged bootstrap:

```bash
sudo bash deploy/bootstrap-aapanel.sh disk.sitezilla.net
```

Now publish the **staged** static website files as the aaPanel website owner. On many installations it is `www`, but inspect the actual user in aaPanel:

```bash
sudo -u www bash /opt/synchro/staged-web/deploy/publish-web.sh disk.sitezilla.net
```

**Do not run `publish-web.sh` as root.** The bootstrap creates root-owned staged assets so the site account can read them even if `~/synchro-source` is not accessible. If the site owner is not `www`, use that correct unprivileged account instead.

Next merge two Nginx snippets into the correct contexts:

- `deploy/nginx-http-limits.conf` -> **once** inside Nginx `http {}`.
- `deploy/nginx-synchro.conf` -> inside **only the Synchro HTTPS** `server {}`, merging any existing `location /` rule rather than duplicating it.

Then validate syntax and reload only if test passes. Use the Nginx binary actually managed by aaPanel:

```bash
sudo nginx -t && sudo systemctl reload nginx
curl -fsS http://127.0.0.1:8787/health
```

The expected JSON version is `0.4.2`. Open `https://disk.sitezilla.net/install/` only after proxy and static site pass. If the site was already installed and has valid SQLite data, it remains installed — **do not delete the database or regenerate existing device tokens**.

## Mandatory validation

| Test | Expected |
| --- | --- |
| `bash -n deploy/bootstrap-aapanel.sh deploy/publish-web.sh` | No shell parse errors |
| `cargo test` / `cargo clippy --all-targets` / `cargo build --release` | All pass; no unaddressed errors |
| Root runs `publish-web.sh` | Refused |
| Site user plants symlink at `app.js` or `install/` | Publisher refuses |
| `PUT` is in progress while `/finish` and prune run | Serialized and recoverable, no false HTTP 201/ready |
| ACK after verified download | Copy still downloadable for one hour (unless original TTL expires sooner) |
| ACK without browser verified SHA | UI refuses |
| >32 WS sessions or >4 streamed downloads | Excess connections rejected, stable memory |
| HTTP request flood to API | Nginx 429/limit_conn triggers |
| Proxy buffering and 8 MiB chunk uploads | No Nginx temp-file growth from downloads |
| `nginx -t` with BOTH contexts | Passes with the actual aaPanel vhost |
| Desktop Wi-Fi vs Android cellular browser | Two-way small file transfer, matching SHA and explicit receipt |

This is source code + documentation, not a claim that the VPS was modified or that all regression tests passed.

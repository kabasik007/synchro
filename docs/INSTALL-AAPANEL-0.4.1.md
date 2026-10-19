# Synchro v0.4.1 — aaPanel + Nginx + Browser Install Wizard

## Goal

Create an HTTPS website on **any chosen hostname** (example: disk.sitezilla.net) and finish the one-time Synchro setup in a browser at:

**https://YOUR-DOMAIN/install/**

The actual **Rust service** runs as systemd, the **web UI** is static under aaPanel Nginx, **SQLite** and files are private. Existing OpenCart/PHP websites are untouched.

**This has not been installed on your VPS from ChatGPT.** Commands below require your own server shell/aaPanel access.

## 1. Requirements

- Ubuntu or compatible Linux with aaPanel Nginx; DNS/Cloudflare hostname pointing to Nginx and working HTTPS.
- Rust stable toolchain (installed separately), cargo, compiler toolchain, Git and OpenSSL.
- At least a few GiB of free space **after** the server and other websites are accounted for. The default logical storage quota is 5 GiB and the maximum protocol file is 2 GiB.
- A sudo/root shell for a one-time service bootstrap.
- Never paste the actual administrator token into chat, screenshots or Git commits.

## 2. Create your domain in aaPanel

In **aaPanel → Website → Add site**:

1. Choose a domain such as disk.sitezilla.net (this is a default suggestion only).
2. Root: **/www/wwwroot/disk.sitezilla.net** (aaPanel normally offers this path).
3. Site type: static; **no PHP required**.
4. **No MySQL/MariaDB database** needed — Synchro's Rust server creates its private SQLite database automatically.
5. Configure HTTPS with a valid certificate, including any Cloudflare/Tunnel settings. Keep DNS and certificate under your control.

If you change domain later, update DNS/Cloudflare, Nginx server_name and TLS certificate. In the Synchro administrator area, update public_url. Clients using relative URLs keep working when opened on the new domain, but existing native clients with saved absolute addresses must be changed.

## 3. Clone, compile and bootstrap

Connect through aaPanel Terminal or SSH:

    cd /opt
    sudo git clone https://github.com/kabasik007/synchro.git synchro-source
    cd /opt/synchro-source/server
    cargo --version
    cargo test
    cargo clippy --all-targets
    cargo build --release

If cargo isn't available with your current Linux account, install a supported Rust stable toolchain first. Avoid running unknown remote install scripts as root.

The bootstrap requires a **precompiled** executable at server/target/release/synchro-server and an **already existing** aaPanel site directory. Return to the repository root and run:

    cd /opt/synchro-source
    sudo bash deploy/bootstrap-aapanel.sh disk.sitezilla.net

The script:
- checks that you created the correct aaPanel site and that it is not another website;
- creates the unprivileged **synchro** system account;
- installs the binary into /opt/synchro/bin/;
- creates **private** /var/lib/synchro and /etc/synchro paths;
- generates an initial cryptographically random administrator secret if no private .env exists, **without printing the key**;
- installs and starts a hardened systemd service;
- copies only web/ assets to the aaPanel site root;
- **does not modify Nginx**, alter TLS, touch other site configs or expose TCP port 8787.

Check the local API:

    sudo systemctl status synchro-server --no-pager
    curl -fsS http://127.0.0.1:8787/health

Expected JSON: {"status":"ok","version":"0.4.1"}.

## 4. Add the aaPanel Nginx reverse proxy

Open your exact HTTPS site config in aaPanel. Merge the directives in:

**deploy/nginx-synchro.conf**

**inside the HTTPS server block**. Do **not** paste it as an entire replacement Nginx config. In particular, do not duplicate an existing location / block: merge it carefully, preserving SSL, certificate renewals and existing aaPanel handling.

This routes /api/v1/ and /ws to 127.0.0.1:8787; static /install/ and other web assets come from the site root. Nginx CSP references the current host dynamically, rather than a fixed domain.

Test:

    sudo nginx -t
    sudo systemctl reload nginx

If nginx -t fails, **do not reload**; correct the conflict first. Your aaPanel may use an Nginx binary at /www/server/nginx/sbin/nginx instead of the PATH nginx executable; prefer the actual installed binary.

Now open https://disk.sitezilla.net/install/ (replace with the domain you chose).

## 5. Use the browser installer

The installer is a **responsive four-step web application** styled to match Synchro:

1. **Checks** — HTTPS works, /health returns a server result, /api/v1/setup/status confirms installation isn't finished.
2. **Server and SQLite** — enter administrator secret, validate SQLite integrity, test storage write permissions, see actual free disk space, logical quota and retention.
3. **Devices** — name your Windows PC and Android. Both receive separate cryptographically strong tokens, created in one SQLite transaction.
4. **Finish** — copy both tokens, store them in a password manager, and go to the Synchro Disk homepage.

To read the private administrator secret, use an authorized private server shell. It is stored in:

**/etc/synchro/synchro.env**

Only inspect that file yourself as root; don't expose it in a screenshot or share it. The browser never receives the token automatically. The setup page only sends it in an Authorization header over HTTPS when you explicitly initiate a check or finish.

After successful setup, the Rust API marks installation finished in SQLite. The web installer then shows a locked state and won't generate initial keys again. If setup succeeded but the browser lost the one-time response, use the admin device registration in the main web UI to create **replacement** credentials — original tokens are deliberately not recoverable from their stored hashes.

## 6. Browser-to-browser Internet Mode test

On Windows, open the new HTTPS Synchro website and connect using your **Windows PC device token**. On the Android phone, turn Wi-Fi off (use 4G/5G), open the same HTTPS site, and connect using its **Android device token**. Select the other device, choose a small file (start with <10 MiB) and send.

The recipient gets a notification through WebSocket while connected or will see the file in Inbox later. Download, verify the SHA-256 checksum and confirm delivery only after saving the file. This flow supports offline recipients and reverse direction.

**The first working integration is two browsers.** Existing native Windows and Android apps do not yet use this Internet Mode API. Do not promise a native background receiver until implemented.

## 7. Domain reconfiguration after setup

Inside the Synchro Disk main page, expand administrator setup and choose **Публічна адреса сервера**, enter the new HTTPS origin and authorize with the **admin token**. The API updates the SQLite public_url setting.

You also must:
- add/change hostname in aaPanel and your Nginx server_name;
- configure DNS / Cloudflare / Tunnel;
- issue/attach HTTPS certificate for the new hostname;
- deploy/copy public web assets to the new website root if it's a new aaPanel site;
- update any clients that have an old absolute server URL.

If using the same underlying SQLite and file directory, the devices and transfers remain; **do not recreate the database** for a domain change. Do not re-run the initial setup wizard.

## 8. SQLite maintenance

The server maintains /var/lib/synchro/synchro.sqlite3 in WAL mode. Device tokens are stored hashed; metadata and settings are stored as JSON/text/numbers. The actual uploaded file bytes live under /var/lib/synchro/files.

For a consistent backup, stop the service briefly or use SQLite's online backup API; copying only the .sqlite3 file while WAL is active is unsafe. Back up **both the database and file storage**, and keep a protected copy of /etc/synchro/synchro.env. Set up restricted filesystem permissions and retention.

For current single-owner use, SQLite is simpler than MariaDB. A MariaDB option would require a separately implemented database backend and migration, not a checkbox in the installer.

## 9. Security and limits

- Rust binds **only** 127.0.0.1:8787; Nginx handles HTTPS.
- Browser cannot execute Linux commands or edit aaPanel configurations; only root-running bootstrap/systemd can do that.
- The administrator token is 64 hex characters generated with OpenSSL; initial device tokens are 256-bit random values shown once.
- Browser transfers are limited to 64 MiB; native/server protocol limit is 2 GiB, though large-file finalization is not yet fully asynchronous and can time out at proxies.
- File bytes are **not yet end-to-end encrypted**; HTTPS protects transport but the server operator can access content at rest.
- SQLite and the file store must be **outside wwwroot**.
- An Nginx rate limiter, authentication audit logs, client E2EE, resumable native transfers, Bunny S3 and automatic device discovery remain future work.

See [v0.4.1 audit](AUDIT-INTERNET-0.4.1.md) for severity, remaining risks and test matrix. Do not treat v0.4.1 as production ready without cargo checks, Nginx tests and two-network transfer tests.

#!/usr/bin/env bash
# Bootstrap Synchro under aaPanel. Finish at https://DOMAIN/install/.
# Intentionally does NOT modify an aaPanel Nginx virtual host or TLS configuration.
set -Eeuo pipefail
umask 077

if [[ "$EUID" -ne 0 ]]; then
  echo "Run as root: sudo bash deploy/bootstrap-aapanel.sh disk.sitezilla.net" >&2
  exit 1
fi
DOMAIN="${1:-}"
if [[ ! "$DOMAIN" =~ ^[a-zA-Z0-9]([a-zA-Z0-9.-]{0,251}[a-zA-Z0-9])?$ ]] ||
   [[ "$DOMAIN" == *..* ]]; then
  echo "Provide a hostname, e.g. disk.sitezilla.net" >&2
  exit 2
fi
SCRIPT_DIR="$(cd -- "$(dirname -- "$0")" && pwd)"
REPO="$(cd -- "$SCRIPT_DIR/.." && pwd)"
WEBROOT="/www/wwwroot/$DOMAIN"
BIN="$REPO/server/target/release/synchro-server"
ENVFILE="/etc/synchro/synchro.env"
UNIT="/etc/systemd/system/synchro-server.service"

if [[ ! -d "$WEBROOT" ]]; then
  echo "First create the static HTTPS aaPanel site for $DOMAIN at $WEBROOT" >&2
  exit 3
fi
if [[ ! -x "$BIN" ]]; then
  echo "Build Rust first: cd $REPO/server && cargo test && cargo build --release" >&2
  exit 4
fi
if [[ -f "$WEBROOT/index.html" ]] && ! grep -qiE '<title>Synchro|Synchro Disk' "$WEBROOT/index.html"; then
  echo "Refusing to overwrite another website: $WEBROOT/index.html" >&2
  exit 5
fi
if ! command -v openssl >/dev/null 2>&1; then
  echo "OpenSSL is required to generate a secure secret." >&2
  exit 6
fi

if ! getent passwd synchro >/dev/null; then
  useradd --system --home /var/lib/synchro --shell /usr/sbin/nologin synchro
fi
install -d -m 0755 /opt/synchro/bin
install -d -o synchro -g synchro -m 0700 /var/lib/synchro
install -d -o synchro -g synchro -m 0700 /var/lib/synchro/files
install -d -o root -g root -m 0700 /etc/synchro
install -m 0755 "$BIN" /opt/synchro/bin/synchro-server

if [[ ! -e "$ENVFILE" ]]; then
  # Secret is never printed to the terminal, shell arguments or access logs.
  SECRET="$(openssl rand -hex 32)"
  cat > "$ENVFILE" <<EOF
SYNCHRO_ADMIN_TOKEN=$SECRET
SYNCHRO_BIND=127.0.0.1:8787
SYNCHRO_STORAGE_DIR=/var/lib/synchro/files
SYNCHRO_DB_PATH=/var/lib/synchro/synchro.sqlite3
SYNCHRO_STORAGE_QUOTA_BYTES=5368709120
SYNCHRO_TTL_HOURS=72
RUST_LOG=synchro_server=info
EOF
  chmod 0600 "$ENVFILE"
  echo "Private administrator token created at $ENVFILE."
else
  echo "Existing private settings preserved at $ENVFILE."
fi

install -m 0644 "$SCRIPT_DIR/synchro-server.service" "$UNIT"
systemctl daemon-reload
systemctl enable synchro-server
systemctl restart synchro-server

# Stage public files in root-owned /opt (NOT in the writable aaPanel site).
# Site owner can then publish even if repo is inside a private SSH home.
STAGING="/opt/synchro/staged-web"
install -d -m 0755 "$STAGING" "$STAGING/web" "$STAGING/web/install" "$STAGING/deploy"
for relative in index.html app.css app.js install/index.html install/setup.css install/setup.js; do
  install -m 0644 "$REPO/web/$relative" "$STAGING/web/$relative"
done
install -m 0644 "$SCRIPT_DIR/publish-web.sh" "$STAGING/deploy/publish-web.sh"

# SECURITY: root never writes to the site user-controlled webroot.
echo "Static assets staged at $STAGING. Publish as the aaPanel site owner:"
echo "  sudo -u www bash $STAGING/deploy/publish-web.sh $DOMAIN"

echo "Synchro prepared. Next:"
echo "  1. Publish staged files as the aaPanel site owner (often www)."
echo "  2. Merge deploy/nginx-synchro.conf into the HTTPS vhost for $DOMAIN."
echo "  3. Run: nginx -t && systemctl reload nginx"
echo "  4. Test: curl -fsS http://127.0.0.1:8787/health"
echo "  5. Open https://$DOMAIN/install/"
echo "  6. Privately read the admin secret from $ENVFILE."
echo "Nginx unchanged; Rust is bound to 127.0.0.1 only."

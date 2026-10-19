#!/usr/bin/env bash
# Static deployment for an existing aaPanel Synchro site. NEVER run as root.
set -Eeuo pipefail
umask 022
if [[ "$EUID" -eq 0 ]]; then
  echo "Refusing root deployment to aaPanel webroot. Run as website owner." >&2
  exit 1
fi
DOMAIN="${1:-}"
if [[ ! "$DOMAIN" =~ ^[a-zA-Z0-9]([a-zA-Z0-9.-]{0,251}[a-zA-Z0-9])?$ ]] ||
   [[ "$DOMAIN" == *..* ]]; then
  echo "Usage: bash deploy/publish-web.sh disk.sitezilla.net" >&2
  exit 2
fi
REPO="$(cd -- "$(dirname -- "$0")/.." && pwd)"
ROOT="/www/wwwroot/$DOMAIN"
if [[ ! -d "$ROOT" || -L "$ROOT" || -L "/www/wwwroot" ]]; then
  echo "Website directory missing or symlinked: $ROOT" >&2
  exit 3
fi
if [[ ! -w "$ROOT" ]]; then
  echo "Current user cannot write site root. Use aaPanel File Manager or site owner." >&2
  exit 4
fi
if [[ -L "$ROOT/index.html" ]]; then
  echo "Refusing symlink in website index." >&2
  exit 5
fi
if [[ -f "$ROOT/index.html" ]] && ! grep -qiE '<title>Synchro|Synchro Disk' "$ROOT/index.html"; then
  echo "A different site exists here. Back it up/move it using aaPanel first." >&2
  exit 6
fi
if [[ -e "$ROOT/install" && ( -L "$ROOT/install" || ! -d "$ROOT/install" ) ]]; then
  echo "Refusing non-directory or symlinked install path." >&2
  exit 7
fi
mkdir -p -- "$ROOT/install"
# Only these six explicitly reviewed files are published.
for relative in index.html app.css app.js install/index.html install/setup.css install/setup.js; do
  source="$REPO/web/$relative"
  target="$ROOT/$relative"
  directory="$(dirname -- "$target")"
  if [[ ! -f "$source" || -L "$source" || -L "$target" || -L "$directory" ]]; then
    echo "Unsafe/missing web asset: $relative" >&2
    exit 8
  fi
  tmp="$(mktemp "$directory/.synchro-publish-XXXXXXXX")"
  if ! cp -- "$source" "$tmp"; then
    rm -f -- "$tmp"
    exit 9
  fi
  chmod 0644 "$tmp"
  mv -fT -- "$tmp" "$target"
done
echo "Synchro static assets published to $ROOT as $(id -un)."
echo "Next: merge deploy/nginx-synchro.conf into aaPanel, test nginx -t and open https://$DOMAIN/install/."

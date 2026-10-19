#!/usr/bin/env bash
# Run from a normal Ubuntu shell user before privileged deployment.
# This script never starts/stops services, modifies aaPanel, or uses sudo.
set -Eeuo pipefail
REPO="$(cd -- "$(dirname -- "$0")/.." && pwd)"

echo "[Synchro] Validate shell scripts..."
bash -n "$REPO/deploy/bootstrap-aapanel.sh"
bash -n "$REPO/deploy/publish-web.sh"
bash -n "$REPO/deploy/verify-source.sh"

if command -v node >/dev/null 2>&1; then
  echo "[Synchro] Validate browser JavaScript..."
  node --check "$REPO/web/app.js"
  node --check "$REPO/web/install/setup.js"
else
  echo "[Synchro] Node.js not found: browser syntax verification skipped."
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is required to test/build the Rust server." >&2
  exit 1
fi
cd "$REPO/server"
echo "[Synchro] Rust tests..."
cargo test
echo "[Synchro] Rust type checking..."
cargo check
echo "[Synchro] Rust linting..."
cargo clippy --all-targets
echo "[Synchro] Build optimized server..."
cargo build --release
echo "[Synchro] Build verified locally. Still test live HTTPS and file transfer separately."

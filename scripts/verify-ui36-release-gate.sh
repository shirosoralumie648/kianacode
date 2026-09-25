#!/usr/bin/env bash
set -euo pipefail

if [[ "${GITHUB_ACTIONS:-false}" != "true" ]]; then
  echo "remote_ci_required" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

node scripts/verify-desktop-assets.js
cargo fmt --all --check
cargo build --bin kiana --locked

if rg -n --hidden --glob '!target/**' --glob '!.git/**' --glob '!*.lock' \
  'BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY|ghp_|github_pat_|sk-live-|x-kiana-web-token=[^_]' \
  contrib/desktop dist scripts 2>/dev/null; then
  echo "release_secret_marker_found" >&2
  exit 1
fi

echo "ui36_release_gate_source_and_build_ok"

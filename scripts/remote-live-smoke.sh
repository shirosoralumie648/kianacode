#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

mode="${1:---required}"
if [[ "$mode" != "--required" && "$mode" != "--skip-if-unconfigured" ]]; then
  echo "usage: $0 [--required|--skip-if-unconfigured]" >&2
  exit 2
fi

out_dir="${KIANA_REMOTE_LIVE_SMOKE_DIR:-${KIANA_LIVE_SMOKE_DIR:-target/live-smoke}/remote}"
mkdir -p "$out_dir"

python_bin() {
  command -v python3 2>/dev/null || command -v python 2>/dev/null || {
    echo "remote live smoke requires python3 or python" >&2
    exit 1
  }
}

run_kiana() {
  if [[ -n "${KIANA_BIN:-}" ]]; then
    "$KIANA_BIN" "$@"
  else
    cargo run -p kiana-entrypoints --bin kiana -- "$@"
  fi
}

remote_configured() {
  [[ -n "${KIANA_REMOTE_ACCESS_TOKEN:-}" ]] ||
    [[ -n "${CLAUDE_ACCESS_TOKEN:-}" ]] ||
    [[ -n "${ANTHROPIC_AUTH_TOKEN:-}" ]] ||
    [[ -n "${KIANA_OAUTH_TOKENS_FILE:-}" && -f "${KIANA_OAUTH_TOKENS_FILE:-}" ]] ||
    [[ -n "${CLAUDE_CODE_OAUTH_TOKENS_FILE:-}" && -f "${CLAUDE_CODE_OAUTH_TOKENS_FILE:-}" ]]
}

if [[ "$mode" == "--skip-if-unconfigured" ]] && ! remote_configured; then
  echo "remote live smoke skipped: no remote bearer token or OAuth token file is configured"
  exit 0
fi

if [[ "$mode" == "--required" ]] && ! remote_configured; then
  echo "remote live smoke requires KIANA_REMOTE_ACCESS_TOKEN, CLAUDE_ACCESS_TOKEN, a remote ANTHROPIC_AUTH_TOKEN, or an OAuth token file" >&2
  exit 1
fi

remote_json="$out_dir/code-session-smoke.json"

raw_remote_json="${out_dir}/code-session-smoke.raw.json"
run_kiana remote-session code-session smoke --json --tag kiana-live-smoke >"$raw_remote_json"

REMOTE_LIVE_SMOKE_RAW_JSON="$raw_remote_json" \
REMOTE_LIVE_SMOKE_JSON="$remote_json" \
"$(python_bin)" - <<'PY'
import json
import os
import sys
from datetime import datetime, timezone

raw_path = os.environ["REMOTE_LIVE_SMOKE_RAW_JSON"]
path = os.environ["REMOTE_LIVE_SMOKE_JSON"]
with open(raw_path, "r", encoding="utf-8") as handle:
    report = json.load(handle)

checks = [
    report.get("status") == "ok",
    isinstance(report.get("session_id"), str) and report["session_id"].startswith("cse_"),
    isinstance(report.get("api_base_url"), str) and report["api_base_url"].startswith(("http://", "https://")),
    isinstance(report.get("sdk_url"), str) and report["sdk_url"].startswith(("http://", "https://")),
    isinstance(report.get("expires_in"), int) and report["expires_in"] > 0,
    isinstance(report.get("worker_epoch"), int) and report["worker_epoch"] >= 0,
]
if not all(checks):
    print("remote code-session live smoke JSON failed contract checks", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)

proof = {
    "schema": "kiana.remote-code-session-smoke.v1",
    "status": "ok",
    "checked_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "session_id": report["session_id"],
    "api_base_url": report["api_base_url"],
    "sdk_url": report["sdk_url"],
    "expires_in": report["expires_in"],
    "worker_epoch": report["worker_epoch"],
}
with open(path, "w", encoding="utf-8") as handle:
    json.dump(proof, handle, indent=2, sort_keys=True)
    handle.write("\n")

print(f"remote live smoke passed: session_id={proof['session_id']} proof={path}")
PY

#!/usr/bin/env bash
# OA-26 is intentionally CI-only. It exercises the on-disk gate remotely and emits a bounded
# evidence manifest; a local invocation refuses instead of pretending a local run is release proof.
set -Eeuo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ "${GITHUB_ACTIONS:-}" != "true" && "${CI:-}" != "true" ]]; then
  echo "oa26-durable-observability-gate: remote_ci_required" >&2
  exit 2
fi

evidence_dir="${OA26_EVIDENCE_DIR:-$ROOT/dist/oa26-durable}"
mkdir -p "$evidence_dir"

sha256sum \
  kiana-domain/src/journal.rs \
  kiana-domain/src/performance.rs \
  kiana-domain/src/parity.rs \
  kiana-core/src/receipts.rs \
  kiana-core/src/audit_projection.rs \
  kiana-core/src/audit_export.rs \
  kiana-core/src/parity.rs \
  kiana-eventlog/src/jsonl.rs \
  kiana-eventlog/src/journal_core.rs \
  kiana-core/tests/oa26_durable_gate.rs \
  kiana-ports/src/observability_queue.rs \
  scripts/oa26-durable-observability-gate.sh \
  >"$evidence_dir/source-sha256.txt"

cargo fmt --all --check
cargo check --workspace --tests --locked --offline
cargo test -p kiana-core --test oa26_durable_gate --locked --offline -- --test-threads=1
cargo test -p kiana-core --test oa24_entrypoint_parity --locked --offline -- --test-threads=1
cargo build --release --locked --offline -p kiana-entrypoints --bin kiana

sha256sum target/release/kiana >"$evidence_dir/release-binary-sha256.txt"
git diff --check

python3 - "$evidence_dir" <<'PY'
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

root = Path.cwd()
out = Path(sys.argv[1])

def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()

manifest = {
    "schema": "kiana.oa26-durable-evidence.v1",
    "source_snapshot": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
    "proof_level": "local_behavior",
    "journal_reopen": "passed_in_github_ci",
    "unknown_reconciliation": "passed_in_github_ci",
    "queue_fencing": "passed_in_github_ci",
    "source_sha256": sha(out / "source-sha256.txt"),
    "release_binary_sha256": sha(out / "release-binary-sha256.txt"),
    "environment": {
        "github_actions": os.environ.get("GITHUB_ACTIONS", ""),
        "runner_os": os.environ.get("RUNNER_OS", ""),
        "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
    },
    "limitations": [
        "single GitHub runner reopen is not physical power-loss evidence",
        "no external provider/Broker/telemetry backend was contacted",
        "no durable benchmark artifact or cross-process retention/reconcile store exists",
    ],
}
(out / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

if rg -n --pcre2 '(?i)(bearer\s+[A-Za-z0-9._-]{8,}|sk-[A-Za-z0-9]{8,}|raw-secret)' "$evidence_dir"; then
  echo "oa26-durable-observability-gate: secret_sentinel_detected" >&2
  exit 1
fi

echo "oa26-durable-observability-gate: passed; evidence=$evidence_dir"

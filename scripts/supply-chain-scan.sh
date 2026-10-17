#!/usr/bin/env bash
set -euo pipefail

# SC-28 is the read-only dependency/release input gate. It never publishes an
# artifact. Any missing lockfile, unresolved dependency graph, lock drift,
# unknown license, advisory finding or scanner failure keeps the report
# quarantined and returns non-zero, so a reviewer always receives a machine
# readable report instead of a bare non-zero exit.
cd "$(dirname "$0")/.."

out_dir="${1:-${SC28_OUT_DIR:-dist/sc28-supply-chain}}"
mkdir -p "$out_dir"

if command -v python3 >/dev/null 2>&1; then
  python_bin="python3"
elif command -v python >/dev/null 2>&1; then
  python_bin="python"
else
  echo "SC-28 requires python3 or python" >&2
  exit 1
fi

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

lockfile="Cargo.lock"
lock_present=true
lock_before="missing"
lock_after="missing"
metadata_file="$out_dir/cargo-metadata.json"
metadata_exit=127

if [[ -f "$lockfile" ]]; then
  lock_before="$(sha256_file "$lockfile")"
  printf '%s  %s\n' "$lock_before" "$lockfile" > "$out_dir/Cargo.lock.sha256"
  # The locked metadata command is the only dependency graph source for SC-28:
  # cargo metadata --locked --format-version 1 (optionally with --offline). A
  # non-zero exit is recorded as evidence instead of aborting the script, so an
  # unlocked or unresolvable dependency graph still produces a quarantine record.
  cargo_args=(metadata --locked --format-version 1)
  if [[ "${SC28_OFFLINE:-0}" == "1" ]]; then
    cargo_args+=(--offline)
  fi
  set +e
  cargo "${cargo_args[@]}" > "$metadata_file"
  metadata_exit=$?
  set -e
  lock_after="$(sha256_file "$lockfile")"
else
  lock_present=false
  : > "$metadata_file"
  echo "SC-28: Cargo.lock is absent; recording a quarantine report instead of exiting early" >&2
fi

lock_dirty=false
if ! git diff --quiet -- Cargo.lock; then
  lock_dirty=true
fi
if [[ -n "$(git status --porcelain -- Cargo.lock)" ]]; then
  lock_dirty=true
fi

tool_root="${SC28_TOOL_ROOT:-target/sc28-tools}"
if [[ -d "$tool_root/bin" ]]; then
  if [[ "$tool_root" = /* ]]; then
    export PATH="$tool_root/bin:$PATH"
  else
    export PATH="$PWD/$tool_root/bin:$PATH"
  fi
fi

advisory_db="${SC28_ADVISORY_DB:-target/sc28-advisory-db}"
mkdir -p "$advisory_db"
audit_json="$out_dir/cargo-audit.json"
audit_stderr="$out_dir/cargo-audit.stderr"
audit_exit=127
audit_tool=missing
if command -v cargo-audit >/dev/null 2>&1; then
  audit_tool="$(command -v cargo-audit)"
  set +e
  cargo audit --json --db "$advisory_db" > "$audit_json" 2> "$audit_stderr"
  audit_exit=$?
  set -e
else
  echo '{"vulnerabilities":{"list":[]}}' > "$audit_json"
  echo "cargo-audit is not installed" > "$audit_stderr"
fi

deny_log="$out_dir/cargo-deny.log"
deny_exit=127
deny_tool=missing
if command -v cargo-deny >/dev/null 2>&1; then
  deny_tool="$(command -v cargo-deny)"
  set +e
  cargo deny check advisories licenses bans sources > "$deny_log" 2>&1
  deny_exit=$?
  set -e
else
  echo "cargo-deny is not installed" > "$deny_log"
fi

source_revision="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
max_high="${SC28_MAX_HIGH_ADVISORIES:-0}"
max_critical="${SC28_MAX_CRITICAL_ADVISORIES:-0}"
expected_digest="${SC28_EXPECTED_LOCKFILE_SHA256:-}"

# Record which scanner builds produced the evidence. The binaries are installed by
# the CI lane with `cargo install --locked`; their resolved versions are evidence,
# not a pin, so a future drift is visible in the report instead of silent.
audit_version="unknown"
if [[ "$audit_tool" != "missing" ]]; then
  audit_version="$("$audit_tool" --version 2>/dev/null | head -n1 || true)"
  audit_version="${audit_version:-unknown}"
fi
deny_version="unknown"
if [[ "$deny_tool" != "missing" ]]; then
  deny_version="$("$deny_tool" --version 2>/dev/null | head -n1 || true)"
  deny_version="${deny_version:-unknown}"
fi

set +e
"$python_bin" scripts/supply-chain-scan.py \
  --metadata "$metadata_file" \
  --lockfile "$lockfile" \
  --out-dir "$out_dir" \
  --source-revision "$source_revision" \
  --lock-before "$lock_before" \
  --lock-after "$lock_after" \
  --lock-present "$lock_present" \
  --metadata-exit "$metadata_exit" \
  --lock-dirty "$lock_dirty" \
  --expected-lock-digest "$expected_digest" \
  --audit-json "$audit_json" \
  --audit-exit "$audit_exit" \
  --deny-exit "$deny_exit" \
  --audit-tool "$audit_tool" \
  --deny-tool "$deny_tool" \
  --audit-version "$audit_version" \
  --deny-version "$deny_version" \
  --max-high "$max_high" \
  --max-critical "$max_critical"
scan_exit=$?
set -e

if [[ "$scan_exit" -ne 0 ]]; then
  echo "SC-28 supply-chain scan quarantined the source snapshot; see $out_dir/quarantine.json" >&2
  exit "$scan_exit"
fi
echo "SC-28 supply-chain scan passed: $out_dir/supply-chain-report.json"

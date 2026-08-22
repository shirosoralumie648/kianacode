#!/usr/bin/env bash
# v0.3 Phase 4 WB-01: cassette golden path.
# Proof ceiling: local_behavior. Not a live-provider gate.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CASSETTE="${KIANA_HARNESS_CASSETTE:-$ROOT/scripts/fixtures/harness-golden-apply-patch.json}"
WORKDIR=""

die() {
  echo "harness-golden-smoke: $*" >&2
  exit 1
}

cleanup() {
  if [[ -n "$WORKDIR" && -d "$WORKDIR" ]]; then
    rm -rf "$WORKDIR"
  fi
}
trap cleanup EXIT

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing $1"
}

json_field() {
  local file="$1"
  local path="$2"
  python3 - "$file" "$path" <<'PY'
import json, sys
from pathlib import Path
path = Path(sys.argv[1])
raw = path.read_text(encoding="utf-8")
start = raw.find("{")
if start < 0:
    raise SystemExit(f"no json object in {path}")
doc = json.loads(raw[start:])
cur = doc
for part in sys.argv[2].split("."):
    if isinstance(cur, list):
        cur = cur[int(part)]
    else:
        cur = cur[part]
if isinstance(cur, bool):
    print("true" if cur else "false")
elif cur is None:
    print("null")
else:
    print(cur)
PY
}

json_file_field() {
  python3 - "$1" "$2" <<'PY'
import json, sys
from pathlib import Path
doc = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
cur = doc
for part in sys.argv[2].split("."):
    cur = cur[part]
if isinstance(cur, bool):
    print("true" if cur else "false")
elif cur is None:
    print("null")
else:
    print(cur)
PY
}

resolve_bin() {
  local exe=""
  case "$(uname -s)" in
    MSYS*|MINGW*|CYGWIN*) exe=".exe" ;;
  esac
  if [[ -n "${KIANA_BIN:-}" ]]; then
    printf '%s\n' "$KIANA_BIN"
    return 0
  fi
  if [[ -x "$ROOT/target/debug/kiana${exe}" ]]; then
    printf '%s\n' "$ROOT/target/debug/kiana${exe}"
    return 0
  fi
  if [[ -x "$ROOT/target/release/kiana${exe}" ]]; then
    printf '%s\n' "$ROOT/target/release/kiana${exe}"
    return 0
  fi
  die "need KIANA_BIN or target/debug/kiana; do not cargo-build inside this smoke"
}

assert_eq() {
  local got="$1"
  local want="$2"
  local label="$3"
  [[ "$got" == "$want" ]] || die "$label: got '$got' want '$want'"
}

assert_file_eq() {
  local file="$1"
  local want="$2"
  local label="$3"
  python3 - "$file" "$want" "$label" <<'PY' || exit 1
import sys
from pathlib import Path
path, want, label = sys.argv[1], sys.argv[2], sys.argv[3]
got = Path(path).read_text(encoding="utf-8")
if got != want:
    raise SystemExit(f"harness-golden-smoke: {label}: got {got!r} want {want!r}")
PY
}

assert_contains() {
  local haystack="$1"
  local needle="$2"
  local label="$3"
  [[ "$haystack" == *"$needle"* ]] || die "$label: missing '$needle'"
}

assert_not_contains() {
  local haystack="$1"
  local needle="$2"
  local label="$3"
  [[ "$haystack" != *"$needle"* ]] || die "$label: unexpectedly contains '$needle'"
}

make_fixture() {
  local dest="$1"
  mkdir -p "$dest/.git"
}

run_kiana() {
  local cwd="$1"
  local home="$2"
  shift 2
  (
    cd "$cwd"
    env \
      -u KIANA_HARNESS_SCRIPT \
      -u KIANA_PROVIDER \
      -u KIANA_FAKE_PROVIDER_SCRIPT \
      -u ANTHROPIC_API_KEY \
      -u KIANA_OPENAI_API_KEY \
      -u OPENAI_API_KEY \
      HOME="$home" \
      KIANA_HOME="$home" \
      "$BIN" "$@"
  )
}

run_kiana_cassette() {
  local cwd="$1"
  local home="$2"
  shift 2
  (
    cd "$cwd"
    env \
      -u KIANA_PROVIDER \
      -u KIANA_FAKE_PROVIDER_SCRIPT \
      -u ANTHROPIC_API_KEY \
      -u KIANA_OPENAI_API_KEY \
      -u OPENAI_API_KEY \
      HOME="$home" \
      KIANA_HOME="$home" \
      KIANA_HARNESS_SCRIPT="$CASSETTE" \
      "$BIN" "$@"
  )
}

need_cmd python3
[[ -f "$CASSETTE" ]] || die "cassette missing: $CASSETTE"
BIN="$(resolve_bin)"
[[ -x "$BIN" ]] || die "not executable: $BIN"

WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/kiana-golden.XXXXXX")"
SYMPOSIUM="$WORKDIR/symposium"
BUILDER="$WORKDIR/builder"
HOME_A="$WORKDIR/home-a"
HOME_B="$WORKDIR/home-b"
make_fixture "$SYMPOSIUM"
make_fixture "$BUILDER"
mkdir -p "$HOME_A" "$HOME_B"

echo "== trust symposium fixture"
trust_out="$(run_kiana "$SYMPOSIUM" "$HOME_A" trust . 2>&1)" || die "trust failed: $trust_out"

echo "== anti-meeting symposium (no model)"
sym_json="$WORKDIR/symposium.json"
set +e
run_kiana "$SYMPOSIUM" "$HOME_A" \
  run --symposium --anti-meeting --sandbox workspace-write --json -- \
  "create GOLDEN_PATH.txt containing hello" >"$sym_json" 2>"$WORKDIR/symposium.err"
sym_status=$?
set -e
if [[ "$sym_status" -ne 0 ]]; then
  die "anti-meeting failed ($sym_status): $(cat "$WORKDIR/symposium.err") $(cat "$sym_json")"
fi

status="$(json_field "$sym_json" status)"
assert_eq "$status" "completed" "symposium status"
schema="$(json_field "$sym_json" output.schema)"
assert_eq "$schema" "kiana.symposium-result.v1" "symposium schema"
present="$(json_field "$sym_json" output.builder_present)"
assert_eq "$present" "false" "builder_present"
skipped="$(json_field "$sym_json" output.skipped_meeting)"
assert_eq "$skipped" "true" "skipped_meeting"
[[ -f "$SYMPOSIUM/plan/DECISION.json" ]] || die "missing plan/DECISION.json"
[[ -f "$SYMPOSIUM/packet/TASK.json" ]] || die "missing packet/TASK.json"
[[ ! -e "$SYMPOSIUM/GOLDEN_PATH.txt" ]] || die "anti-meeting must not write GOLDEN_PATH.txt"

decision_schema="$(json_file_field "$SYMPOSIUM/plan/DECISION.json" schema)"
assert_eq "$decision_schema" "kiana.decision-record.v1" "decision schema"
packet_schema="$(json_file_field "$SYMPOSIUM/packet/TASK.json" schema)"
assert_eq "$packet_schema" "kiana.work-packet.v1" "packet schema"
packet_role="$(json_file_field "$SYMPOSIUM/packet/TASK.json" assignee_role)"
assert_eq "$packet_role" "builder" "packet assignee"
packet_id="$(json_file_field "$SYMPOSIUM/packet/TASK.json" id)"
[[ -n "$packet_id" && "$packet_id" != "null" ]] || die "packet id missing"
packet_goal="$(json_file_field "$SYMPOSIUM/packet/TASK.json" goal)"
assert_contains "$packet_goal" "GOLDEN_PATH.txt" "packet goal"
decision_skip="$(json_file_field "$SYMPOSIUM/plan/DECISION.json" skipped_meeting)"
assert_eq "$decision_skip" "true" "decision skipped_meeting"
sym_text="$(cat "$sym_json")$(cat "$WORKDIR/symposium.err")"
assert_not_contains "$sym_text" "live provider" "anti-meeting live claim"
assert_not_contains "$sym_text" "provider-live" "anti-meeting live claim"

echo "== packet spawn with cassette"
pkt_json="$WORKDIR/packet.json"
set +e
run_kiana_cassette "$SYMPOSIUM" "$HOME_A" \
  run --packet packet/TASK.json --sandbox workspace-write --json \
  >"$pkt_json" 2>"$WORKDIR/packet.err"
pkt_status=$?
set -e
if [[ "$pkt_status" -ne 0 ]]; then
  die "packet spawn failed ($pkt_status): $(cat "$WORKDIR/packet.err") $(cat "$pkt_json")"
fi

assert_eq "$(json_field "$pkt_json" status)" "completed" "packet status"
assert_eq "$(json_field "$pkt_json" output.role_id)" "builder" "packet role_id"
assert_eq "$(json_field "$pkt_json" output.department_id)" "executing" "packet department"
assert_eq "$(json_field "$pkt_json" output.work_packet_id)" "$packet_id" "packet work_packet_id"
assert_eq "$(json_field "$pkt_json" output.files_changed.0)" "GOLDEN_PATH.txt" "packet files_changed"
[[ -f "$SYMPOSIUM/GOLDEN_PATH.txt" ]] || die "GOLDEN_PATH.txt missing after packet spawn"
assert_file_eq "$SYMPOSIUM/GOLDEN_PATH.txt" $'hello\n' "GOLDEN_PATH.txt contents"
pkt_text="$(cat "$pkt_json")$(cat "$WORKDIR/packet.err")"
assert_contains "$pkt_text" "kiana-harness" "packet harness"
assert_not_contains "$pkt_text" "live provider" "packet live claim"

echo "== v0.2 builder direct-write regression"
trust_b="$(run_kiana "$BUILDER" "$HOME_B" trust . 2>&1)" || die "builder trust failed: $trust_b"
bld_json="$WORKDIR/builder.json"
set +e
run_kiana_cassette "$BUILDER" "$HOME_B" \
  run --sandbox workspace-write --json -- \
  "create GOLDEN_PATH.txt containing hello" >"$bld_json" 2>"$WORKDIR/builder.err"
bld_status=$?
set -e
if [[ "$bld_status" -ne 0 ]]; then
  die "builder run failed ($bld_status): $(cat "$WORKDIR/builder.err") $(cat "$bld_json")"
fi
assert_eq "$(json_field "$bld_json" status)" "completed" "builder status"
assert_eq "$(json_field "$bld_json" output.role_id)" "builder" "builder role_id"
assert_eq "$(json_field "$bld_json" output.department_id)" "executing" "builder department"
assert_eq "$(json_field "$bld_json" output.files_changed.0)" "GOLDEN_PATH.txt" "builder files_changed"
[[ -f "$BUILDER/GOLDEN_PATH.txt" ]] || die "builder GOLDEN_PATH.txt missing"
assert_file_eq "$BUILDER/GOLDEN_PATH.txt" $'hello\n' "builder GOLDEN_PATH.txt contents"

echo "harness golden smoke passed (local_behavior)"
echo "proof: cassette=$CASSETTE bin=$BIN"
echo "not claimed: live provider, physical install, TUI harness"

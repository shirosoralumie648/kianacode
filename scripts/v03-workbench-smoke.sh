#!/usr/bin/env bash
# v0.3 Phase 4 workbench wrapper: golden eval + TUI park + temp install demo.
# Does not run scripts/release-smoke.sh or cargo fmt --all.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_TMP=""

die() {
  echo "v03-workbench-smoke: $*" >&2
  exit 1
}

cleanup() {
  if [[ -n "$INSTALL_TMP" && -d "$INSTALL_TMP" ]]; then
    rm -rf "$INSTALL_TMP"
  fi
}
trap cleanup EXIT

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
  die "need KIANA_BIN or target/debug/kiana"
}

BIN="$(resolve_bin)"
[[ -x "$BIN" ]] || die "not executable: $BIN"
export KIANA_BIN="$BIN"

echo "== WB-01 harness golden"
bash "$ROOT/scripts/harness-golden-smoke.sh"

echo "== WB-03 TUI remains parked"
help_out="$("$BIN" --help 2>&1)" || die "kiana --help failed"
[[ "$help_out" == *"kiana tui"* ]] || die "help missing tui"
[[ "$help_out" == *"Parked in v0.2"* ]] || die "help missing TUI park"
[[ "$help_out" == *"not DaemonHost"* ]] || die "help missing not DaemonHost"

set +e
tui_out="$("$BIN" tui </dev/null 2>&1)"
tui_status=$?
set -e
[[ "$tui_status" -ne 0 ]] || die "kiana tui without TTY must fail"
[[ "$tui_out" != *"kiana-harness"* ]] || die "parked TUI printed kiana-harness"
[[ "$tui_out" != *"harness: kiana-harness"* ]] || die "parked TUI claimed harness"

echo "== WB-02 install.sh demo (temp INSTALL_DIR)"
INSTALL_TMP="$(mktemp -d "${TMPDIR:-/tmp}/kiana-phase4-bin.XXXXXX")"
INSTALL_DIR="$INSTALL_TMP" \
  KIANA_SKIP_PATH_SETUP=1 \
  KIANA_SKIP_BUILD=1 \
  KIANA_BIN="$BIN" \
  bash "$ROOT/install.sh"

installed="$INSTALL_TMP/kiana"
[[ -x "$installed" ]] || die "install.sh did not write $installed"
"$installed" --version >/dev/null || die "installed binary --version failed"
run_help="$("$installed" run --help 2>&1 || true)"
[[ "$run_help" == *"--symposium"* ]] || die "installed run --help missing --symposium"
[[ "$run_help" == *"--packet"* ]] || die "installed run --help missing --packet"

echo "v0.3 workbench smoke passed (local_behavior)"
echo "not claimed: live provider, ~/.local/bin production install, TUI on DaemonHost"

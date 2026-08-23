#!/usr/bin/env bash
# v1.0.3 folder workbench: cwd / --workdir / GUI picker on DaemonHost.
# Proof ceiling: local_behavior. Not live provider, not kiana tui, not dsh web clone.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CASSETTE="${KIANA_HARNESS_CASSETTE:-$ROOT/scripts/fixtures/harness-golden-apply-patch.json}"
WORKDIR=""

die() {
  echo "v10-workbench-smoke: $*" >&2
  exit 1
}

cleanup() {
  if [[ -n "$WORKDIR" && -d "$WORKDIR" ]]; then
    rm -rf "$WORKDIR"
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
  die "missing kiana binary; set KIANA_BIN or build target/debug/kiana"
}

[[ -f "$CASSETTE" ]] || die "cassette missing: $CASSETTE"
[[ -f "$ROOT/contrib/kiana.desktop" ]] || die "missing contrib/kiana.desktop"
grep -q -- "--workdir %f" "$ROOT/contrib/kiana.desktop" || die "desktop file must target --workdir"
BIN="$(resolve_bin)"
[[ -x "$BIN" ]] || die "not executable: $BIN"

WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/kiana-workbench.XXXXXX")"
FIXTURE="$WORKDIR/project"
HOME_DIR="$WORKDIR/home"
mkdir -p "$FIXTURE/.git" "$HOME_DIR"

help_out="$("$BIN" workbench --help 2>&1 || true)"
echo "$help_out" | grep -q -- "--workdir" || die "workbench help missing --workdir"
echo "$help_out" | grep -q "DaemonHost" || die "workbench help missing DaemonHost"

top_help="$("$BIN" --help 2>&1 || true)"
echo "$top_help" | grep -q -- "--pick-folder" || die "top help missing --pick-folder"

untrusted="$(
  cd "$FIXTURE"
  env -u DISPLAY -u WAYLAND_DISPLAY -u KIANA_PROVIDER -u ANTHROPIC_API_KEY \
    HOME="$HOME_DIR" KIANA_HOME="$HOME_DIR" KIANA_HARNESS_SCRIPT="$CASSETTE" \
    "$BIN" workbench --json --sandbox workspace-write -- "create GOLDEN_PATH.txt" 2>&1 || true
)"
echo "$untrusted" | grep -q "workspace_write_requires_trusted_non_safe_profile" || die "untrusted workbench did not fail closed: $untrusted"
[[ ! -f "$FIXTURE/GOLDEN_PATH.txt" ]] || die "untrusted write leaked"

(
  cd "$FIXTURE"
  env -u DISPLAY -u WAYLAND_DISPLAY HOME="$HOME_DIR" KIANA_HOME="$HOME_DIR" \
    "$BIN" trust .
) >/dev/null

trusted="$(
  cd "$FIXTURE"
  env -u DISPLAY -u WAYLAND_DISPLAY -u KIANA_PROVIDER -u ANTHROPIC_API_KEY \
    HOME="$HOME_DIR" KIANA_HOME="$HOME_DIR" KIANA_HARNESS_SCRIPT="$CASSETTE" \
    "$BIN" --workdir "$FIXTURE" --json --sandbox workspace-write -- \
      "create a file named GOLDEN_PATH.txt containing hello"
)"
echo "$trusted" | grep -q '"status": "completed"' || echo "$trusted" | grep -q '"status":"completed"' || die "trusted workbench did not complete: $trusted"
[[ -f "$FIXTURE/GOLDEN_PATH.txt" ]] || die "trusted workbench did not write GOLDEN_PATH.txt"
grep -qx 'hello' "$FIXTURE/GOLDEN_PATH.txt" || die "GOLDEN_PATH.txt contents wrong"

picker_out="$(
  env -u DISPLAY -u WAYLAND_DISPLAY -u KIANA_PROVIDER -u ANTHROPIC_API_KEY \
    HOME="$HOME_DIR" KIANA_HOME="$HOME_DIR" \
    "$BIN" --pick-folder 2>&1 || true
)"
echo "$picker_out" | grep -q "pick_folder_unavailable" || die "picker without display should fail closed: $picker_out"

echo "v10-workbench-smoke: ok bin=$BIN"

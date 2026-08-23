#!/usr/bin/env bash
# v1.0.1 personal lifecycle: temp INSTALL_DIR install/upgrade/rollback/recover/uninstall.
# Does not run scripts/release-smoke.sh, package-lifecycle-smoke.sh, or cargo fmt --all.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_TMP=""
ROLLBACK_TMP=""

die() {
  echo "v10-personal-lifecycle-smoke: $*" >&2
  exit 1
}

cleanup() {
  if [[ -n "$INSTALL_TMP" && -d "$INSTALL_TMP" ]]; then
    rm -rf "$INSTALL_TMP"
  fi
  if [[ -n "$ROLLBACK_TMP" && -d "$ROLLBACK_TMP" ]]; then
    rm -rf "$ROLLBACK_TMP"
  fi
}
trap cleanup EXIT

file_hash() {
  local file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    die "need sha256sum or shasum"
  fi
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
  die "need KIANA_BIN or target/debug/kiana (do not use stale target/release/kiana)"
}

require_user_md() {
  local manual="$ROOT/USER.md"
  [[ -f "$manual" ]] || die "USER.md missing"
  local contents
  contents="$(cat "$manual")"
  [[ "$contents" == *"kiana trust ."* ]] || die "USER.md missing kiana trust ."
  [[ "$contents" == *"kiana run --sandbox workspace-write"* ]] || die "USER.md missing run"
  [[ "$contents" == *"--symposium --anti-meeting"* ]] || die "USER.md missing symposium"
  [[ "$contents" == *"kiana run --packet"* ]] || die "USER.md missing packet"
  [[ "$contents" == *"kiana run --review"* ]] || die "USER.md missing review"
  [[ "$contents" == *"install.sh"* ]] || die "USER.md missing install.sh"
  [[ "$contents" == *"install.sh --uninstall"* ]] || die "USER.md missing uninstall"
  [[ "$contents" == *"Parked"* || "$contents" == *"park"* ]] || die "USER.md missing TUI park"
  [[ "$contents" == *"mcp_transport_unsupported"* ]] || die "USER.md missing HTTP MCP fail-closed"
  [[ "$contents" == *"live provider"* ]] || die "USER.md missing live-provider honesty"
}

run_install() {
  INSTALL_DIR="$INSTALL_TMP" \
    KIANA_SKIP_PATH_SETUP=1 \
    KIANA_SKIP_BUILD=1 \
    KIANA_BIN="$BIN" \
    bash "$ROOT/install.sh"
}

echo "== REL-02 USER.md names real commands"
require_user_md

BIN="$(resolve_bin)"
[[ -x "$BIN" ]] || die "not executable: $BIN"
[[ "$BIN" != "$ROOT/target/release/kiana" && "$BIN" != "$ROOT/target/release/kiana.exe" ]] \
  || die "refusing stale target/release/kiana; set KIANA_BIN to target/debug/kiana"
export KIANA_BIN="$BIN"

bash -n "$ROOT/install.sh" || die "install.sh syntax"
bash -n "$ROOT/scripts/v10-personal-lifecycle-smoke.sh" || die "self syntax"

echo "== REL-01 temp INSTALL_DIR lifecycle"
INSTALL_TMP="$(mktemp -d "${TMPDIR:-/tmp}/kiana-v10-bin.XXXXXX")"
ROLLBACK_TMP="$(mktemp -d "${TMPDIR:-/tmp}/kiana-v10-rollback.XXXXXX")"
[[ "$INSTALL_TMP" != "$HOME/.local/bin" ]] || die "refusing ~/.local/bin"
[[ "$INSTALL_TMP" != "${HOME}/.local/bin" ]] || die "refusing ~/.local/bin"

run_install
installed="$INSTALL_TMP/kiana"
[[ -x "$installed" ]] || die "install.sh did not write $installed"
"$installed" --version >/dev/null || die "installed binary --version failed"
install_hash="$(file_hash "$installed")"
cp "$installed" "$ROLLBACK_TMP/kiana"
chmod +x "$ROLLBACK_TMP/kiana"

echo "== upgrade (reinstall same binary)"
run_install
upgrade_hash="$(file_hash "$installed")"
[[ "$upgrade_hash" == "$install_hash" ]] || die "same-binary upgrade changed checksum"

echo "== rollback (restore backup)"
cp "$ROLLBACK_TMP/kiana" "$installed"
chmod +x "$installed"
rollback_hash="$(file_hash "$installed")"
[[ "$rollback_hash" == "$install_hash" ]] || die "rollback did not match first-install checksum"
"$installed" --version >/dev/null || die "rollback binary --version failed"

echo "== recover (delete then reinstall)"
rm -f "$installed"
[[ ! -e "$installed" ]] || die "failed to delete installed binary"
run_install
[[ -x "$installed" ]] || die "recover did not restore $installed"
"$installed" --version >/dev/null || die "recovered binary --version failed"

echo "== uninstall (idempotent)"
INSTALL_DIR="$INSTALL_TMP" bash "$ROOT/install.sh" --uninstall >/dev/null
[[ ! -e "$installed" ]] || die "uninstall left binary behind: $installed"
INSTALL_DIR="$INSTALL_TMP" bash "$ROOT/install.sh" --uninstall >/dev/null
[[ ! -e "$installed" ]] || die "second uninstall recreated $installed"

echo "v1.0.1 personal lifecycle smoke passed (local_behavior)"
echo "not claimed: ~/.local/bin production install, packaged tarball, SBOM/signing, live provider, REL-03 recode"

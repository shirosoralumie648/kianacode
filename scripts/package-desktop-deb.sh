#!/usr/bin/env bash
# Build an installable .deb for the Electron shell + kiana CLI.
# Same DaemonHost. Loopback only. Not a signed Debian archive.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DESKTOP_DIR="$REPO_DIR/contrib/desktop"
DIST_DIR="${DIST_DIR:-$REPO_DIR/dist}"
VERSION="${VERSION:-$(tr -d '\r\n' < "$REPO_DIR/VERSION")}"
LAYOUT_ONLY=0
SKIP_ELECTRON=0
KEEP_ROOT=0

usage() {
  cat <<EOF
Usage: bash scripts/package-desktop-deb.sh [--layout-only] [--skip-electron] [--keep-root]

Writes dist/kiana-desktop_<version>_<arch>.deb
Needs: node, npm, dpkg-deb, and a kiana binary (KIANA_BIN or target/debug|release).
First run downloads Electron Chromium (~100MB). Default mirror:
  ELECTRON_MIRROR=https://npmmirror.com/mirrors/electron/
--layout-only   stage the Debian tree, do not run dpkg-deb
--skip-electron skip Electron runtime (tree is not a GUI package)
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --help|-h) usage; exit 0 ;;
    --layout-only) LAYOUT_ONLY=1 ;;
    --skip-electron) SKIP_ELECTRON=1 ;;
    --keep-root) KEEP_ROOT=1 ;;
    *) echo "unknown option: $1" >&2; usage; exit 1 ;;
  esac
  shift
done

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "need $1" >&2
    exit 1
  }
}

need node
need python3

machine="$(uname -m)"
arch="$(node -e 'const {debianArch}=require(process.argv[1]); process.stdout.write(debianArch(process.argv[2]))' "$DESKTOP_DIR/lib/deb-layout.js" "$machine")"
artifact="$(node -e 'const {artifactName}=require(process.argv[1]); process.stdout.write(artifactName(process.argv[2], process.argv[3]))' "$DESKTOP_DIR/lib/deb-layout.js" "$VERSION" "$arch")"

resolve_kiana() {
  local exe=""
  case "$(uname -s)" in
    MSYS*|MINGW*|CYGWIN*) exe=".exe" ;;
  esac
  if [[ -n "${KIANA_BIN:-}" && -x "${KIANA_BIN}" ]]; then
    printf '%s\n' "$KIANA_BIN"
    return 0
  fi
  for candidate in \
    "$REPO_DIR/target/debug/kiana${exe}" \
    "$REPO_DIR/target/release/kiana${exe}" \
    "${HOME}/.local/bin/kiana${exe}"
  do
    if [[ -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  echo "kiana binary not found. Set KIANA_BIN or build kiana-entrypoints." >&2
  exit 1
}

KIANA_BIN="$(resolve_kiana)"
if [[ "$LAYOUT_ONLY" != "1" ]]; then
  if ! "$KIANA_BIN" web --help </dev/null 2>/dev/null | grep -q "kiana web"; then
    echo "Refusing to package $KIANA_BIN: it has no \"kiana web\" command." >&2
    echo "The August-11 release binary will open TUI and fail under Electron." >&2
    echo "Build current tree: cargo build -p kiana-entrypoints --bin kiana" >&2
    echo "Then: KIANA_BIN=target/debug/kiana bash scripts/package-desktop-deb.sh" >&2
    exit 1
  fi
fi
ELECTRON_DIST="$DESKTOP_DIR/node_modules/electron/dist"

ensure_electron() {
  if [[ -x "$ELECTRON_DIST/electron" ]]; then
    echo "Using cached Electron runtime: $ELECTRON_DIST/electron"
    return 0
  fi
  need npm
  if [[ -z "${ELECTRON_MIRROR:-}" ]]; then
    export ELECTRON_MIRROR="https://npmmirror.com/mirrors/electron/"
  fi
  echo "Downloading Electron Chromium runtime (this is the slow step, ~100MB)."
  echo "Mirror: $ELECTRON_MIRROR"
  echo "Override with ELECTRON_MIRROR=... or reuse node_modules/electron/dist."
  (cd "$DESKTOP_DIR" && npm install --omit=dev --foreground-scripts --no-audit --no-fund)
  if [[ ! -x "$ELECTRON_DIST/electron" ]]; then
    echo "Electron runtime missing at $ELECTRON_DIST/electron" >&2
    echo "GitHub downloads often hang. Try:" >&2
    echo "  ELECTRON_MIRROR=https://npmmirror.com/mirrors/electron/ bash scripts/package-desktop-deb.sh" >&2
    exit 1
  fi
}

if [[ "$SKIP_ELECTRON" != "1" ]]; then
  ensure_electron
fi

# dpkg-deb requires DEBIAN/ mode 0755. The repo may live on NTFS (mode 777),
# so stage on a Unix filesystem unless the caller overrides KIANA_DEB_ROOT.
ROOT="${KIANA_DEB_ROOT:-/tmp/kiana-desktop-deb-root}"
rm -rf "$ROOT"
APP="$ROOT/usr/lib/kiana-desktop"
BIN="$ROOT/usr/bin"
APPS="$ROOT/usr/share/applications"
ICON="$ROOT/usr/share/icons/hicolor/64x64/apps"
DOC="$ROOT/usr/share/doc/kiana-desktop"
DEBIAN="$ROOT/DEBIAN"

mkdir -p "$APP/resources/app/lib" "$BIN" "$APPS" "$ICON" "$DOC" "$DEBIAN"
chmod 0755 "$DEBIAN"

cp "$DESKTOP_DIR/package.json" "$APP/resources/app/"
cp "$DESKTOP_DIR/main.js" "$APP/resources/app/"
cp "$DESKTOP_DIR/preload.js" "$APP/resources/app/"
cp "$DESKTOP_DIR/welcome.html" "$APP/resources/app/"
cp "$DESKTOP_DIR/icon.png" "$APP/resources/app/"
cp "$DESKTOP_DIR/icon.png" "$ICON/kiana.png"
cp -a "$DESKTOP_DIR/lib/." "$APP/resources/app/lib/"
cp "$KIANA_BIN" "$APP/resources/kiana"
chmod 755 "$APP/resources/kiana"
cp "$APP/resources/kiana" "$BIN/kiana"
chmod 755 "$BIN/kiana"

node -e 'const {desktopEntry,launcherScript,postinst,copyright}=require(process.argv[1]);
const fs=require("fs");
fs.writeFileSync(process.argv[2], desktopEntry());
fs.writeFileSync(process.argv[3], launcherScript());
fs.writeFileSync(process.argv[4], postinst());
fs.writeFileSync(process.argv[5], copyright());
' "$DESKTOP_DIR/lib/deb-layout.js" \
  "$APPS/kiana-desktop.desktop" \
  "$BIN/kiana-desktop" \
  "$DEBIAN/postinst" \
  "$DOC/copyright"
chmod 755 "$BIN/kiana-desktop" "$DEBIAN/postinst"

if [[ "$SKIP_ELECTRON" != "1" ]]; then
  python3 - "$ELECTRON_DIST" "$APP" <<'PY'
from pathlib import Path
import shutil
import sys
src, dest = Path(sys.argv[1]), Path(sys.argv[2])
skip = {"resources"}
for child in src.iterdir():
    if child.name in skip:
        continue
    target = dest / ( "kiana-desktop" if child.name == "electron" else child.name )
    if child.is_dir():
        shutil.copytree(child, target, dirs_exist_ok=True)
    else:
        shutil.copy2(child, target)
resources = src / "resources"
if resources.is_dir():
    for child in resources.iterdir():
        if child.name in {"default_app.asar", "default_app.asar.unpacked"}:
            continue
        target = dest / "resources" / child.name
        if child.is_dir():
            shutil.copytree(child, target, dirs_exist_ok=True)
        else:
            shutil.copy2(child, target)
PY
  chmod 755 "$APP/kiana-desktop"
  if [[ -f "$APP/chrome-sandbox" ]]; then
    chmod 4755 "$APP/chrome-sandbox" || true
  fi
fi

size_kb="$(python3 - "$ROOT" <<'PY'
from pathlib import Path
import sys
root = Path(sys.argv[1])
total = 0
for path in root.rglob("*"):
    if path.is_file() and "DEBIAN" not in path.parts:
        total += path.stat().st_size
print((total + 1023) // 1024)
PY
)"

node -e 'const {control}=require(process.argv[1]);
const fs=require("fs");
fs.writeFileSync(process.argv[2], control({version:process.argv[3], arch:process.argv[4], installedSizeKb:Number(process.argv[5])}));
' "$DESKTOP_DIR/lib/deb-layout.js" "$DEBIAN/control" "$VERSION" "$arch" "$size_kb"
chmod 0644 "$DEBIAN/control" "$APPS/kiana-desktop.desktop" "$DOC/copyright"
chmod 0755 "$DEBIAN"

if [[ "$LAYOUT_ONLY" == "1" ]]; then
  echo "Staged Debian tree: $ROOT"
  echo "Artifact name: $artifact"
  exit 0
fi

need dpkg-deb
mkdir -p "$DIST_DIR"
out="$DIST_DIR/$artifact"
dpkg-deb --root-owner-group --build "$ROOT" "$out"
if [[ "$KEEP_ROOT" != "1" ]]; then
  rm -rf "$ROOT"
fi
echo "Wrote $out"
echo "Install with: sudo dpkg -i $out"
echo "Not a signed Debian archive. Loopback only. Close window can keep tray."

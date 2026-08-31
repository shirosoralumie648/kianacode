#!/usr/bin/env bash
# Install the Electron shell around `kiana web`.
# This is a local desktop wrapper, not a signed store package and not a second harness.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
INSTALL_DIR="${INSTALL_DIR:-${HOME}/.local/bin}"
APP_HOME="${KIANA_DESKTOP_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/kiana/desktop}"
DESKTOP_DIR="${KIANA_DESKTOP_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/applications}"
ICON_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/64x64/apps"
EXE_EXT=""
case "$(uname -s)" in
  MSYS*|MINGW*|CYGWIN*) EXE_EXT=".exe" ;;
esac

if [[ "${1:-}" == "--uninstall" || "${1:-}" == "uninstall" ]]; then
  rm -f "$INSTALL_DIR/kiana-desktop"
  rm -f "$DESKTOP_DIR/kiana-desktop.desktop"
  rm -f "$ICON_DIR/kiana.png"
  rm -rf "$APP_HOME"
  echo "Uninstalled kiana-desktop"
  exit 0
fi

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "need $1" >&2
    exit 1
  }
}

need npm
need python3

KIANA_BIN="${KIANA_BIN:-}"
if [[ -z "$KIANA_BIN" ]]; then
  if [[ -x "$INSTALL_DIR/kiana${EXE_EXT}" ]]; then
    KIANA_BIN="$INSTALL_DIR/kiana${EXE_EXT}"
  elif [[ -x "$REPO_DIR/target/debug/kiana${EXE_EXT}" ]]; then
    KIANA_BIN="$REPO_DIR/target/debug/kiana${EXE_EXT}"
  elif [[ -x "$REPO_DIR/target/release/kiana${EXE_EXT}" ]]; then
    KIANA_BIN="$REPO_DIR/target/release/kiana${EXE_EXT}"
  elif command -v kiana >/dev/null 2>&1; then
    KIANA_BIN="$(command -v kiana)"
  else
    echo "kiana binary not found. Run install.sh or set KIANA_BIN." >&2
    exit 1
  fi
fi

mkdir -p "$APP_HOME" "$INSTALL_DIR" "$DESKTOP_DIR" "$ICON_DIR" "$APP_HOME/resources"
python3 - "$REPO_DIR/contrib/desktop" "$APP_HOME" <<'PY'
from pathlib import Path
import shutil
import sys
src, dest = Path(sys.argv[1]), Path(sys.argv[2])
keep = {"node_modules", "dist", "resources"}
if dest.exists():
    for child in dest.iterdir():
        if child.name in keep:
            continue
        if child.is_dir():
            shutil.rmtree(child)
        else:
            child.unlink()
for child in src.iterdir():
    if child.name in {"node_modules", "dist", "resources"}:
        continue
    target = dest / child.name
    if child.is_dir():
        shutil.copytree(child, target)
    else:
        shutil.copy2(child, target)
PY
cp "$KIANA_BIN" "$APP_HOME/resources/kiana"
chmod +x "$APP_HOME/resources/kiana"

if [[ ! -f "$APP_HOME/icon.png" ]]; then
  python3 - "$APP_HOME/icon.png" <<'PY'
import struct, zlib, sys, pathlib
def chunk(tag, data):
    crc = zlib.crc32(tag + data) & 0xffffffff
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", crc)
w = h = 64
rows = b"".join(b"\x00" + bytes([0x2A, 0x6B, 0xFF, 255]) * w for _ in range(h))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")
pathlib.Path(sys.argv[1]).write_bytes(png)
PY
fi
cp "$APP_HOME/icon.png" "$ICON_DIR/kiana.png"

if [[ -z "${ELECTRON_MIRROR:-}" ]]; then
  export ELECTRON_MIRROR="https://npmmirror.com/mirrors/electron/"
fi
echo "Installing Electron runtime from $ELECTRON_MIRROR (slow; ~100MB Chromium zip)"
(cd "$APP_HOME" && npm install --omit=dev --foreground-scripts --no-audit --no-fund)

cat > "$INSTALL_DIR/kiana-desktop" <<EOF
#!/usr/bin/env bash
set -euo pipefail
export KIANA_BIN="\${KIANA_BIN:-$APP_HOME/resources/kiana}"
ELECTRON="$APP_HOME/node_modules/.bin/electron"
if [[ ! -x "\$ELECTRON" ]]; then
  echo "Electron is missing at \$ELECTRON. Re-run scripts/install-desktop.sh" >&2
  exit 1
fi
exec "\$ELECTRON" "$APP_HOME" "\$@"
EOF
chmod +x "$INSTALL_DIR/kiana-desktop"

python3 - "$REPO_DIR/contrib/kiana-desktop.desktop" "$DESKTOP_DIR/kiana-desktop.desktop" "$INSTALL_DIR/kiana-desktop" <<'PY'
from pathlib import Path
import sys
src, dest, binary = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
text = src.read_text(encoding="utf8")
text = text.replace("Exec=kiana-desktop", f"Exec={binary}")
text = text.replace("TryExec=kiana-desktop", f"TryExec={binary}")
dest.write_text(text, encoding="utf8")
PY

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$DESKTOP_DIR" >/dev/null 2>&1 || true
fi

echo "Installed desktop shell:"
echo "  launcher: $INSTALL_DIR/kiana-desktop"
echo "  app:      $APP_HOME"
echo "  worker:   $KIANA_BIN"
echo "Open from the app menu as Kiana, or run kiana-desktop."
echo "Closing the window can keep DaemonHost in the tray. This is not a signed store package."
echo "Debian package: bash scripts/package-desktop-deb.sh && sudo dpkg -i dist/kiana-desktop_*.deb"

#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
package_root="$(cd "$script_dir/.." && pwd)"

case "$(uname -s)" in
  MSYS*|MINGW*|CYGWIN*) exe_ext=".exe" ;;
  *) exe_ext="" ;;
esac

native_env_path() {
  local path="$1"
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$path"
  else
    printf '%s\n' "$path"
  fi
}

usage() {
  cat >&2 <<'EOF'
Usage: install-release-binary.sh [--install|--uninstall]

Environment:
  INSTALL_DIR  Target directory. Defaults to $HOME/.local/bin.
EOF
}

mode="${1:---install}"
if [[ $# -gt 1 ]]; then
  usage
  exit 2
fi
case "$mode" in
  --install|install) mode="install" ;;
  --uninstall|uninstall) mode="uninstall" ;;
  -h|--help|help)
    usage
    exit 0
    ;;
  *)
    usage
    exit 2
    ;;
esac

binary="$package_root/kiana${exe_ext}"
install_dir="${INSTALL_DIR:-$HOME/.local/bin}"
installed="$install_dir/kiana${exe_ext}"

if [[ "$mode" == "uninstall" ]]; then
  if [[ -e "$installed" ]]; then
    rm -f "$installed"
    echo "Uninstalled $installed"
  else
    echo "Already absent $installed"
  fi
  exit 0
fi

if [[ ! -f "$binary" ]]; then
  echo "release binary not found: $binary" >&2
  exit 1
fi

mkdir -p "$install_dir"
cp "$binary" "$installed"
chmod +x "$installed"

verify_first_start_file="$install_dir/.kiana-install-first-start.json"
KIANA_FIRST_START_FILE="$(native_env_path "$verify_first_start_file")" "$installed" --version >/dev/null
rm -f "$verify_first_start_file"

echo "Installed $installed"

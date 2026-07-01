#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
package_root="$(cd "$script_dir/.." && pwd)"

case "$(uname -s)" in
  MSYS*|MINGW*|CYGWIN*) exe_ext=".exe" ;;
  *) exe_ext="" ;;
esac

binary="$package_root/kiana${exe_ext}"
if [[ ! -f "$binary" ]]; then
  echo "release binary not found: $binary" >&2
  exit 1
fi

install_dir="${INSTALL_DIR:-$HOME/.local/bin}"
installed="$install_dir/kiana${exe_ext}"

mkdir -p "$install_dir"
cp "$binary" "$installed"
chmod +x "$installed"

"$installed" --version >/dev/null

echo "Installed $installed"

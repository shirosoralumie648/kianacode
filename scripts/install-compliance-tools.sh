#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

tool_root="${KIANA_COMPLIANCE_TOOL_ROOT:-target/compliance-tools}"
cargo_home="${KIANA_COMPLIANCE_CARGO_HOME:-target/compliance-cargo-home}"

case "$(uname -s)" in
  MSYS*|MINGW*|CYGWIN*) exe_ext=".exe" ;;
  *) exe_ext="" ;;
esac

mkdir -p "$tool_root" "$cargo_home"

install_tool() {
  local crate="$1"
  local binary="$2"
  if [[ -x "$tool_root/bin/${binary}${exe_ext}" ]]; then
    echo "Using existing $tool_root/bin/${binary}${exe_ext}"
    return 0
  fi
  CARGO_HOME="$cargo_home" cargo install "$crate" --locked --root "$tool_root"
}

install_tool cargo-audit cargo-audit
install_tool cargo-deny cargo-deny

echo "Compliance tools installed in $tool_root/bin"
echo "Add this directory to PATH before running full compliance locally:"
echo "  $tool_root/bin"

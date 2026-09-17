"use strict";

const PACKAGE = "kiana-desktop";
const APP_LIB = "/usr/lib/kiana-desktop";
const CLI_BIN = "/usr/bin/kiana";
const DESKTOP_BIN = "/usr/bin/kiana-desktop";
const DESKTOP_FILE = "/usr/share/applications/kiana-desktop.desktop";
const ICON_FILE = "/usr/share/icons/hicolor/64x64/apps/kiana.png";

function debianArch(machine) {
  const value = String(machine || "").trim();
  if (value === "x86_64" || value === "amd64") {
    return "amd64";
  }
  if (value === "aarch64" || value === "arm64") {
    return "arm64";
  }
  throw new Error(`unsupported_arch:${value || "unknown"}`);
}

function artifactName(version, arch) {
  return `${PACKAGE}_${version}_${arch}.deb`;
}

function control({ version, arch, installedSizeKb }) {
  if (!version || !arch) {
    throw new Error("deb_control_incomplete");
  }
  const size = Number.isFinite(installedSizeKb) ? String(installedSizeKb) : "0";
  return [
    `Package: ${PACKAGE}`,
    `Version: ${version}`,
    "Section: devel",
    "Priority: optional",
    `Architecture: ${arch}`,
    "Maintainer: Kiana Project Contributors <kiana@local>",
    "Homepage: https://github.com/kiana-project/kiana",
    `Installed-Size: ${size}`,
    "Depends: libasound2 | libasound2t64, libatk-bridge2.0-0, libatk1.0-0, libatspi2.0-0, libc6, libcairo2, libcups2 | libcups2t64, libdbus-1-3, libdrm2, libexpat1, libgbm1, libgtk-3-0 | libgtk-3-0t64, libnspr4, libnss3, libpango-1.0-0, libx11-6, libxcb1, libxcomposite1, libxdamage1, libxext6, libxfixes3, libxkbcommon0, libxrandr2, xdg-utils",
    "Recommends: libnotify4",
    "Description: Local-first Kiana workbench",
    " Electron shell around kiana web. Same DaemonHost worker.",
    " Loopback only. Close the window to keep the tray, or quit.",
    " This is a local package, not a signed Debian archive.",
    "",
  ].join("\n");
}

function desktopEntry() {
  return [
    "[Desktop Entry]",
    "Type=Application",
    "Name=Kiana",
    "Comment=Local-first agent workbench (Electron shell over DaemonHost)",
    `Exec=${DESKTOP_BIN}`,
    `TryExec=${DESKTOP_BIN}`,
    "Terminal=false",
    "Icon=kiana",
    "Categories=Development;Utility;",
    "StartupNotify=true",
    "StartupWMClass=Kiana",
    "Keywords=agent;codex;pi;dsh;kiana;",
    "",
  ].join("\n");
}

function launcherScript() {
  return `#!/bin/sh
exec ${APP_LIB}/kiana-desktop "$@"
`;
}

function postinst() {
  return `#!/bin/sh
set -e
sandbox="${APP_LIB}/chrome-sandbox"
if [ -f "$sandbox" ]; then
  chown root:root "$sandbox" 2>/dev/null || true
  chmod 4755 "$sandbox" 2>/dev/null || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
exit 0
`;
}

function copyright() {
  return `Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: kiana
Source: https://github.com/kiana-project/kiana

Files: *
Copyright: Kiana Project Contributors
License: MIT or Apache-2.0
 This package is MIT OR Apache-2.0.
 Electron runtime and Chromium files keep their upstream licenses.
 This is not a signed Debian archive and not a production store release.
`;
}

module.exports = {
  PACKAGE,
  APP_LIB,
  CLI_BIN,
  DESKTOP_BIN,
  DESKTOP_FILE,
  ICON_FILE,
  debianArch,
  artifactName,
  control,
  desktopEntry,
  launcherScript,
  postinst,
  copyright,
};

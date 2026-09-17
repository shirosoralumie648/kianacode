"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");
const {
  debianArch,
  artifactName,
  control,
  desktopEntry,
  launcherScript,
  APP_LIB,
  DESKTOP_BIN,
  CLI_BIN,
} = require("../lib/deb-layout");

test("maps host machines onto Debian architectures", () => {
  assert.equal(debianArch("x86_64"), "amd64");
  assert.equal(debianArch("amd64"), "amd64");
  assert.equal(debianArch("aarch64"), "arm64");
  assert.throws(() => debianArch("ppc64"), /unsupported_arch/);
});

test("deb artifact is a versioned kiana-desktop package name", () => {
  assert.equal(artifactName("0.1.0", "amd64"), "kiana-desktop_0.1.0_amd64.deb");
});

test("control and desktop files install both CLI and Electron launcher", () => {
  const text = control({ version: "0.1.0", arch: "amd64", installedSizeKb: 12 });
  assert.match(text, /^Package: kiana-desktop$/m);
  assert.match(text, /^Architecture: amd64$/m);
  assert.match(text, /not a signed Debian archive/);
  const desktop = desktopEntry();
  assert.match(desktop, new RegExp(`Exec=${DESKTOP_BIN}`));
  assert.match(desktop, /Terminal=false/);
  assert.match(launcherScript(), new RegExp(APP_LIB));
  assert.equal(CLI_BIN, "/usr/bin/kiana");
});

test("package-desktop-deb.sh stages a dpkg tree with worker + desktop entry", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "kiana-deb-"));
  const fakeBin = path.join(root, "kiana-bin");
  fs.writeFileSync(fakeBin, "#!/bin/sh\necho fake\n", { mode: 0o755 });
  const stage = path.join(root, "stage");
  const script = path.join(__dirname, "..", "..", "..", "scripts", "package-desktop-deb.sh");
  const result = spawnSync(
    "bash",
    [script, "--layout-only", "--skip-electron", "--keep-root"],
    {
      env: {
        ...process.env,
        KIANA_BIN: fakeBin,
        KIANA_DEB_ROOT: stage,
        DIST_DIR: path.join(root, "dist"),
        VERSION: "0.1.0",
      },
      encoding: "utf8",
    }
  );
  assert.equal(result.status, 0, result.stderr + result.stdout);
  const controlText = fs.readFileSync(path.join(stage, "DEBIAN/control"), "utf8");
  assert.match(controlText, /Package: kiana-desktop/);
  assert.match(
    fs.readFileSync(path.join(stage, "usr/share/applications/kiana-desktop.desktop"), "utf8"),
    /Exec=\/usr\/bin\/kiana-desktop/
  );
  assert.ok(fs.existsSync(path.join(stage, "usr/bin/kiana")));
  assert.ok(fs.existsSync(path.join(stage, "usr/lib/kiana-desktop/resources/kiana")));
  assert.ok(fs.existsSync(path.join(stage, "usr/lib/kiana-desktop/resources/app/main.js")));
});

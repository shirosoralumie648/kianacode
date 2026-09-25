"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const {
  ASSET_MANIFEST_SCHEMA,
  cacheControlForAsset,
  cspHashForBytes,
  sha256File,
  validateAssetManifest,
  validatePackageFiles,
  verifyAssetManifest,
} = require("../lib/asset-manifest");

const fixture = JSON.parse(
  fs.readFileSync(path.join(__dirname, "fixtures", "ui28-assets.json"), "utf8")
);

function manifestFor(root, appVersion = "0.1.0") {
  const files = ["main.js", "preload.js"];
  return {
    schema: ASSET_MANIFEST_SCHEMA,
    manifest_version: 1,
    app_version: appVersion,
    protocol_schema: "kiana.protocol.v1",
    ui_schema: "kiana.ui.v1",
    license: "MIT OR Apache-2.0",
    csp: { mode: "runtime_nonce", hash_algorithm: "sha256", forbid: ["unsafe-inline", "unsafe-eval"] },
    assets: files.map(file => {
      const bytes = fs.readFileSync(path.join(root, file));
      return { path: file, sha256: sha256File(path.join(root, file)), bytes: bytes.length, cache: "immutable" };
    }),
  };
}

test("UI-28 fixture records package, hash and CSP deny-first cases", () => {
  assert.equal(fixture.schema, "kiana.desktop-assets-fixture.v1");
  assert.ok(fixture.denied.some(item => item.includes("hash")));
  assert.ok(fixture.denied.some(item => item.includes("unsafe-inline")));
  assert.deepEqual(fixture.csp, { mode: "runtime_nonce", hash_algorithm: "sha256" });
});

test("asset manifest verifies exact package bytes and cache/CSP contracts", t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "kiana-ui28-assets-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.writeFileSync(path.join(root, "main.js"), "console.log('main');\n");
  fs.writeFileSync(path.join(root, "preload.js"), "'use strict';\n");
  const packageJson = {
    license: "MIT OR Apache-2.0",
    build: { files: ["main.js", "preload.js", "welcome.html", "asset-manifest.json", "lib/**/*"] },
  };
  const manifest = manifestFor(root);
  assert.equal(verifyAssetManifest(manifest, root, packageJson).assets.length, 2);
  assert.equal(cacheControlForAsset({ cache: "immutable" }), "public, max-age=31536000, immutable");
  assert.equal(cacheControlForAsset({ cache: "no-store" }), "no-store");
  assert.match(cspHashForBytes(Buffer.from("inline")), /^sha256-[A-Za-z0-9+/]+=*$/);
  assert.throws(() => verifyAssetManifest({ ...manifest, assets: [{ ...manifest.assets[0], bytes: 999 }] }, root, packageJson), /digest_mismatch/);
  assert.throws(() => validateAssetManifest({ ...manifest, unknown: true }), /unknown_field/);
  assert.throws(() => validatePackageFiles({ ...packageJson, build: { files: ["main.js", "preload.js", "welcome.html", ".env"] } }), /missing|secret_pattern/);
});

test("UI-28 source guards keep packaging and CSP checks outside execution authority", () => {
  const packageJson = JSON.parse(fs.readFileSync(path.join(__dirname, "..", "package.json"), "utf8"));
  const manifest = fs.readFileSync(path.join(__dirname, "..", "asset-manifest.json"), "utf8");
  const verifier = fs.readFileSync(path.join(__dirname, "..", "..", "..", "scripts", "verify-desktop-assets.js"), "utf8");
  const assetModule = fs.readFileSync(path.join(__dirname, "..", "lib", "asset-manifest.js"), "utf8");
  const webRust = fs.readFileSync(path.join(__dirname, "..", "..", "..", "kiana-entrypoints", "src", "web.rs"), "utf8");
  assert.ok(packageJson.build.files.includes("asset-manifest.json"));
  assert.match(manifest, /kiana\.desktop-assets\.v1/);
  for (const marker of ["verifyAssetManifest", "sha256", "runtime_nonce", "unsafe-inline", "unsafe-eval"]) {
    assert.ok(verifier.includes(marker) || assetModule.includes(marker) || manifest.includes(marker), marker);
  }
  assert.match(webRust, /content-security-policy/);
  assert.doesNotMatch(webRust, /unsafe-inline|unsafe-eval/);
  assert.doesNotMatch(verifier, /web_token|x-kiana-web-token|access_token|refresh_token/i);
});

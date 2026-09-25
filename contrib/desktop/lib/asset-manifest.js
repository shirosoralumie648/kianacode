"use strict";

const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

const ASSET_MANIFEST_SCHEMA = "kiana.desktop-assets.v1";
const MAX_ASSETS = 256;
const DIGEST_PATTERN = /^[a-f0-9]{64}$/;
const FORBIDDEN_PACKAGE_PATTERN = /(^|\/)(?:\.env(?:\.|$)|.*\.(?:pem|key|p12|pfx))$/i;
const FORBIDDEN_CONTENT_MARKERS = [
  "-----BEGIN " + "PRIVATE KEY-----",
  "ghp" + "_",
  "github_pat" + "_",
  "sk-live" + "-",
];

function plain(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function sha256Bytes(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

function sha256File(file) {
  return sha256Bytes(fs.readFileSync(file));
}

function safeRelativeAssetPath(value) {
  if (typeof value !== "string" || value.length === 0 || value.length > 512 ||
      path.isAbsolute(value) || value.includes("\0") || value.split(/[\\/]+/).includes("..") ||
      FORBIDDEN_PACKAGE_PATTERN.test(value)) {
    throw new Error("desktop_asset_path_invalid");
  }
  return value.replaceAll("\\", "/");
}

function validateAssetManifest(manifest) {
  if (!plain(manifest)) throw new Error("desktop_asset_manifest_invalid");
  const allowed = new Set(["schema", "manifest_version", "app_version", "protocol_schema", "ui_schema", "license", "csp", "assets"]);
  if (Object.keys(manifest).some(key => !allowed.has(key))) throw new Error("desktop_asset_manifest_unknown_field");
  if (manifest.schema !== ASSET_MANIFEST_SCHEMA || manifest.manifest_version !== 1) {
    throw new Error("desktop_asset_manifest_schema_mismatch");
  }
  if (typeof manifest.app_version !== "string" || manifest.app_version.length === 0 || manifest.app_version.length > 64) {
    throw new Error("desktop_asset_version_invalid");
  }
  if (manifest.protocol_schema !== "kiana.protocol.v1" || manifest.ui_schema !== "kiana.ui.v1") {
    throw new Error("desktop_asset_protocol_invalid");
  }
  if (manifest.license !== "MIT OR Apache-2.0") throw new Error("desktop_asset_license_invalid");
  if (!plain(manifest.csp) || Object.keys(manifest.csp).some(key => !["mode", "hash_algorithm", "forbid"].includes(key)) ||
      manifest.csp.mode !== "runtime_nonce" || manifest.csp.hash_algorithm !== "sha256" ||
      !Array.isArray(manifest.csp.forbid) || !manifest.csp.forbid.includes("unsafe-inline") ||
      !manifest.csp.forbid.includes("unsafe-eval")) {
    throw new Error("desktop_asset_csp_invalid");
  }
  if (!Array.isArray(manifest.assets) || manifest.assets.length === 0 || manifest.assets.length > MAX_ASSETS) {
    throw new Error("desktop_asset_list_invalid");
  }
  const seen = new Set();
  const assets = manifest.assets.map(asset => {
    if (!plain(asset) || Object.keys(asset).some(key => !["path", "sha256", "bytes", "cache"].includes(key))) {
      throw new Error("desktop_asset_entry_invalid");
    }
    const assetPath = safeRelativeAssetPath(asset.path);
    if (seen.has(assetPath)) throw new Error("desktop_asset_duplicate");
    seen.add(assetPath);
    if (!DIGEST_PATTERN.test(asset.sha256) || !Number.isSafeInteger(asset.bytes) || asset.bytes < 0) {
      throw new Error("desktop_asset_digest_invalid");
    }
    if (!['immutable', 'no-store'].includes(asset.cache)) throw new Error("desktop_asset_cache_invalid");
    return { path: assetPath, sha256: asset.sha256, bytes: asset.bytes, cache: asset.cache };
  });
  return {
    schema: ASSET_MANIFEST_SCHEMA,
    manifest_version: 1,
    app_version: manifest.app_version,
    protocol_schema: manifest.protocol_schema,
    ui_schema: manifest.ui_schema,
    license: manifest.license,
    csp: { mode: manifest.csp.mode, hash_algorithm: manifest.csp.hash_algorithm, forbid: [...manifest.csp.forbid] },
    assets,
  };
}

function validatePackageFiles(packageJson) {
  if (!plain(packageJson) || !plain(packageJson.build) || !Array.isArray(packageJson.build.files)) {
    throw new Error("desktop_package_build_files_missing");
  }
  const files = packageJson.build.files.map(String);
  for (const required of ["main.js", "preload.js", "welcome.html", "asset-manifest.json"]) {
    if (!files.includes(required)) throw new Error("desktop_package_asset_missing:" + required);
  }
  if (files.some(file => FORBIDDEN_PACKAGE_PATTERN.test(file) || /(?:^|[*/])(?:\.env|.*token.*|.*secret.*)/i.test(file))) {
    throw new Error("desktop_package_secret_pattern");
  }
  if (packageJson.license !== "MIT OR Apache-2.0") throw new Error("desktop_package_license_invalid");
  return files;
}

function verifyAssetManifest(manifest, root, packageJson) {
  const normalized = validateAssetManifest(manifest);
  validatePackageFiles(packageJson);
  for (const asset of normalized.assets) {
    const file = path.join(root, asset.path);
    const stat = fs.lstatSync(file);
    if (!stat.isFile() || stat.isSymbolicLink()) throw new Error("desktop_asset_file_invalid:" + asset.path);
    const bytes = fs.readFileSync(file);
    if (bytes.byteLength !== asset.bytes || sha256Bytes(bytes) !== asset.sha256) {
      throw new Error("desktop_asset_digest_mismatch:" + asset.path);
    }
    const text = bytes.toString("utf8");
    if (FORBIDDEN_CONTENT_MARKERS.some(marker => text.includes(marker))) {
      throw new Error("desktop_asset_secret_marker:" + asset.path);
    }
  }
  return normalized;
}

function cacheControlForAsset(asset) {
  if (!asset || !["immutable", "no-store"].includes(asset.cache)) throw new Error("desktop_asset_cache_invalid");
  return asset.cache === "immutable" ? "public, max-age=31536000, immutable" : "no-store";
}

function cspHashForBytes(bytes) {
  return "sha256-" + crypto.createHash("sha256").update(bytes).digest("base64");
}

module.exports = {
  ASSET_MANIFEST_SCHEMA,
  cacheControlForAsset,
  cspHashForBytes,
  safeRelativeAssetPath,
  sha256File,
  validateAssetManifest,
  validatePackageFiles,
  verifyAssetManifest,
};

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { verifyAssetManifest } = require("../contrib/desktop/lib/asset-manifest");

const repositoryRoot = path.resolve(__dirname, "..");
const desktopRoot = path.join(repositoryRoot, "contrib", "desktop");
const manifestPath = path.join(desktopRoot, "asset-manifest.json");
const packagePath = path.join(desktopRoot, "package.json");

const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
const packageJson = JSON.parse(fs.readFileSync(packagePath, "utf8"));
const verified = verifyAssetManifest(manifest, desktopRoot, packageJson);
process.stdout.write(JSON.stringify({
  schema: verified.schema,
  app_version: verified.app_version,
  asset_count: verified.assets.length,
  csp_mode: verified.csp.mode,
}) + "\n");

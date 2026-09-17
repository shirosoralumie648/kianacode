"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");

function findKiana(options = {}) {
  const env = options.env || process.env;
  const home = options.homedir || os.homedir();
  const exists = options.existsSync || ((candidate) => {
    try {
      fs.accessSync(candidate, fs.constants.X_OK);
      return true;
    } catch {
      return false;
    }
  });
  const extra = options.extraCandidates || [];
  const candidates = [
    env.KIANA_BIN,
    options.resourcesKiana,
    ...extra,
    path.join(home, ".local", "bin", "kiana"),
    path.join(home, ".local", "bin", "kiana.exe"),
  ].filter(Boolean);
  for (const candidate of candidates) {
    if (exists(candidate)) {
      return candidate;
    }
  }
  return null;
}

function kianaArgs(workdir) {
  const args = ["web", "--no-open", "--bind", "127.0.0.1:0"];
  if (workdir) {
    args.push("--workdir", workdir);
  }
  return args;
}

module.exports = { findKiana, kianaArgs };

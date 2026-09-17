"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");

function kianaHome(home) {
  return path.join(home || os.homedir(), ".kiana");
}

function workspacesRoot(home) {
  return path.join(kianaHome(home), "workspaces");
}

function isScratchWorkspace(folder, home) {
  if (!folder) {
    return false;
  }
  const root = path.resolve(workspacesRoot(home));
  const target = path.resolve(folder);
  return target === root || target.startsWith(root + path.sep);
}

function scratchName(now = new Date()) {
  const pad = (value) => String(value).padStart(2, "0");
  return (
    "project-" +
    now.getUTCFullYear() +
    pad(now.getUTCMonth() + 1) +
    pad(now.getUTCDate()) +
    "-" +
    pad(now.getUTCHours()) +
    pad(now.getUTCMinutes()) +
    pad(now.getUTCSeconds())
  );
}

function createScratchWorkspace(options = {}) {
  const mkdir = options.mkdirSync || ((dir) => fs.mkdirSync(dir, { recursive: true }));
  const write = options.writeFileSync || ((file, text) => fs.writeFileSync(file, text));
  const exists = options.existsSync || ((file) => fs.existsSync(file));
  const home = options.homedir || os.homedir();
  const now = options.now || new Date();
  let dir = path.join(workspacesRoot(home), scratchName(now));
  if (exists(dir)) {
    dir = `${dir}-${Math.random().toString(36).slice(2, 6)}`;
  }
  mkdir(dir);
  write(
    path.join(dir, "README.md"),
    [
      "# Kiana scratch workspace",
      "",
      "This folder was created because no project was selected.",
      "The agent works here under ~/.kiana/workspaces.",
      "",
    ].join("\n")
  );
  return dir;
}

module.exports = {
  kianaHome,
  workspacesRoot,
  isScratchWorkspace,
  scratchName,
  createScratchWorkspace,
};

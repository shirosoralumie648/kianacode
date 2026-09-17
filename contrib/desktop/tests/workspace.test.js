"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("fs");
const path = require("path");
const {
  workspacesRoot,
  isScratchWorkspace,
  scratchName,
  createScratchWorkspace,
} = require("../lib/workspace");

test("welcome page walks through workspace, trust, then first task", () => {
  const html = fs.readFileSync(path.join(__dirname, "..", "welcome.html"), "utf8");
  assert.match(html, /先选工作区/);
  assert.match(html, /打开已有文件夹/);
  assert.match(html, /先不选项目，自动创建/);
  assert.match(html, /~\/\.kiana\/workspaces/);
  assert.match(html, /信任是什么/);
});

test("scratch projects live under ~/.kiana/workspaces", () => {
  const home = "/home/demo";
  assert.equal(workspacesRoot(home), path.join(home, ".kiana", "workspaces"));
  assert.equal(
    isScratchWorkspace("/home/demo/.kiana/workspaces/project-1", home),
    true
  );
  assert.equal(isScratchWorkspace("/media/code/my-repo", home), false);
});

test("createScratchWorkspace writes a dated folder without picking a GUI path", () => {
  const created = [];
  const files = {};
  const dir = createScratchWorkspace({
    homedir: "/home/demo",
    now: new Date("2026-08-24T08:41:05Z"),
    existsSync: () => false,
    mkdirSync: (value) => created.push(value),
    writeFileSync: (file, text) => {
      files[file] = text;
    },
  });
  assert.equal(dir, "/home/demo/.kiana/workspaces/" + scratchName(new Date("2026-08-24T08:41:05Z")));
  assert.deepEqual(created, [dir]);
  assert.match(files[path.join(dir, "README.md")], /scratch workspace/);
});

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { once } = require("node:events");
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const { groupExists, stopWorker } = require("../lib/worker");

test("desktop_safe_close_leaves_no_orphan_process", async () => {
  const child = spawn(
    process.execPath,
    [
      "-e",
      // Ignore the graceful signal so the bounded SIGKILL/taskkill fallback is exercised.
      "process.on('SIGTERM', () => {}); setInterval(() => {}, 1000);",
    ],
    {
      detached: process.platform !== "win32",
      stdio: "ignore",
      windowsHide: true,
    }
  );
  const pid = child.pid;
  try {
    await once(child, "spawn");
    await stopWorker(child, { graceMs: 150, killMs: 1500 });
    assert.ok(child.exitCode !== null || child.signalCode !== null);
    if (process.platform !== "win32") {
      assert.equal(groupExists(pid), false);
    }
  } finally {
    if (child.exitCode === null && child.signalCode === null) {
      try {
        await stopWorker(child, { graceMs: 50, killMs: 1000 });
      } catch {
        // Preserve the original assertion; CI still reports a failed stop through the test.
      }
    }
  }
});

test("desktop_shell_reuses_loopback_worker_and_safe_close_path", () => {
  const main = fs.readFileSync(path.join(__dirname, "..", "main.js"), "utf8");
  const worker = fs.readFileSync(path.join(__dirname, "..", "lib", "worker.js"), "utf8");
  const welcome = fs.readFileSync(path.join(__dirname, "..", "welcome.html"), "utf8");
  for (const marker of [
    "workspace:state",
    "workspace:open",
    "workspace:new",
    "workspace:continue",
    "createScratchWorkspace",
    "autoTrust",
    "waitForUrl",
    "kiana web startup timed out",
    "createTray",
    "Keep in background",
    "Quit stops the worker",
    "before-quit",
    "shutdownComplete",
    "stopWorker(proc)",
    "detached: process.platform !== \"win32\"",
    "127.0.0.1:0",
    "desktop_safe_close_leaves_no_orphan_process",
  ]) {
    assert.ok(
      main.includes(marker) || worker.includes(marker) || welcome.includes(marker),
      `desktop marker missing: ${marker}`
    );
  }
  assert.match(main, /event\.preventDefault\(\);/);
  assert.match(main, /if \(shutdownComplete\) return/);
  assert.match(worker, /process\.kill\(-proc\.pid, signal\)/);
  assert.match(worker, /worker_stop_unconfirmed/);
});

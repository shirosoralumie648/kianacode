"use strict";

const { spawn } = require("child_process");
const { setTimeout: delay } = require("timers/promises");

function groupExists(pid) {
  try { process.kill(-pid, 0); return true; }
  catch (error) { if (error.code === "ESRCH") return false; throw error; }
}

function signalGroup(proc, signal) {
  if (process.platform === "win32") {
    if (proc.exitCode === null && proc.signalCode === null) proc.kill(signal);
    return;
  }
  try { process.kill(-proc.pid, signal); }
  catch (error) { if (error.code !== "ESRCH") throw error; }
}

async function waitStopped(proc, deadline) {
  while (Date.now() < deadline) {
    const exited = proc.exitCode !== null || proc.signalCode !== null;
    if (exited && (process.platform === "win32" || !groupExists(proc.pid))) return true;
    await delay(25);
  }
  return false;
}

async function killWindowsTree(pid) {
  await new Promise((resolve, reject) => {
    const killer = spawn("taskkill", ["/pid", String(pid), "/T", "/F"], { windowsHide: true, stdio: "ignore" });
    const timer = setTimeout(() => { killer.kill(); reject(new Error("worker_tree_stop_timeout")); }, 2000);
    killer.once("error", error => { clearTimeout(timer); reject(error); });
    killer.once("exit", code => { clearTimeout(timer); code === 0 ? resolve() : reject(new Error("worker_tree_stop_unconfirmed")); });
  });
}

// The worker owns a dedicated process group. SIGTERM lets DaemonHost cancel its
// active runs and persist outcomes; SIGKILL is only the bounded final fallback.
async function stopWorker(proc, { graceMs = 12000, killMs = 2000 } = {}) {
  if (!proc || !proc.pid) return;
  if (process.platform !== "win32" && !groupExists(proc.pid)) return;
  signalGroup(proc, "SIGTERM");
  if (await waitStopped(proc, Date.now() + graceMs)) return;
  if (process.platform === "win32") await killWindowsTree(proc.pid);
  else signalGroup(proc, "SIGKILL");
  if (!await waitStopped(proc, Date.now() + killMs)) throw new Error("worker_stop_unconfirmed");
}

module.exports = { groupExists, stopWorker };

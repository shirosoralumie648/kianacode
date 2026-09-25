"use strict";

const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const { URL } = require("node:url");

const READY_PREFIX = "KIANA_DESKTOP_READY=";
const READY_SCHEMA = "kiana.desktop-ready.v1";
const HEALTH_SCHEMA = "kiana.health-snapshot.v1";
const SIDECAR_SCHEMA = "kiana.ui-instance-record.v1";
const MAX_READY_BYTES = 4096;
const MAX_SIDECAR_BYTES = 64 * 1024;

function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map(key =>
      `${JSON.stringify(key)}:${canonicalJson(value[key])}`
    ).join(",")}}`;
  }
  return JSON.stringify(value);
}

function jsonDigest(value) {
  return `sha256:${crypto.createHash("sha256").update(canonicalJson(value)).digest("hex")}`;
}

function safeRegularFile(file, maxBytes) {
  const stat = fs.lstatSync(file);
  if (stat.isSymbolicLink() || !stat.isFile() || stat.size > maxBytes) return false;
  if (process.platform !== "win32" && (stat.mode & 0o077) !== 0) return false;
  return true;
}

function safeDirectory(directory) {
  const stat = fs.lstatSync(directory);
  if (stat.isSymbolicLink() || !stat.isDirectory() ||
      (process.platform !== "win32" && (stat.mode & 0o077) !== 0)) {
    throw new Error("desktop_sidecar_directory_invalid");
  }
}

function readInstanceSidecar(workspace, ready, expectedPid) {
  safeDirectory(path.join(workspace, ".kiana"));
  const directory = path.join(workspace, ".kiana", "instances");
  safeDirectory(directory);
  const lock = path.join(directory, "instance.lock");
  try {
    if (!safeRegularFile(lock, 0)) throw new Error("desktop_sidecar_lock_invalid");
  } catch {
    throw new Error("desktop_sidecar_lock_invalid");
  }
  const recordPath = path.join(directory, `${ready.instance_id}.json`);
  if (!/^[a-zA-Z0-9-]{1,256}$/.test(ready.instance_id)) {
    throw new Error("desktop_sidecar_record_invalid");
  }
  try {
    if (!safeRegularFile(recordPath, MAX_SIDECAR_BYTES)) throw new Error("desktop_sidecar_record_invalid");
  } catch {
    throw new Error("desktop_sidecar_record_invalid");
  }
  let record;
  try { record = JSON.parse(fs.readFileSync(recordPath, "utf8")); }
  catch { throw new Error("desktop_sidecar_record_invalid"); }
  const expectedKeys = [
    "authority_epoch", "endpoint_digest", "instance_id", "pid", "protocol_schema",
    "ready", "record_digest", "schema", "transport", "workspace_digest",
  ].sort().join(",");
  if (!record || typeof record !== "object" || Array.isArray(record) ||
      Object.keys(record).sort().join(",") !== expectedKeys ||
      record.schema !== SIDECAR_SCHEMA || record.protocol_schema !== "kiana.protocol.v1" ||
      record.transport !== "in_process" ||
      record.ready !== true || !Number.isSafeInteger(record.authority_epoch) || record.authority_epoch <= 0 ||
      record.pid !== expectedPid || record.instance_id !== ready.instance_id ||
      record.workspace_digest !== jsonDigest({ workspace }) ||
      record.endpoint_digest !== jsonDigest({ endpoint: ready.url }) ||
      record.workspace_digest !== ready.workspace_digest || record.endpoint_digest !== ready.endpoint_digest ||
      record.authority_epoch !== ready.authority_epoch || record.record_digest !== ready.record_digest) {
    throw new Error("desktop_sidecar_identity_mismatch");
  }
  const digestInput = { ...record, record_digest: "" };
  if (record.record_digest !== jsonDigest(digestInput)) throw new Error("desktop_sidecar_digest_mismatch");
  return record;
}

function validReady(record, { nonce, workspace, pid }) {
  if (!record || typeof record !== "object" || Array.isArray(record) ||
      Object.keys(record).sort().join(",") !==
        "authority_epoch,endpoint_digest,epoch,feed_instance_id,instance_id,nonce,pid,protocol_schema,record_digest,schema,sidecar_schema,url,workspace,workspace_digest") {
    return false;
  }
  let canonicalWorkspace;
  try { canonicalWorkspace = fs.realpathSync(workspace); } catch { return false; }
  if (record.schema !== READY_SCHEMA || record.sidecar_schema !== SIDECAR_SCHEMA ||
      record.protocol_schema !== "kiana.protocol.v1" ||
      record.nonce !== nonce || record.pid !== pid ||
      typeof record.instance_id !== "string" || !/^[a-zA-Z0-9-]{1,256}$/.test(record.instance_id) ||
      !Number.isSafeInteger(record.authority_epoch) || record.authority_epoch <= 0 ||
      typeof record.feed_instance_id !== "string" || !/^[a-zA-Z0-9-]{1,256}$/.test(record.feed_instance_id) ||
      typeof record.epoch !== "string" || !/^[a-zA-Z0-9-]{1,256}$/.test(record.epoch) ||
      typeof record.workspace !== "string" || record.workspace !== canonicalWorkspace) {
    return false;
  }
  try {
    const url = new URL(record.url);
    return url.protocol === "http:" && url.hostname === "127.0.0.1" &&
      Number(url.port) > 0 && url.username === "" && url.password === "" &&
      url.pathname === "/" && url.search === "" && url.hash === "" &&
      record.workspace_digest === jsonDigest({ workspace: canonicalWorkspace }) &&
      record.endpoint_digest === jsonDigest({ endpoint: record.url }) &&
      [record.workspace_digest, record.endpoint_digest, record.record_digest].every(value =>
        typeof value === "string" && /^sha256:[a-f0-9]{64}$/.test(value)
      );
  } catch {
    return false;
  }
}

function parseReadyLine(line, identity) {
  if (!line.startsWith(READY_PREFIX)) return null;
  if (Buffer.byteLength(line) > MAX_READY_BYTES) throw new Error("desktop_ready_oversized");
  let record;
  try { record = JSON.parse(line.slice(READY_PREFIX.length)); }
  catch { throw new Error("desktop_ready_invalid"); }
  if (!validReady(record, identity)) throw new Error("desktop_ready_identity_mismatch");
  return record;
}

async function waitForReady(proc, identity, timeoutMs = 20000) {
  return new Promise((resolve, reject) => {
    let buffer = "";
    let settled = false;
    const cleanup = () => {
      clearTimeout(timer);
      proc.stdout.off("data", onData);
      proc.off("exit", onExit);
      proc.off("error", onError);
    };
    const finish = (error, record) => {
      if (settled) return;
      settled = true;
      cleanup();
      error ? reject(error) : resolve(record);
    };
    const onData = chunk => {
      buffer += chunk.toString();
      if (Buffer.byteLength(buffer) > MAX_READY_BYTES * 4) {
        finish(new Error("desktop_ready_output_limit"));
        return;
      }
      let newline;
      while ((newline = buffer.indexOf("\n")) !== -1) {
        const line = buffer.slice(0, newline).trimEnd();
        buffer = buffer.slice(newline + 1);
        try {
          const record = parseReadyLine(line, identity);
          if (record) return finish(null, record);
        } catch (error) { return finish(error); }
      }
    };
    const onExit = code => finish(new Error(`desktop_worker_exited_before_ready:${code ?? "unknown"}`));
    const onError = error => finish(error);
    const timer = setTimeout(() => finish(new Error("kiana web startup timed out")), timeoutMs);
    proc.stdout.on("data", onData);
    proc.once("exit", onExit);
    proc.once("error", onError);
  });
}

function healthMatches(health, record) {
  return health && health.schema === HEALTH_SCHEMA && health.ok === true &&
    health.loopback === true && health.instance_id === record.feed_instance_id &&
    health.epoch === record.epoch && health.protocol_schema === record.protocol_schema &&
    health.pid === record.pid && health.desktop_instance_id === record.instance_id &&
    health.desktop_authority_epoch === record.authority_epoch &&
    health.desktop_workspace_digest === record.workspace_digest &&
    health.desktop_record_digest === record.record_digest;
}

async function probeReady(record, fetchFn, { attempts = 20, delayMs = 100, verifyInstance = async () => {} } = {}) {
  for (let attempt = 0; attempt < attempts; attempt++) {
    await verifyInstance();
    let response;
    let health;
    try {
      response = await fetchFn(`${record.url}/api/health`, { signal: AbortSignal.timeout(1000) });
      if (response.ok) health = await response.json();
    } catch { /* Server may not yet be accepting connections. */ }
    if (response && healthMatches(health, record)) {
      await verifyInstance();
      return;
    }
    if (attempt + 1 < attempts) await new Promise(resolve => setTimeout(resolve, delayMs));
  }
  throw new Error("desktop_health_identity_mismatch");
}

module.exports = {
  READY_PREFIX, READY_SCHEMA, SIDECAR_SCHEMA, parseReadyLine, waitForReady,
  readInstanceSidecar, healthMatches, probeReady,
};

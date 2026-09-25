"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const {
  READY_PREFIX,
  parseReadyLine,
  readInstanceSidecar,
  healthMatches,
  probeReady,
} = require("../lib/readiness");

function digest(value) {
  const canonical = input => {
    if (Array.isArray(input)) return `[${input.map(canonical).join(",")}]`;
    if (input && typeof input === "object") {
      return `{${Object.keys(input).sort().map(key => `${JSON.stringify(key)}:${canonical(input[key])}`).join(",")}}`;
    }
    return JSON.stringify(input);
  };
  return `sha256:${crypto.createHash("sha256").update(canonical(value)).digest("hex")}`;
}

function fixture() {
  const workspace = fs.mkdtempSync(path.join(os.tmpdir(), "kiana-ui25-"));
  const directory = path.join(workspace, ".kiana", "instances");
  fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
  fs.writeFileSync(path.join(directory, "instance.lock"), "", { mode: 0o600 });
  const endpoint = "http://127.0.0.1:43123";
  const record = {
    authority_epoch: 7,
    endpoint_digest: digest({ endpoint }),
    instance_id: "lease-25-01",
    pid: process.pid,
    protocol_schema: "kiana.protocol.v1",
    ready: true,
    record_digest: "",
    schema: "kiana.ui-instance-record.v1",
    transport: "in_process",
    workspace_digest: digest({ workspace }),
  };
  record.record_digest = digest(record);
  const recordPath = path.join(directory, `${record.instance_id}.json`);
  fs.writeFileSync(recordPath, JSON.stringify(record), { mode: 0o600 });
  const ready = {
    authority_epoch: record.authority_epoch,
    endpoint_digest: record.endpoint_digest,
    epoch: "feed-epoch-25",
    feed_instance_id: "feed-instance-25",
    instance_id: record.instance_id,
    nonce: "ready.nonce-25",
    pid: process.pid,
    protocol_schema: record.protocol_schema,
    record_digest: record.record_digest,
    schema: "kiana.desktop-ready.v1",
    sidecar_schema: record.schema,
    url: endpoint,
    workspace,
    workspace_digest: record.workspace_digest,
  };
  return { workspace, directory, record, ready, recordPath };
}

function health(ready) {
  return {
    schema: "kiana.health-snapshot.v1",
    ok: true,
    loopback: true,
    instance_id: ready.feed_instance_id,
    epoch: ready.epoch,
    protocol_schema: ready.protocol_schema,
    pid: ready.pid,
    desktop_instance_id: ready.instance_id,
    desktop_authority_epoch: ready.authority_epoch,
    desktop_workspace_digest: ready.workspace_digest,
    desktop_record_digest: ready.record_digest,
  };
}

test("UI-25 fixture declares deny-first attachment cases", () => {
  const fixturePath = path.join(__dirname, "fixtures", "ui25-readiness.json");
  const contract = JSON.parse(fs.readFileSync(fixturePath, "utf8"));
  assert.equal(contract.schema, "kiana.desktop-readiness-fixture.v1");
  assert.ok(contract.denied.some(item => item.includes("stderr URL")));
  assert.ok(contract.denied.some(item => item.includes("reused pid")));
  assert.ok(contract.denied.some(item => item.includes("automatic session resume")));
});

test("structured ready record is bound to nonce, pid and canonical workspace", t => {
  const current = fixture();
  t.after(() => fs.rmSync(current.workspace, { recursive: true, force: true }));
  const identity = { nonce: current.ready.nonce, workspace: current.workspace, pid: process.pid };
  assert.deepEqual(
    parseReadyLine(`${READY_PREFIX}${JSON.stringify(current.ready)}`, identity),
    current.ready
  );
  assert.throws(() => parseReadyLine(`${READY_PREFIX}${JSON.stringify({ ...current.ready, pid: process.pid + 1 })}`, identity), /identity_mismatch/);
  assert.throws(() => parseReadyLine(`${READY_PREFIX}${JSON.stringify({ ...current.ready, nonce: "old-nonce" })}`, identity), /identity_mismatch/);
  assert.equal(parseReadyLine("KIANA_WEB_URL=http://127.0.0.1:43123", identity), null);
  assert.throws(() => parseReadyLine(`${READY_PREFIX}${JSON.stringify({ ...current.ready, url: "http://192.0.2.1:43123" })}`, identity), /identity_mismatch/);
});

test("sidecar discovery rejects stale pid, wrong workspace, protocol and instance", t => {
  const current = fixture();
  t.after(() => fs.rmSync(current.workspace, { recursive: true, force: true }));
  assert.equal(readInstanceSidecar(current.workspace, current.ready, process.pid).record_digest, current.record.record_digest);

  const wrongPid = { ...current.record, pid: process.pid + 1 };
  wrongPid.record_digest = "";
  wrongPid.record_digest = digest(wrongPid);
  fs.writeFileSync(current.recordPath, JSON.stringify(wrongPid), { mode: 0o600 });
  assert.throws(() => readInstanceSidecar(current.workspace, current.ready, process.pid), /identity_mismatch/);

  fs.writeFileSync(current.recordPath, JSON.stringify(current.record), { mode: 0o600 });
  const wrongWorkspace = `${current.workspace}-other`;
  assert.throws(() => readInstanceSidecar(wrongWorkspace, current.ready, process.pid));
  assert.throws(() => readInstanceSidecar(current.workspace, { ...current.ready, instance_id: "other-instance" }, process.pid), /record_invalid/);

  const wrongProtocol = { ...current.record, protocol_schema: "kiana.protocol.v0", record_digest: "" };
  wrongProtocol.record_digest = digest(wrongProtocol);
  fs.writeFileSync(current.recordPath, JSON.stringify(wrongProtocol), { mode: 0o600 });
  assert.throws(() => readInstanceSidecar(current.workspace, current.ready, process.pid), /identity_mismatch/);
});

test("sidecar lock and records reject symbolic links and broad permissions", t => {
  const current = fixture();
  t.after(() => fs.rmSync(current.workspace, { recursive: true, force: true }));
  const lock = path.join(current.directory, "instance.lock");
  fs.unlinkSync(lock);
  fs.symlinkSync(current.recordPath, lock);
  assert.throws(() => readInstanceSidecar(current.workspace, current.ready, process.pid), /lock_invalid/);
  fs.unlinkSync(lock);
  fs.writeFileSync(lock, "", { mode: 0o600 });
  if (process.platform !== "win32") {
    fs.chmodSync(current.recordPath, 0o644);
    assert.throws(() => readInstanceSidecar(current.workspace, current.ready, process.pid), /record_invalid/);
    fs.chmodSync(current.recordPath, 0o600);
    fs.chmodSync(current.directory, 0o755);
    assert.throws(() => readInstanceSidecar(current.workspace, current.ready, process.pid), /directory_invalid/);
  }
});

test("health identity must match feed and discovered desktop lease", t => {
  const current = fixture();
  assert.equal(healthMatches(health(current.ready), current.ready), true);
  assert.equal(healthMatches({ ...health(current.ready), desktop_instance_id: "other" }, current.ready), false);
  assert.equal(healthMatches({ ...health(current.ready), pid: current.ready.pid + 1 }, current.ready), false);
  assert.equal(healthMatches({ ...health(current.ready), desktop_workspace_digest: "wrong" }, current.ready), false);
});

test("health probe revalidates the discovered child and never accepts a mismatched peer", async () => {
  const current = fixture();
  let verifyCount = 0;
  let fetchedUrl = null;
  await assert.rejects(
    probeReady(current.ready, async url => {
      fetchedUrl = url;
      return { ok: true, json: async () => ({ ...health(current.ready), desktop_record_digest: "wrong" }) };
    }, { attempts: 1, verifyInstance: async () => { verifyCount++; } }),
    /identity_mismatch/
  );
  assert.equal(fetchedUrl, `${current.ready.url}/api/health`);
  assert.equal(verifyCount, 1);

  verifyCount = 0;
  await probeReady(current.ready, async url => {
    fetchedUrl = url;
    return { ok: true, json: async () => health(current.ready) };
  }, { attempts: 1, verifyInstance: async () => { verifyCount++; } });
  assert.equal(verifyCount, 2);
});

test("desktop source only attaches after structured stdout, sidecar discovery and health identity", () => {
  const main = fs.readFileSync(path.join(__dirname, "..", "main.js"), "utf8");
  const rust = fs.readFileSync(path.join(__dirname, "..", "..", "..", "kiana-entrypoints", "src", "web.rs"), "utf8");
  const worker = fs.readFileSync(path.join(__dirname, "..", "lib", "worker.js"), "utf8");
  const readiness = fs.readFileSync(path.join(__dirname, "..", "lib", "readiness.js"), "utf8");
  const fixtureText = fs.readFileSync(path.join(__dirname, "fixtures", "ui25-readiness.json"), "utf8");
  assert.match(main, /waitForReady\(proc, \{ nonce: readyNonce, workspace, pid: proc\.pid \}\)/);
  assert.match(main, /readInstanceSidecar\(workspace, ready, proc\.pid\)/);
  assert.match(main, /probeReady\(ready,[\s\S]*verifyInstance/);
  assert.match(main, /child\.stderr\.resume\(\)/);
  assert.doesNotMatch(main, /parseWebUrl|parse-url/);
  assert.match(readiness, /proc\.stdout\.on\("data", onData\)/);
  assert.doesNotMatch(readiness, /proc\.stderr/);
  assert.match(rust, /acquire_instance\(&workdir/);
  assert.match(rust, /"record_digest": record\.record_digest/);
  assert.match(rust, /"desktop_authority_epoch"/);
  assert.match(worker, /proc\.exitCode !== null \|\| proc\.signalCode !== null/);
  assert.match(worker, /worker_stop_unconfirmed/);
  assert.ok(fixtureText.includes("automatic session resume"));
  assert.doesNotMatch(main, /resume_run|resumeRun|\/api\/resume/);
});

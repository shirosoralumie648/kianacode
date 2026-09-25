"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const {
  DESKTOP_STORE_SCHEMA,
  emptyDesktopStore,
  mergeDesktopStore,
  readDesktopStore,
  reattachPlan,
  validateDesktopStore,
  writeDesktopStore,
  workspaceReference,
} = require("../lib/desktop-persistence");

const fixture = JSON.parse(
  fs.readFileSync(path.join(__dirname, "fixtures", "ui27-persistence.json"), "utf8")
);

test("UI-27 fixture records metadata-only persistence and detach denies", () => {
  assert.equal(fixture.schema, "kiana.desktop-persistence-fixture.v1");
  assert.ok(fixture.denied.some(item => item.includes("access token")));
  assert.ok(fixture.denied.some(item => item.includes("old epoch cursor")));
  assert.equal(fixture.reattach.requires_new_handshake, true);
  assert.equal(fixture.reattach.auto_resume, false);
});

test("desktop store keeps bounded references and reattach never submits old cursor", () => {
  const workspace = workspaceReference(path.join(os.tmpdir(), "kiana-ui27-workspace"));
  let store = emptyDesktopStore();
  store = mergeDesktopStore(store, {
    workspace_path: workspace.path,
    layout: { x: 10, y: 20, width: 1280, height: 800 },
    draft_policy: "restore_prompt",
    instance_ref: { instance_id: "instance-27", authority_epoch: 3, feed_epoch: "feed-27" },
    session_ref: { session_id: "session-27", scope_digest: "a".repeat(64) },
    last_cursor: { epoch: "feed-27", sequence: 18 },
    detached: true,
  });
  assert.equal(store.schema, DESKTOP_STORE_SCHEMA);
  assert.equal(store.workspace_ref.digest, workspace.digest);
  assert.equal(store.last_cursor.sequence, 18);
  const plan = reattachPlan(store);
  assert.equal(plan.workspace_path, workspace.path);
  assert.equal(plan.requires_new_handshake, true);
  assert.equal(plan.auto_resume, false);
  assert.equal(plan.auto_approve, false);
  assert.equal(plan.auto_trust, false);
  assert.equal(Object.hasOwn(plan, "cursor"), false);
  assert.equal(plan.cursor_disposition, "discard_until_new_handshake");
  assert.throws(() => mergeDesktopStore(store, { access_token: "secret" }), /sensitive/);
  assert.throws(() => validateDesktopStore({ ...store, unknown: true }), /unknown_field/);
  const realWorkspace = fs.mkdtempSync(path.join(os.tmpdir(), "kiana-ui27-workspace-real-"));
  const linkedWorkspace = `${realWorkspace}-link`;
  fs.symlinkSync(realWorkspace, linkedWorkspace);
  assert.throws(() => workspaceReference(linkedWorkspace), /workspace_symlink/);
  fs.rmSync(realWorkspace, { recursive: true, force: true });
  fs.unlinkSync(linkedWorkspace);
});

test("desktop store writes atomically with restricted mode and rejects symlink targets", t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "kiana-ui27-store-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const file = path.join(root, "desktop.json");
  const store = emptyDesktopStore();
  writeDesktopStore(file, store);
  assert.deepEqual(readDesktopStore(file), store);
  if (process.platform !== "win32") {
    assert.equal(fs.statSync(file).mode & 0o777, 0o600);
    fs.chmodSync(file, 0o644);
    assert.throws(() => readDesktopStore(file), /permissions/);
    fs.chmodSync(file, 0o600);
    const link = path.join(root, "link.json");
    fs.symlinkSync(file, link);
    assert.throws(() => writeDesktopStore(link, store), /symlink/);
  }
});

test("UI-27 source guard keeps persistence outside execution and token boundaries", () => {
  const main = fs.readFileSync(path.join(__dirname, "..", "main.js"), "utf8");
  const persistence = fs.readFileSync(path.join(__dirname, "..", "lib", "desktop-persistence.js"), "utf8");
  const preload = fs.readFileSync(path.join(__dirname, "..", "preload.js"), "utf8");
  for (const marker of [
    "readDesktopStore",
    "writeDesktopStore",
    "reattachPlan",
    "workspace_ref",
    "last_cursor",
    "detached",
    "autoTrustScratch",
    "auto_resume: false",
    "desktop_store_sensitive_field",
  ]) {
    assert.ok(main.includes(marker) || persistence.includes(marker) || preload.includes(marker), marker);
  }
  assert.match(main, /openWorkspace\(plan\.workspace_path, \{ autoTrustScratch: false \}\)/);
  assert.doesNotMatch(persistence, /writeFileSync\([^\n]*access_token/);
  assert.doesNotMatch(main, /persist.*instance_token/i);
});

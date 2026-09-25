"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const {
  closeAttention,
  createDesktopState,
  reduceDesktopState,
} = require("../lib/desktop-state");
const { closePrompt } = require("../lib/close-policy");
const {
  NOTIFICATION_SCHEMA,
  NotificationBridge,
  descriptorForServerFact,
} = require("../lib/notifications");

const fixture = JSON.parse(
  fs.readFileSync(path.join(__dirname, "fixtures", "ui26-workspace-tray.json"), "utf8")
);
const binding = "a".repeat(64);

function fact(overrides = {}) {
  return {
    schema: NOTIFICATION_SCHEMA,
    source: "server",
    kind: "terminal",
    notification_id: "terminal:run-26:epoch-26:1",
    feed_epoch: "epoch-26",
    sequence: 1,
    status: "completed",
    workspace_binding_digest: binding,
    ...overrides,
  };
}

test("UI-26 fixture records deny-first workspace, notification and close rules", () => {
  assert.equal(fixture.schema, "kiana.desktop-workspace-tray-fixture.v1");
  for (const item of fixture.denied) assert.equal(typeof item, "string");
  assert.ok(fixture.denied.some(item => item.includes("private content")));
  assert.ok(fixture.denied.some(item => item.includes("implicitly cancel")));
  assert.ok(fixture.accepted.some(item => item.includes("typed intent")));
});

test("desktop state resets attention on workspace switch and deduplicates server facts", () => {
  let state = createDesktopState();
  state = reduceDesktopState(state, { type: "workspace_requested", workspace: "/workspace/one" });
  state = reduceDesktopState(state, { type: "worker_ready", instance_id: "instance-26", epoch: "feed-26" });
  const approval = fact({
    kind: "approval",
    status: "pending",
    notification_id: "approval:26:1",
  });
  state = reduceDesktopState(state, { type: "server_fact", fact: approval });
  state = reduceDesktopState(state, { type: "server_fact", fact: approval });
  state = reduceDesktopState(state, { type: "draft_changed", dirty: true });
  assert.deepEqual(closeAttention(state), {
    pending_count: 1,
    unknown_count: 0,
    draft_dirty: false,
    requires_attention: true,
  });
  state = reduceDesktopState(state, { type: "workspace_requested", workspace: "/workspace/two" });
  assert.equal(state.workspace, "/workspace/two");
  assert.equal(closeAttention(state).requires_attention, false);
  assert.equal(state.draft_dirty, false);
});

test("desktop state rejects malformed server facts before attention mutation", () => {
  const state = createDesktopState();
  assert.throws(
    () => reduceDesktopState(state, { type: "server_fact", fact: fact({ sequence: 0 }) }),
    /desktop_server_fact_invalid/
  );
  assert.throws(
    () => reduceDesktopState(state, { type: "server_fact", fact: fact({ status: "running" }) }),
    /desktop_server_fact_invalid/
  );
});

test("notification bridge accepts only bound server facts and fixed redacted copy", () => {
  const bridge = new NotificationBridge({ workspaceBindingDigest: binding });
  const accepted = bridge.accept(fact({ status: "result_unknown" }));
  assert.equal(accepted.ok, true);
  assert.equal(accepted.disposition, "new");
  assert.match(accepted.value.body, /回执/);
  assert.doesNotMatch(accepted.value.body, /workspace|token|secret|run-26/);
  assert.deepEqual(bridge.accept(fact({ status: "result_unknown" })), {
    ok: true,
    disposition: "duplicate",
  });
  assert.equal(
    descriptorForServerFact(fact({ workspace_binding_digest: "b".repeat(64) }), {
      workspaceBindingDigest: binding,
    }).reason,
    "desktop_notification_workspace_mismatch"
  );
  assert.equal(
    descriptorForServerFact(fact({ kind: "approval", status: "completed" }), {
      workspaceBindingDigest: binding,
    }).reason,
    "desktop_notification_status_invalid"
  );
  assert.equal(
    bridge.accept({ ...fact(), private_body: "do not show" }).reason,
    "desktop_notification_unknown_field"
  );
  assert.equal(
    bridge.accept(fact({ status: "failed" })).reason,
    "desktop_notification_replay_conflict"
  );
});

test("close policy surfaces attention but declares no implicit execution mutation", () => {
  const prompt = closePrompt({ pending_count: 2, unknown_count: 1, draft_dirty: true });
  assert.equal(prompt.attention, true);
  assert.match(prompt.detail, /2 个待处理/);
  assert.match(prompt.detail, /1 个结果尚未确认/);
  assert.match(prompt.detail, /不会取消/);
  assert.equal(prompt.mutation, "none");
});

test("UI-26 source keeps desktop effects behind server fact and typed workspace boundaries", () => {
  const main = fs.readFileSync(path.join(__dirname, "..", "main.js"), "utf8");
  const preload = fs.readFileSync(path.join(__dirname, "..", "preload.js"), "utf8");
  const ipc = fs.readFileSync(path.join(__dirname, "..", "lib", "ipc-security.js"), "utf8");
  const notifications = fs.readFileSync(path.join(__dirname, "..", "lib", "notifications.js"), "utf8");
  const closePolicy = fs.readFileSync(path.join(__dirname, "..", "lib", "close-policy.js"), "utf8");
  const page = fs.readFileSync(path.join(__dirname, "..", "..", "..", "kiana-entrypoints", "src", "web_page.html"), "utf8");
  for (const marker of [
    "NotificationBridge",
    "notifyServerFact",
    "desktop.notification.server-fact.v1",
    "workspace_binding_digest",
    "closePrompt",
    "closeAttention",
    "mutation: \"none\"",
  ]) {
    assert.ok(main.includes(marker) || preload.includes(marker) || ipc.includes(marker) ||
      notifications.includes(marker) || closePolicy.includes(marker) || page.includes(marker), marker);
  }
  assert.match(page, /notifyDesktopServerFact\('approval'/);
  assert.match(page, /notifyDesktopServerFact\('terminal'/);
  assert.doesNotMatch(page, /notifyServerFact\([^)]*JSON\.stringify/);
  assert.doesNotMatch(notifications, /fact\.(body|title|path|token|secret)/);
});

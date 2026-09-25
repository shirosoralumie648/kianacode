"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const {
  DESKTOP_NOTIFICATION_PERMISSION_STATES,
  DesktopNotificationAdapter,
  NOTIFICATION_SCHEMA,
} = require("../lib/notifications");

const fixture = JSON.parse(
  fs.readFileSync(path.join(__dirname, "fixtures", "nm16-desktop-notification.json"), "utf8")
);
const binding = "a".repeat(64);

function fact(overrides = {}) {
  return {
    schema: NOTIFICATION_SCHEMA,
    source: "server",
    kind: "terminal",
    notification_id: "terminal:nm16:epoch:1",
    feed_epoch: "epoch-nm16",
    sequence: 1,
    status: "completed",
    workspace_binding_digest: binding,
    ...overrides,
  };
}

class FakeNotification {
  static calls = [];

  constructor(options) {
    this.options = options;
    FakeNotification.calls.push(options);
  }

  show() {
    this.shown = true;
  }
}

test("NM-16 fixture records permission, redaction and close/detach boundaries", () => {
  assert.equal(fixture.schema, "kiana.desktop-notification-adapter.v1");
  assert.deepEqual(fixture.permission_states, [...DESKTOP_NOTIFICATION_PERMISSION_STATES]);
  for (const item of fixture.deny_first) assert.equal(typeof item, "string");
  assert.ok(fixture.deny_first.includes("permission_denied_is_not_shown"));
  assert.ok(fixture.recovery.includes("durable_inbox_remains_server_owned"));
});

test("permission denied and unknown never claim an OS notification was shown", () => {
  FakeNotification.calls = [];
  const denied = new DesktopNotificationAdapter({
    workspaceBindingDigest: binding,
    permission: "denied",
    notifier: FakeNotification,
  });
  assert.equal(denied.accept(fact()).disposition, "permission_denied");
  assert.equal(FakeNotification.calls.length, 0);

  const unknown = new DesktopNotificationAdapter({
    workspaceBindingDigest: binding,
    permission: "unknown",
    notifier: FakeNotification,
  });
  assert.equal(unknown.accept(fact({ notification_id: "terminal:nm16:epoch:2" })).disposition, "unavailable");
  assert.equal(FakeNotification.calls.length, 0);
  assert.deepEqual(unknown.setPermission("invalid"), {
    ok: false,
    reason: "desktop_notification_permission_invalid",
  });
});

test("granted permission only emits fixed redacted copy and preserves replay fences", () => {
  FakeNotification.calls = [];
  const adapter = new DesktopNotificationAdapter({
    workspaceBindingDigest: binding,
    permission: "granted",
    notifier: FakeNotification,
  });
  const result = adapter.accept(fact());
  assert.equal(result.disposition, "shown");
  assert.equal(FakeNotification.calls.length, 1);
  assert.match(FakeNotification.calls[0].body, /回执/);
  assert.doesNotMatch(FakeNotification.calls[0].body, /workspace|token|secret|nm16/);
  assert.equal(adapter.accept(fact()).disposition, "duplicate");
  assert.equal(
    adapter.accept(fact({ workspace_binding_digest: "b".repeat(64) })).reason,
    "desktop_notification_workspace_mismatch"
  );
  assert.equal(
    adapter.accept({ ...fact({ notification_id: "terminal:nm16:epoch:3" }), private_payload: "secret" }).reason,
    "desktop_notification_unknown_field"
  );
});

test("notifier failure remains unknown and does not become delivered", () => {
  const adapter = new DesktopNotificationAdapter({
    workspaceBindingDigest: binding,
    permission: "granted",
    notifier: class {
      constructor() { throw new Error("permission or OS failure"); }
    },
  });
  assert.equal(adapter.accept(fact({ notification_id: "terminal:nm16:epoch:4" })).disposition, "unknown");
});

test("NM-16 source keeps notification and lifecycle effects behind explicit boundaries", () => {
  const main = fs.readFileSync(path.join(__dirname, "..", "main.js"), "utf8");
  const notifications = fs.readFileSync(path.join(__dirname, "..", "lib", "notifications.js"), "utf8");
  const persistence = fs.readFileSync(path.join(__dirname, "..", "lib", "desktop-persistence.js"), "utf8");
  const closePolicy = fs.readFileSync(path.join(__dirname, "..", "lib", "close-policy.js"), "utf8");
  for (const marker of [
    "DesktopNotificationAdapter",
    "resolveNotificationPermission",
    "Notification.permission",
    "permission_denied",
    "desktop_notification_permission_invalid",
    "Kiana 回合完成",
    "workspace_binding_digest",
    "closePrompt",
    "reattachPlan",
    "detached",
  ]) {
    assert.ok(main.includes(marker) || notifications.includes(marker) || persistence.includes(marker) || closePolicy.includes(marker), marker);
  }
  const notify = main
    .split("function notifyServerFact")
    [1]
    .split("function installWindowSecurity")[0];
  assert.doesNotMatch(notify, /cancel_envelope|resume_envelope|approve|trust/);
  assert.doesNotMatch(notifications, /fact\.(body|title|path|token|secret)/);
  assert.doesNotMatch(notifications, /ipcRenderer|child_process|spawn\(/);
  assert.doesNotMatch(closePolicy, /cancel_envelope|resume_envelope|approve/);
});

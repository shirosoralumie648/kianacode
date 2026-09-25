"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { pathToFileURL } = require("node:url");
const {
  CHANNEL_INTENTS,
  HANDSHAKE_CHANNEL,
  IPC_CHANNELS,
  IPC_PROTOCOL_VERSION,
  INTENT_PROTOCOL_VERSION,
  classifyNavigation,
  createIpcSession,
  validateHandshake,
  validateIpcRequest,
} = require("../lib/ipc-security");

const fixture = JSON.parse(
  fs.readFileSync(path.join(__dirname, "fixtures", "ui24-ipc.json"), "utf8")
);

test("ui24 fixture keeps trusted loopback and welcome navigation bounded", () => {
  const welcomeUrl = "file:///opt/kiana/contrib/desktop/welcome.html";
  const trustedOrigin = "http://127.0.0.1:4321";
  assert.equal(fixture.protocol_version, IPC_PROTOCOL_VERSION);
  assert.equal(fixture.intent_version, INTENT_PROTOCOL_VERSION);
  for (const url of fixture.trusted) {
    const decision = classifyNavigation(url, { trustedOrigin, welcomeUrl });
    assert.equal(decision.action, "allow-loopback", url);
  }
  for (const url of fixture.external_browser) {
    const decision = classifyNavigation(url, { trustedOrigin, welcomeUrl });
    assert.equal(decision.action, "external", url);
  }
  for (const url of fixture.denied) {
    const decision = classifyNavigation(url, { trustedOrigin, welcomeUrl });
    assert.equal(decision.action, "deny", `${url}: ${decision.reason}`);
  }
});

test("ui24 IPC handshake and intents bind sender, origin, workspace and nonce", () => {
  const sender = {};
  const event = {
    sender,
    senderFrame: { url: "http://127.0.0.1:4321/", parent: null },
  };
  const session = createIpcSession({
    origin: "http://127.0.0.1:4321",
    workspaceBinding: "workspace.binding.opaque",
  });
  const handshake = validateHandshake({
    event,
    expectedSender: sender,
    session,
    welcomeUrl: "file:///opt/kiana/contrib/desktop/welcome.html",
    payload: { version: IPC_PROTOCOL_VERSION },
  });
  assert.equal(handshake.ok, true);
  assert.equal(handshake.value.version, IPC_PROTOCOL_VERSION);
  assert.equal(handshake.value.workspace_binding, "workspace.binding.opaque");
  assert.equal(Object.hasOwn(handshake.value, "web_token"), false);

  const envelope = {
    version: IPC_PROTOCOL_VERSION,
    instance_token: handshake.value.instance_token,
    nonce: 1,
    workspace_binding: handshake.value.workspace_binding,
  };
  const accepted = validateIpcRequest({
    event,
    expectedSender: sender,
    session,
    welcomeUrl: "file:///opt/kiana/contrib/desktop/welcome.html",
    channel: IPC_CHANNELS.state,
    envelope,
  });
  assert.equal(accepted.ok, true);
  assert.equal(accepted.intent.version, INTENT_PROTOCOL_VERSION);
  assert.equal(accepted.intent.name, CHANNEL_INTENTS[IPC_CHANNELS.state]);
  assert.equal(accepted.intent.workspace_binding_digest.length, 64);
  assert.equal(Object.hasOwn(accepted.intent, "instance_token"), false);

  const replay = validateIpcRequest({
    event,
    expectedSender: sender,
    session,
    welcomeUrl: "file:///opt/kiana/contrib/desktop/welcome.html",
    channel: IPC_CHANNELS.state,
    envelope,
  });
  assert.deepEqual(replay, { ok: false, reason: "nonce_mismatch" });
});

test("ui24 IPC denies foreign senders, nested frames, forged bindings and channels", () => {
  const sender = {};
  const session = createIpcSession({
    origin: "http://127.0.0.1:4321",
    workspaceBinding: "workspace.binding.opaque",
  });
  const makeEvent = (overrides = {}) => ({
    sender,
    senderFrame: { url: "http://127.0.0.1:4321/", parent: null },
    ...overrides,
  });
  const base = {
    version: IPC_PROTOCOL_VERSION,
    instance_token: session.instanceToken,
    nonce: 1,
    workspace_binding: session.workspaceBinding,
  };
  const check = (event, envelope, channel = IPC_CHANNELS.state) =>
    validateIpcRequest({
      event,
      expectedSender: sender,
      session,
      welcomeUrl: "file:///opt/kiana/contrib/desktop/welcome.html",
      channel,
      envelope,
    });
  assert.equal(check(makeEvent({ sender: {} }), base).reason, "unknown_sender");
  assert.equal(check({ sender }, base).reason, "missing_sender_frame");
  assert.equal(
    check(makeEvent({ senderFrame: { url: "http://127.0.0.1:4321/", parent: {} } }), base).reason,
    "nested_sender_frame"
  );
  assert.equal(
    check(makeEvent({ senderFrame: { url: "http://127.0.0.1:4322/", parent: null } }), base).reason,
    "sender_origin_mismatch"
  );
  assert.equal(
    check(makeEvent(), { ...base, instance_token: "forged" }).reason,
    "instance_token_mismatch"
  );
  assert.equal(
    check(makeEvent(), { ...base, workspace_binding: "other" }).reason,
    "workspace_binding_mismatch"
  );
  assert.equal(check(makeEvent(), base, "workspace:arbitrary").reason, "unknown_channel");
  assert.deepEqual(
    validateHandshake({
      event: makeEvent(),
      expectedSender: sender,
      session,
      welcomeUrl: "file:///opt/kiana/contrib/desktop/welcome.html",
      payload: { version: "kiana.desktop.ipc.v0" },
    }),
    { ok: false, reason: "protocol_version_mismatch" }
  );
  assert.deepEqual(
    validateHandshake({
      event: makeEvent(),
      expectedSender: sender,
      session,
      welcomeUrl: "file:///opt/kiana/contrib/desktop/welcome.html",
      payload: { version: IPC_PROTOCOL_VERSION, extra: "ignored" },
    }),
    { ok: false, reason: "protocol_version_mismatch" }
  );
  assert.equal(check(makeEvent(), { ...base, extra: "ignored" }).reason, "unexpected_envelope_field");
});

test("ui24 source guard keeps Electron security flags and token-free renderer boundary", () => {
  const main = fs.readFileSync(path.join(__dirname, "..", "main.js"), "utf8");
  const preload = fs.readFileSync(path.join(__dirname, "..", "preload.js"), "utf8");
  const security = fs.readFileSync(path.join(__dirname, "..", "lib", "ipc-security.js"), "utf8");
  const packageJson = JSON.parse(fs.readFileSync(path.join(__dirname, "..", "package.json"), "utf8"));
  for (const marker of [
    "desktop:handshake",
    "senderFrame",
    "workspace_binding",
    "validateIpcRequest",
    "kiana.desktop.intent.v1",
    "setWindowOpenHandler",
    "will-navigate",
    "shell.openExternal",
    "contextIsolation: true",
    "nodeIntegration: false",
    "sandbox: true",
    "webSecurity: true",
    "allowRunningInsecureContent: false",
  ]) {
    assert.ok(main.includes(marker) || preload.includes(marker) || security.includes(marker), marker);
  }
  assert.match(main, /ipcMain\.handle\(HANDSHAKE_CHANNEL/);
  assert.match(main, /validateIpcRequest\(/);
  assert.match(main, /action: "deny"/);
  assert.match(security, /workspace_binding_digest/);
  assert.equal(/ipcMain\.handle\("workspace:[^"]+", \(\) =>/.test(main), false);
  assert.equal(/web_token|x-kiana-web-token|token=/i.test(main), false);
  assert.equal(/web_token|x-kiana-web-token|token=/i.test(preload), false);
  assert.equal(packageJson.main, "main.js");
  assert.equal(packageJson.build.files.includes("lib/**/*"), true);
});

test("ui24 fixture names every deny-first sender reason", () => {
  for (const reason of fixture.sender_rejections) {
    assert.match(reason, /^[a-z][a-z_]+$/);
  }
  assert.ok(fixture.channels.includes(HANDSHAKE_CHANNEL));
  for (const channel of Object.values(IPC_CHANNELS)) {
    assert.ok(fixture.channels.includes(channel), channel);
  }
});

test("ui24 welcome URL remains a local file path with no query credential", () => {
  const local = pathToFileURL(path.join(__dirname, "..", "welcome.html")).href;
  assert.equal(new URL(local).protocol, "file:");
  assert.equal(new URL(local).search, "");
  assert.equal(new URL(local).hash, "");
});

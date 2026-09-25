"use strict";

const crypto = require("node:crypto");

// The Electron shell is a client of the Web/DaemonHost path.  These values are
// deliberately separate from the Web bearer credential: they are short-lived
// process-local binding material used only to authenticate this BrowserWindow.
const IPC_PROTOCOL_VERSION = "kiana.desktop.ipc.v1";
const INTENT_PROTOCOL_VERSION = "kiana.desktop.intent.v1";
const HANDSHAKE_CHANNEL = "desktop:handshake";

const IPC_CHANNELS = Object.freeze({
  state: "workspace:state",
  open: "workspace:open",
  newProject: "workspace:new",
  continue: "workspace:continue",
  notification: "desktop:notification",
});

const CHANNEL_INTENTS = Object.freeze({
  [IPC_CHANNELS.state]: "desktop.workspace.state.v1",
  [IPC_CHANNELS.open]: "desktop.workspace.open.v1",
  [IPC_CHANNELS.newProject]: "desktop.workspace.new.v1",
  [IPC_CHANNELS.continue]: "desktop.workspace.continue.v1",
  [IPC_CHANNELS.notification]: "desktop.notification.server-fact.v1",
});

const ALLOWED_CHANNELS = new Set([
  HANDSHAKE_CHANNEL,
  ...Object.values(IPC_CHANNELS),
]);

const SAFE_EXTERNAL_SCHEMES = new Set(["http:", "https:"]);
const LOOPBACK_HOSTS = new Set(["127.0.0.1", "localhost", "::1", "[::1]"]);
const SENSITIVE_QUERY_KEYS = new Set([
  "token",
  "web_token",
  "x-kiana-web-token",
  "workspace",
  "workdir",
  "project",
]);

function opaqueToken(label) {
  const prefix = String(label || "binding").replace(/[^a-z0-9]/gi, "").slice(0, 16) || "binding";
  return `${prefix}.${crypto.randomBytes(24).toString("hex")}`;
}

function digestBinding(value) {
  if (value === null || value === undefined) {
    return null;
  }
  return crypto.createHash("sha256").update(String(value), "utf8").digest("hex");
}

function createIpcSession({ origin, workspaceBinding = null } = {}) {
  if (!origin || typeof origin !== "string") {
    throw new TypeError("ipc_origin_required");
  }
  if (workspaceBinding !== null && typeof workspaceBinding !== "string") {
    throw new TypeError("ipc_workspace_binding_invalid");
  }
  return {
    protocolVersion: IPC_PROTOCOL_VERSION,
    instanceToken: opaqueToken("instance"),
    nonce: 0,
    origin,
    workspaceBinding,
    workspaceBindingDigest: digestBinding(workspaceBinding),
  };
}

function secureEqual(left, right) {
  if (typeof left !== "string" || typeof right !== "string") {
    return false;
  }
  const a = Buffer.from(left, "utf8");
  const b = Buffer.from(right, "utf8");
  return a.length === b.length && crypto.timingSafeEqual(a, b);
}

function parseUrl(rawUrl) {
  if (typeof rawUrl !== "string" || rawUrl.length === 0 || rawUrl.length > 4096 || rawUrl.includes("\0")) {
    return null;
  }
  try {
    return new URL(rawUrl);
  } catch {
    return null;
  }
}

function canonicalUrl(url) {
  const parsed = parseUrl(url);
  return parsed ? parsed.href : null;
}

function normalizedHostname(hostname) {
  return String(hostname || "").toLowerCase().replace(/^\[|\]$/g, "");
}

function isLoopbackHost(hostname) {
  const normalized = normalizedHostname(hostname);
  return LOOPBACK_HOSTS.has(normalized) || normalized === "::1";
}

function hasSensitiveUrlMaterial(url) {
  if (!url) {
    return true;
  }
  for (const key of url.searchParams.keys()) {
    if (SENSITIVE_QUERY_KEYS.has(String(key).toLowerCase())) {
      return true;
    }
  }
  // No fragment is needed by the desktop shell.  Rejecting it keeps opaque
  // bearer material from being copied into a renderer-visible URL.
  return Boolean(url.hash);
}

function isTrustedLoopbackUrl(rawUrl, trustedOrigin) {
  const url = parseUrl(rawUrl);
  if (!url || typeof trustedOrigin !== "string") {
    return false;
  }
  const expected = parseUrl(trustedOrigin);
  if (!expected || !["http:", "https:"].includes(expected.protocol)) {
    return false;
  }
  return (
    url.protocol === expected.protocol &&
    url.origin === expected.origin &&
    isLoopbackHost(url.hostname) &&
    url.username === "" &&
    url.password === "" &&
    !hasSensitiveUrlMaterial(url)
  );
}

function isWelcomeUrl(rawUrl, welcomeUrl) {
  const actual = canonicalUrl(rawUrl);
  const expected = canonicalUrl(welcomeUrl);
  if (!actual || !expected) {
    return false;
  }
  const actualUrl = parseUrl(actual);
  const expectedUrl = parseUrl(expected);
  return (
    actualUrl.protocol === "file:" &&
    expectedUrl.protocol === "file:" &&
    actual === expected
  );
}

/**
 * Classify a browser navigation before Electron performs it.
 *
 * `allow-loopback` is consumed only by the existing BrowserWindow.  `external`
 * is a request to open an explicitly clicked safe HTTP(S) URL in the user's
 * external browser; it is never loaded in the renderer.  Every other value is
 * denied, including file/javascript/data and a loopback URL for another
 * instance/port.
 */
function classifyNavigation(rawUrl, { trustedOrigin, welcomeUrl } = {}) {
  const url = parseUrl(rawUrl);
  if (!url) {
    return { action: "deny", reason: "malformed_url" };
  }
  if (isWelcomeUrl(url.href, welcomeUrl)) {
    return { action: "allow-loopback", reason: "trusted_welcome" };
  }
  if (isTrustedLoopbackUrl(url.href, trustedOrigin)) {
    return { action: "allow-loopback", reason: "trusted_loopback" };
  }
  if (
    SAFE_EXTERNAL_SCHEMES.has(url.protocol) &&
    !isLoopbackHost(url.hostname) &&
    url.username === "" &&
    url.password === "" &&
    !hasSensitiveUrlMaterial(url)
  ) {
    return { action: "external", reason: "explicit_external", url: url.href };
  }
  if (!["http:", "https:", "file:"].includes(url.protocol)) {
    return { action: "deny", reason: "unsafe_scheme" };
  }
  if (isLoopbackHost(url.hostname)) {
    return { action: "deny", reason: "untrusted_loopback" };
  }
  return { action: "deny", reason: "navigation_not_allowlisted" };
}

function validateSenderFrame({ event, expectedSender, expectedOrigin, welcomeUrl } = {}) {
  if (!event || !expectedSender || event.sender !== expectedSender) {
    return { ok: false, reason: "unknown_sender" };
  }
  const frame = event.senderFrame;
  if (!frame || typeof frame.url !== "string") {
    return { ok: false, reason: "missing_sender_frame" };
  }
  // Only the top frame can address the desktop bridge.  A nested frame may
  // share the loopback origin while remaining an untrusted document.
  if (frame.parent) {
    return { ok: false, reason: "nested_sender_frame" };
  }
  const decision = classifyNavigation(frame.url, {
    trustedOrigin: expectedOrigin,
    welcomeUrl,
  });
  if (decision.action !== "allow-loopback") {
    return { ok: false, reason: "sender_origin_mismatch" };
  }
  return { ok: true };
}

function validateHandshake({ event, expectedSender, session, welcomeUrl, payload } = {}) {
  const sender = validateSenderFrame({
    event,
    expectedSender,
    expectedOrigin: session && session.origin,
    welcomeUrl,
  });
  if (!sender.ok) {
    return sender;
  }
  if (!payload || payload.version !== IPC_PROTOCOL_VERSION) {
    return { ok: false, reason: "protocol_version_mismatch" };
  }
  return {
    ok: true,
    value: {
      version: IPC_PROTOCOL_VERSION,
      instance_token: session.instanceToken,
      nonce: session.nonce,
      workspace_binding: session.workspaceBinding,
    },
  };
}

function makeIntent(channel, session, nonce) {
  return Object.freeze({
    version: INTENT_PROTOCOL_VERSION,
    name: CHANNEL_INTENTS[channel],
    channel,
    nonce,
    workspace_binding_digest: session.workspaceBindingDigest,
  });
}

function validateNotificationEnvelope(fact, session) {
  if (!fact || typeof fact !== "object" || Array.isArray(fact)) {
    return { ok: false, reason: "desktop_notification_fact_invalid" };
  }
  if (fact.schema !== "kiana.desktop-notification.v1" || fact.source !== "server") {
    return { ok: false, reason: "desktop_notification_schema_mismatch" };
  }
  const knownKeys = new Set([
    "schema", "source", "kind", "notification_id", "feed_epoch", "sequence", "status",
    "workspace_binding_digest",
  ]);
  if (Object.keys(fact).some(key => !knownKeys.has(key))) {
    return { ok: false, reason: "desktop_notification_unknown_field" };
  }
  if (fact.workspace_binding_digest !== session.workspaceBindingDigest) {
    return { ok: false, reason: "desktop_notification_workspace_mismatch" };
  }
  if (typeof fact.workspace_binding_digest !== "string" ||
      !/^[a-f0-9]{64}$/.test(fact.workspace_binding_digest)) {
    return { ok: false, reason: "desktop_notification_workspace_mismatch" };
  }
  if (typeof fact.notification_id !== "string" ||
      !/^[A-Za-z0-9:_-]{1,256}$/.test(fact.notification_id)) {
    return { ok: false, reason: "desktop_notification_id_invalid" };
  }
  if (typeof fact.feed_epoch !== "string" ||
      fact.feed_epoch.length === 0 || fact.feed_epoch.length > 256) {
    return { ok: false, reason: "desktop_notification_epoch_invalid" };
  }
  if (!Number.isSafeInteger(fact.sequence) || fact.sequence < 1) {
    return { ok: false, reason: "desktop_notification_sequence_invalid" };
  }
  if (fact.kind === "approval" && fact.status !== "pending") {
    return { ok: false, reason: "desktop_notification_status_invalid" };
  }
  if (fact.kind === "terminal" &&
      !["completed", "cancelled", "failed", "unknown", "result_unknown"].includes(fact.status)) {
    return { ok: false, reason: "desktop_notification_status_invalid" };
  }
  if (fact.kind !== "approval" && fact.kind !== "terminal") {
    return { ok: false, reason: "desktop_notification_kind_invalid" };
  }
  return { ok: true };
}

function validateIpcRequest({ event, expectedSender, session, welcomeUrl, channel, envelope } = {}) {
  if (!ALLOWED_CHANNELS.has(channel) || channel === HANDSHAKE_CHANNEL || !CHANNEL_INTENTS[channel]) {
    return { ok: false, reason: "unknown_channel" };
  }
  const sender = validateSenderFrame({
    event,
    expectedSender,
    expectedOrigin: session && session.origin,
    welcomeUrl,
  });
  if (!sender.ok) {
    return sender;
  }
  if (!envelope || typeof envelope !== "object" || Array.isArray(envelope)) {
    return { ok: false, reason: "malformed_envelope" };
  }
  if (envelope.version !== IPC_PROTOCOL_VERSION) {
    return { ok: false, reason: "protocol_version_mismatch" };
  }
  if (!secureEqual(envelope.instance_token, session.instanceToken)) {
    return { ok: false, reason: "instance_token_mismatch" };
  }
  if (envelope.workspace_binding !== session.workspaceBinding) {
    return { ok: false, reason: "workspace_binding_mismatch" };
  }
  if (!Number.isSafeInteger(envelope.nonce) || envelope.nonce !== session.nonce + 1) {
    return { ok: false, reason: "nonce_mismatch" };
  }
  if (channel === IPC_CHANNELS.notification) {
    const notification = validateNotificationEnvelope(envelope.fact, session);
    if (!notification.ok) return notification;
  } else if (Object.hasOwn(envelope, "fact")) {
    return { ok: false, reason: "unexpected_intent_payload" };
  }
  session.nonce = envelope.nonce;
  const intent = channel === IPC_CHANNELS.notification
    ? Object.freeze({
        ...makeIntent(channel, session, envelope.nonce),
        fact: Object.freeze({ ...envelope.fact }),
      })
    : makeIntent(channel, session, envelope.nonce);
  return { ok: true, intent };
}

module.exports = {
  ALLOWED_CHANNELS,
  CHANNEL_INTENTS,
  HANDSHAKE_CHANNEL,
  INTENT_PROTOCOL_VERSION,
  IPC_CHANNELS,
  IPC_PROTOCOL_VERSION,
  classifyNavigation,
  createIpcSession,
  digestBinding,
  hasSensitiveUrlMaterial,
  isLoopbackHost,
  isTrustedLoopbackUrl,
  makeIntent,
  opaqueToken,
  validateHandshake,
  validateNotificationEnvelope,
  validateIpcRequest,
  validateSenderFrame,
};

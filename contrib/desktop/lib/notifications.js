"use strict";

const NOTIFICATION_SCHEMA = "kiana.desktop-notification.v1";
const MAX_FEED_EPOCH_BYTES = 256;
const MAX_NOTIFICATION_ID_BYTES = 256;
const DIGEST_PATTERN = /^[a-f0-9]{64}$/;
const OPAQUE_ID_PATTERN = /^[A-Za-z0-9:_-]+$/;
const FACT_KEYS = new Set([
  "schema",
  "source",
  "kind",
  "notification_id",
  "feed_epoch",
  "sequence",
  "status",
  "workspace_binding_digest",
]);
const TERMINAL_STATUSES = new Set([
  "completed",
  "cancelled",
  "failed",
  "unknown",
  "result_unknown",
]);
const DESKTOP_NOTIFICATION_PERMISSION_STATES = Object.freeze([
  "granted",
  "denied",
  "unknown",
  "unavailable",
]);

function reject(reason) {
  return { ok: false, reason };
}

function validateServerNotificationFact(fact, { workspaceBindingDigest } = {}) {
  if (!fact || typeof fact !== "object" || Array.isArray(fact)) {
    return reject("desktop_notification_fact_invalid");
  }
  if (fact.schema !== NOTIFICATION_SCHEMA || fact.source !== "server") {
    return reject("desktop_notification_schema_mismatch");
  }
  if (Object.keys(fact).some(key => !FACT_KEYS.has(key))) {
    return reject("desktop_notification_unknown_field");
  }
  if (!DIGEST_PATTERN.test(fact.workspace_binding_digest) ||
      fact.workspace_binding_digest !== workspaceBindingDigest) {
    return reject("desktop_notification_workspace_mismatch");
  }
  if (typeof fact.notification_id !== "string" ||
      Buffer.byteLength(fact.notification_id, "utf8") === 0 ||
      Buffer.byteLength(fact.notification_id, "utf8") > MAX_NOTIFICATION_ID_BYTES ||
      !OPAQUE_ID_PATTERN.test(fact.notification_id)) {
    return reject("desktop_notification_id_invalid");
  }
  if (typeof fact.feed_epoch !== "string" ||
      fact.feed_epoch.length === 0 ||
      Buffer.byteLength(fact.feed_epoch, "utf8") > MAX_FEED_EPOCH_BYTES) {
    return reject("desktop_notification_epoch_invalid");
  }
  if (!Number.isSafeInteger(fact.sequence) || fact.sequence < 1) {
    return reject("desktop_notification_sequence_invalid");
  }
  if (fact.kind === "approval") {
    if (fact.status !== "pending") return reject("desktop_notification_status_invalid");
  } else if (fact.kind === "terminal") {
    if (!TERMINAL_STATUSES.has(fact.status)) return reject("desktop_notification_status_invalid");
  } else {
    return reject("desktop_notification_kind_invalid");
  }
  return { ok: true };
}

function descriptorForServerFact(fact, options) {
  const result = validateServerNotificationFact(fact, options);
  if (!result.ok) return result;
  if (fact.kind === "approval") {
    return {
      ok: true,
      value: {
        notification_id: fact.notification_id,
        title: "Kiana 需要你的决定",
        body: "有一个待处理的服务端动作。请在工作台中查看。",
      },
    };
  }
  if (fact.status === "completed") {
    return {
      ok: true,
      value: {
        notification_id: fact.notification_id,
        title: "Kiana 回合完成",
        body: "服务端已记录终态。请在工作台中查看回执。",
      },
    };
  }
  if (fact.status === "unknown" || fact.status === "result_unknown") {
    return {
      ok: true,
      value: {
        notification_id: fact.notification_id,
        title: "Kiana 有一个未知结果",
        body: "结果尚未确认。请在工作台中查询原始回执。",
      },
    };
  }
  return {
    ok: true,
    value: {
      notification_id: fact.notification_id,
      title: "Kiana 回合已停止",
      body: "服务端已记录非成功终态。请在工作台中查看详情。",
    },
  };
}

class NotificationBridge {
  constructor({ workspaceBindingDigest, maxSeen = 256 } = {}) {
    this.workspaceBindingDigest = workspaceBindingDigest;
    this.maxSeen = maxSeen;
    this.seen = new Map();
  }

  setWorkspaceBindingDigest(workspaceBindingDigest) {
    this.workspaceBindingDigest = workspaceBindingDigest;
    this.seen.clear();
  }

  accept(fact) {
    const descriptor = descriptorForServerFact(fact, {
      workspaceBindingDigest: this.workspaceBindingDigest,
    });
    if (!descriptor.ok) return descriptor;
    const fingerprint = [
      fact.kind,
      fact.status,
      fact.feed_epoch,
      fact.sequence,
      fact.workspace_binding_digest,
    ].join(":");
    const previous = this.seen.get(descriptor.value.notification_id);
    if (previous !== undefined && previous !== fingerprint) {
      return { ok: false, reason: "desktop_notification_replay_conflict" };
    }
    if (previous !== undefined) {
      return { ok: true, disposition: "duplicate" };
    }
    this.seen.set(descriptor.value.notification_id, fingerprint);
    while (this.seen.size > this.maxSeen) {
      this.seen.delete(this.seen.keys().next().value);
    }
    return { ok: true, disposition: "new", value: descriptor.value };
  }
}

class DesktopNotificationAdapter {
  constructor({
    bridge = undefined,
    workspaceBindingDigest = null,
    maxSeen = 256,
    permission = "unknown",
    notifier = null,
  } = {}) {
    this.bridge = bridge || new NotificationBridge({ workspaceBindingDigest, maxSeen });
    this.permission = DESKTOP_NOTIFICATION_PERMISSION_STATES.includes(permission)
      ? permission
      : "unknown";
    this.notifier = notifier;
  }

  setWorkspaceBindingDigest(workspaceBindingDigest) {
    this.bridge.setWorkspaceBindingDigest(workspaceBindingDigest);
  }

  setPermission(permission) {
    if (!DESKTOP_NOTIFICATION_PERMISSION_STATES.includes(permission)) {
      return { ok: false, reason: "desktop_notification_permission_invalid" };
    }
    this.permission = permission;
    return { ok: true, state: permission };
  }

  permissionState() {
    return this.permission;
  }

  accept(fact) {
    const result = this.bridge.accept(fact);
    if (!result.ok || result.disposition === "duplicate") {
      return result;
    }
    if (this.permission !== "granted") {
      return {
        ok: true,
        disposition: this.permission === "denied" ? "permission_denied" : "unavailable",
        value: result.value,
      };
    }
    if (typeof this.notifier !== "function") {
      return { ok: true, disposition: "unavailable", value: result.value };
    }
    try {
      const notification = new this.notifier({
        title: result.value.title,
        body: result.value.body,
      });
      if (!notification || typeof notification.show !== "function") {
        return { ok: true, disposition: "unavailable", value: result.value };
      }
      notification.show();
      return { ok: true, disposition: "shown", value: result.value };
    } catch {
      return { ok: true, disposition: "unknown", value: result.value };
    }
  }
}

module.exports = {
  DESKTOP_NOTIFICATION_PERMISSION_STATES,
  DesktopNotificationAdapter,
  NOTIFICATION_SCHEMA,
  NotificationBridge,
  descriptorForServerFact,
  validateServerNotificationFact,
};

"use strict";

const DESKTOP_STATE_SCHEMA = "kiana.desktop-state.v1";
const MAX_WORKSPACE_BYTES = 4096;
const MAX_NOTIFICATION_IDS = 256;
const MAX_NOTIFICATION_ID_BYTES = 256;

function boundedString(value, maxBytes, reason) {
  if (typeof value !== "string" || value.length === 0 || Buffer.byteLength(value, "utf8") > maxBytes) {
    throw new Error(reason);
  }
  return value;
}

function cloneObserved(observed) {
  return {
    pending_count: observed.pending_count,
    unknown_count: observed.unknown_count,
  };
}

function freezeState(state) {
  Object.freeze(state.observed);
  Object.freeze(state.seen_notification_ids);
  return Object.freeze(state);
}

function createDesktopState() {
  return freezeState({
    schema: DESKTOP_STATE_SCHEMA,
    workspace: null,
    lifecycle: "stopped",
    instance: null,
    observed: {
      pending_count: 0,
      unknown_count: 0,
    },
    seen_notification_ids: [],
    draft_dirty: false,
  });
}

function isNotificationFact(fact) {
  return Boolean(
    fact &&
    fact.schema === "kiana.desktop-notification.v1" &&
    fact.source === "server" &&
    typeof fact.notification_id === "string" &&
    fact.notification_id.length > 0 &&
    Buffer.byteLength(fact.notification_id, "utf8") <= MAX_NOTIFICATION_ID_BYTES &&
    (fact.kind === "approval" || fact.kind === "terminal")
  );
}

function reduceDesktopState(previous, event) {
  if (!previous || previous.schema !== DESKTOP_STATE_SCHEMA) {
    throw new Error("desktop_state_invalid");
  }
  if (!event || typeof event.type !== "string") {
    throw new Error("desktop_state_event_invalid");
  }

  const next = {
    ...previous,
    observed: cloneObserved(previous.observed),
    seen_notification_ids: [...previous.seen_notification_ids],
  };

  switch (event.type) {
    case "workspace_requested":
      next.workspace = event.workspace === null
        ? null
        : boundedString(event.workspace, MAX_WORKSPACE_BYTES, "desktop_workspace_invalid");
      next.lifecycle = "starting";
      next.instance = null;
      next.observed = { pending_count: 0, unknown_count: 0 };
      next.seen_notification_ids = [];
      break;
    case "worker_ready":
      next.lifecycle = "ready";
      next.instance = {
        id: boundedString(event.instance_id, 256, "desktop_instance_invalid"),
        epoch: boundedString(event.epoch, 256, "desktop_epoch_invalid"),
      };
      break;
    case "worker_stopping":
      next.lifecycle = "stopping";
      break;
    case "worker_stopped":
      next.lifecycle = "stopped";
      next.instance = null;
      break;
    case "worker_failed":
      next.lifecycle = event.unconfirmed ? "unconfirmed" : "failed";
      next.instance = null;
      break;
    case "draft_changed":
      next.draft_dirty = Boolean(event.dirty);
      break;
    case "server_fact": {
      const fact = event.fact;
      if (!isNotificationFact(fact)) {
        throw new Error("desktop_server_fact_invalid");
      }
      if (next.seen_notification_ids.includes(fact.notification_id)) {
        break;
      }
      next.seen_notification_ids.push(fact.notification_id);
      if (next.seen_notification_ids.length > MAX_NOTIFICATION_IDS) {
        next.seen_notification_ids.shift();
      }
      if (fact.kind === "approval") {
        next.observed.pending_count += 1;
      } else if (["unknown", "result_unknown"].includes(fact.status)) {
        next.observed.unknown_count += 1;
      }
      break;
    }
    default:
      throw new Error("desktop_state_event_unknown");
  }

  return freezeState(next);
}

function closeAttention(state) {
  if (!state || state.schema !== DESKTOP_STATE_SCHEMA) {
    throw new Error("desktop_state_invalid");
  }
  return Object.freeze({
    pending_count: state.observed.pending_count,
    unknown_count: state.observed.unknown_count,
    draft_dirty: state.draft_dirty,
    requires_attention: state.observed.pending_count > 0 || state.observed.unknown_count > 0,
  });
}

module.exports = {
  DESKTOP_STATE_SCHEMA,
  closeAttention,
  createDesktopState,
  isNotificationFact,
  reduceDesktopState,
};

"use strict";

const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

const DESKTOP_STORE_SCHEMA = "kiana.desktop-store.v1";
const REFERENCE_SCHEMA = "kiana.desktop-session-reference.v1";
const REATTACH_SCHEMA = "kiana.desktop-reattach-plan.v1";
const MAX_STORE_BYTES = 64 * 1024;
const MAX_PATH_BYTES = 4096;
const MAX_ID_BYTES = 256;
const MAX_CURSOR_EPOCH_BYTES = 256;
const DIGEST_PATTERN = /^[a-f0-9]{64}$/;
const OPAQUE_ID_PATTERN = /^[A-Za-z0-9:_-]{1,256}$/;
const DRAFT_POLICIES = new Set(["discard", "restore_prompt"]);
const SENSITIVE_KEY_PATTERN = /(token|secret|password|credential|authorization|bearer|private[_-]?key|access[_-]?key|refresh)/i;

function plain(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function boundedString(value, maxBytes, reason) {
  if (typeof value !== "string" || value.length === 0 || Buffer.byteLength(value, "utf8") > maxBytes) {
    throw new Error(reason);
  }
  return value;
}

function assertKeys(value, allowed, reason) {
  if (!plain(value) || Object.keys(value).some(key => !allowed.has(key))) {
    throw new Error(reason);
  }
}

function assertNoSensitiveKeys(value) {
  if (Array.isArray(value)) {
    value.forEach(assertNoSensitiveKeys);
    return;
  }
  if (!plain(value)) return;
  for (const [key, nested] of Object.entries(value)) {
    if (SENSITIVE_KEY_PATTERN.test(key)) throw new Error("desktop_store_sensitive_field");
    assertNoSensitiveKeys(nested);
  }
}

function digestWorkspace(workspacePath) {
  const canonical = canonicalWorkspacePath(workspacePath);
  if (canonical.includes("\0")) throw new Error("desktop_workspace_path_invalid");
  return crypto.createHash("sha256").update(canonical, "utf8").digest("hex");
}

function canonicalWorkspacePath(workspacePath) {
  const absolute = path.resolve(boundedString(workspacePath, MAX_PATH_BYTES, "desktop_workspace_path_invalid"));
  try {
    const stat = fs.lstatSync(absolute);
    if (stat.isSymbolicLink()) throw new Error("desktop_workspace_symlink");
    return fs.realpathSync(absolute);
  } catch (error) {
    if (error && error.code === "ENOENT") return absolute;
    throw error;
  }
}

function workspaceReference(workspacePath) {
  const canonical = canonicalWorkspacePath(workspacePath);
  return {
    path: canonical,
    digest: digestWorkspace(canonical),
  };
}

function validateCursor(cursor) {
  if (cursor === null) return null;
  assertKeys(cursor, new Set(["epoch", "sequence"]), "desktop_cursor_unknown_field");
  const epoch = boundedString(cursor.epoch, MAX_CURSOR_EPOCH_BYTES, "desktop_cursor_epoch_invalid");
  if (!Number.isSafeInteger(cursor.sequence) || cursor.sequence < 0) {
    throw new Error("desktop_cursor_sequence_invalid");
  }
  return { epoch, sequence: cursor.sequence };
}

function validateInstanceReference(reference) {
  if (reference === null) return null;
  assertKeys(reference, new Set(["instance_id", "authority_epoch", "feed_epoch"]), "desktop_instance_unknown_field");
  return {
    instance_id: boundedString(reference.instance_id, MAX_ID_BYTES, "desktop_instance_id_invalid"),
    authority_epoch: Number.isSafeInteger(reference.authority_epoch) && reference.authority_epoch >= 0
      ? reference.authority_epoch
      : (() => { throw new Error("desktop_authority_epoch_invalid"); })(),
    feed_epoch: boundedString(reference.feed_epoch, MAX_CURSOR_EPOCH_BYTES, "desktop_feed_epoch_invalid"),
  };
}

function validateSessionReference(reference) {
  if (reference === null) return null;
  assertKeys(reference, new Set(["session_id", "scope_digest"]), "desktop_session_unknown_field");
  if (!OPAQUE_ID_PATTERN.test(reference.session_id)) throw new Error("desktop_session_id_invalid");
  if (!DIGEST_PATTERN.test(reference.scope_digest)) throw new Error("desktop_session_scope_invalid");
  return {
    session_id: reference.session_id,
    scope_digest: reference.scope_digest,
  };
}

function validateLayout(layout) {
  if (layout === null) return null;
  assertKeys(layout, new Set(["x", "y", "width", "height"]), "desktop_layout_unknown_field");
  for (const key of ["x", "y", "width", "height"]) {
    if (!Number.isSafeInteger(layout[key])) throw new Error("desktop_layout_dimension_invalid");
  }
  if (layout.width < 320 || layout.width > 8192 || layout.height < 240 || layout.height > 8192) {
    throw new Error("desktop_layout_bounds_invalid");
  }
  return { x: layout.x, y: layout.y, width: layout.width, height: layout.height };
}

function emptyDesktopStore() {
  return {
    schema: DESKTOP_STORE_SCHEMA,
    revision: 1,
    layout: null,
    draft_policy: "restore_prompt",
    workspace_ref: null,
    instance_ref: null,
    session_ref: null,
    last_cursor: null,
    detached: true,
  };
}

function validateDesktopStore(input) {
  if (!plain(input)) throw new Error("desktop_store_invalid");
  assertKeys(input, new Set([
    "schema", "revision", "layout", "draft_policy", "workspace_ref", "instance_ref",
    "session_ref", "last_cursor", "detached",
  ]), "desktop_store_unknown_field");
  if (input.schema !== DESKTOP_STORE_SCHEMA || input.revision !== 1) {
    throw new Error("desktop_store_schema_mismatch");
  }
  if (!DRAFT_POLICIES.has(input.draft_policy)) throw new Error("desktop_draft_policy_invalid");
  if (input.workspace_ref !== null) {
    assertKeys(input.workspace_ref, new Set(["path", "digest"]), "desktop_workspace_ref_unknown_field");
    if (!path.isAbsolute(input.workspace_ref.path) ||
        digestWorkspace(input.workspace_ref.path) !== input.workspace_ref.digest) {
      throw new Error("desktop_workspace_ref_invalid");
    }
  }
  if (typeof input.detached !== "boolean") throw new Error("desktop_detached_invalid");
  assertNoSensitiveKeys(input);
  return {
    schema: DESKTOP_STORE_SCHEMA,
    revision: 1,
    layout: validateLayout(input.layout),
    draft_policy: input.draft_policy,
    workspace_ref: input.workspace_ref === null ? null : workspaceReference(input.workspace_ref.path),
    instance_ref: validateInstanceReference(input.instance_ref),
    session_ref: validateSessionReference(input.session_ref),
    last_cursor: validateCursor(input.last_cursor),
    detached: input.detached,
  };
}

function validateServerReference(reference, workspaceBindingDigest) {
  if (!plain(reference) || reference.schema !== REFERENCE_SCHEMA || reference.source !== "server") {
    throw new Error("desktop_reference_schema_mismatch");
  }
  assertKeys(reference, new Set([
    "schema", "source", "workspace_binding_digest", "session_id", "scope_digest", "cursor", "draft_policy",
  ]), "desktop_reference_unknown_field");
  if (reference.workspace_binding_digest !== workspaceBindingDigest ||
      !DIGEST_PATTERN.test(reference.workspace_binding_digest)) {
    throw new Error("desktop_reference_workspace_mismatch");
  }
  if (!OPAQUE_ID_PATTERN.test(reference.session_id)) throw new Error("desktop_reference_session_invalid");
  if (!DIGEST_PATTERN.test(reference.scope_digest)) throw new Error("desktop_reference_scope_invalid");
  if (!DRAFT_POLICIES.has(reference.draft_policy)) throw new Error("desktop_reference_policy_invalid");
  return {
    session_ref: {
      session_id: reference.session_id,
      scope_digest: reference.scope_digest,
    },
    last_cursor: validateCursor(reference.cursor),
    draft_policy: reference.draft_policy,
  };
}

function mergeDesktopStore(current, patch = {}) {
  const base = validateDesktopStore(current || emptyDesktopStore());
  assertKeys(patch, new Set([
    "layout", "draft_policy", "workspace_path", "instance_ref", "session_ref", "last_cursor",
    "detached", "server_reference",
  ]), "desktop_store_patch_unknown_field");
  assertNoSensitiveKeys(patch);
  const next = { ...base };
  if (Object.hasOwn(patch, "layout")) next.layout = validateLayout(patch.layout);
  if (Object.hasOwn(patch, "draft_policy")) {
    if (!DRAFT_POLICIES.has(patch.draft_policy)) throw new Error("desktop_draft_policy_invalid");
    next.draft_policy = patch.draft_policy;
  }
  if (Object.hasOwn(patch, "workspace_path")) {
    const ref = workspaceReference(patch.workspace_path);
    const changed = !base.workspace_ref || base.workspace_ref.digest !== ref.digest;
    next.workspace_ref = ref;
    if (changed) {
      next.instance_ref = null;
      next.session_ref = null;
      next.last_cursor = null;
    }
  }
  if (Object.hasOwn(patch, "instance_ref")) next.instance_ref = validateInstanceReference(patch.instance_ref);
  if (Object.hasOwn(patch, "session_ref")) next.session_ref = validateSessionReference(patch.session_ref);
  if (Object.hasOwn(patch, "last_cursor")) next.last_cursor = validateCursor(patch.last_cursor);
  if (Object.hasOwn(patch, "detached")) {
    if (typeof patch.detached !== "boolean") throw new Error("desktop_detached_invalid");
    next.detached = patch.detached;
  }
  if (Object.hasOwn(patch, "server_reference")) {
    const reference = patch.server_reference;
    next.session_ref = validateSessionReference(reference.session_ref);
    next.last_cursor = validateCursor(reference.last_cursor);
    next.draft_policy = reference.draft_policy;
  }
  return validateDesktopStore(next);
}

function readDesktopStore(file) {
  try {
    const stat = fs.lstatSync(file);
    if (stat.isSymbolicLink()) throw new Error("desktop_store_symlink");
    if (!stat.isFile() || stat.size > MAX_STORE_BYTES) throw new Error("desktop_store_size_invalid");
    if (process.platform !== "win32" && (stat.mode & 0o077) !== 0) {
      throw new Error("desktop_store_permissions");
    }
    return validateDesktopStore(JSON.parse(fs.readFileSync(file, "utf8")));
  } catch (error) {
    if (error && error.code === "ENOENT") return emptyDesktopStore();
    throw error;
  }
}

function writeDesktopStore(file, store) {
  const validated = validateDesktopStore(store);
  fs.mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 });
  if (fs.existsSync(file) && fs.lstatSync(file).isSymbolicLink()) throw new Error("desktop_store_symlink");
  const temp = `${file}.tmp-${process.pid}-${crypto.randomBytes(6).toString("hex")}`;
  const encoded = JSON.stringify(validated, null, 2) + "\n";
  if (Buffer.byteLength(encoded, "utf8") > MAX_STORE_BYTES) throw new Error("desktop_store_size_invalid");
  fs.writeFileSync(temp, encoded, { mode: 0o600 });
  fs.chmodSync(temp, 0o600);
  fs.renameSync(temp, file);
  fs.chmodSync(file, 0o600);
  return validated;
}

function reattachPlan(store) {
  const validated = validateDesktopStore(store);
  return {
    schema: REATTACH_SCHEMA,
    workspace_path: validated.workspace_ref && validated.workspace_ref.path,
    requires_new_handshake: true,
    cursor_disposition: validated.last_cursor ? "discard_until_new_handshake" : "none",
    auto_resume: false,
    auto_approve: false,
    auto_trust: false,
    detached: true,
  };
}

module.exports = {
  DESKTOP_STORE_SCHEMA,
  REATTACH_SCHEMA,
  REFERENCE_SCHEMA,
  digestWorkspace,
  emptyDesktopStore,
  mergeDesktopStore,
  readDesktopStore,
  reattachPlan,
  validateDesktopStore,
  validateServerReference,
  workspaceReference,
  writeDesktopStore,
};

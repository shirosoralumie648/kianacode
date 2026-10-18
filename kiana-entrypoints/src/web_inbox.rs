//! Web Human Inbox boundary.
//!
//! The loopback server accepts a versioned, deny-first action intent and forwards it through the
//! existing `human.resolve` command path.  This module performs shape validation only; actor,
//! approval, scope, expiry and policy remain server-owned in ControlPlane/Core.
//!
//! [`annotate_human_inbox_response`] adds server-owned *display* metadata to the read-only inbox
//! projection: a redacted effect scope, the primary audit fact the authority will commit, and the
//! decision-only / effect-dispatch disposition.  None of it authorizes anything — the browser
//! cannot widen a scope, pick an actor or dispatch an effect from it.

use kiana_protocol::{
    json_digest, UiHumanActionIntentV1, UI_HUMAN_ACTION_CARD_SCHEMA, UI_HUMAN_ACTION_INTENT_SCHEMA,
    UI_HUMAN_INBOX_SCHEMA,
};
use serde_json::{json, Value};

pub const WEB_HUMAN_INBOX_ROUTE_SCHEMA: &str = "kiana.web-human-inbox-route.v1";
pub const WEB_HUMAN_RESOLVE_COMMAND: &str = "human.resolve";

/// Schema of the per-item `action_audit` display metadata carried by the annotated projection.
pub const WEB_HUMAN_AUDIT_SCHEMA: &str = "kiana.web-human-audit.v1";

/// Browser-supplied top-level intent fields that only the server may resolve.  An intent carrying
/// one of them is refused before deserialization and before any replay/idempotency slot is claimed,
/// so a forged actor/scope/grant field cannot even consume a stable submission key.
pub const WEB_HUMAN_RESERVED_INTENT_FIELDS: [&str; 10] = [
    "actor",
    "actor_id",
    "scope",
    "path_allow",
    "effect_scope",
    "approval_grant",
    "role_id",
    "department_id",
    "session_id",
    "project_root",
];

/// The decision only records a human fact; it never re-executes the denied tool or dispatches a
/// capability effect.
pub const WEB_HUMAN_DISPOSITION_DECISION_ONLY: &str = "decision_only";
/// The decision records a human fact and then dispatches the approved effect (resume or delivery).
pub const WEB_HUMAN_DISPOSITION_EFFECT_DISPATCH: &str = "effect_dispatch";
/// No server-owned audit mapping exists for this command/action pair.
pub const WEB_HUMAN_DISPOSITION_UNMAPPED: &str = "unmapped";
/// The backend cannot honor this decision, so the Web boundary refuses it outright instead of
/// leaving it in the unmapped class.
pub const WEB_HUMAN_DISPOSITION_REFUSED: &str = "refused";

/// Resolve decisions the card list once claimed but the backend cannot honor.
///
/// `kiana-core`'s `human.resolve` dispatch reaches only the approval (Approve/Deny),
/// company/company.business, failure.reconcile/failure.recovery and feedback.review authorities.
/// There is no amend/edit path for a staged approval; workspace restore is its own
/// `workspace.checkpoint.restore` command whose human decision is the staged approval's
/// approve/deny (it is not a `human.resolve` action); and continuing a run is the explicit
/// `/api/resume` command (`ControlPlane::continue_run`), never a resolve decision.  A browser
/// intent using one of these ids is refused with [`WEB_HUMAN_REFUSAL_ACTION_UNSUPPORTED`] before
/// any replay slot is claimed, so the action can never silently fall into the unmapped
/// disposition.
pub const WEB_HUMAN_UNSUPPORTED_RESOLVE_ACTIONS: [&str; 3] = ["edit", "restore", "continue"];

/// Stable refusal code prefix for an unsupported resolve decision; the action id is appended.
pub const WEB_HUMAN_REFUSAL_ACTION_UNSUPPORTED: &str = "web_human_resolve_action_unsupported";

/// One typed, displayable refusal row: the decision the Web card would otherwise offer, the
/// stable code the boundary returns, and why the backend cannot honor it.
pub struct WebHumanActionRefusal {
    pub command: &'static str,
    pub action_id: &'static str,
    pub refusal_code: &'static str,
    pub reason: &'static str,
}

/// Declared size and rows must stay in sync; the fixture asserts both.
pub const WEB_HUMAN_REFUSALS: [WebHumanActionRefusal; 3] = [
    WebHumanActionRefusal {
        command: "approval",
        action_id: "edit",
        refusal_code: WEB_HUMAN_REFUSAL_ACTION_UNSUPPORTED,
        reason: "no_approval_amend_authority",
    },
    WebHumanActionRefusal {
        command: "approval",
        action_id: "restore",
        refusal_code: WEB_HUMAN_REFUSAL_ACTION_UNSUPPORTED,
        reason: "restore_is_a_separate_checkpoint_command",
    },
    WebHumanActionRefusal {
        command: "approval",
        action_id: "continue",
        refusal_code: WEB_HUMAN_REFUSAL_ACTION_UNSUPPORTED,
        reason: "continue_is_a_separate_resume_command",
    },
];

fn human_action_refusal(command: &str, action_id: &str) -> Option<&'static WebHumanActionRefusal> {
    WEB_HUMAN_REFUSALS
        .iter()
        .find(|refusal| refusal.command == command && refusal.action_id == action_id)
}

/// The intent wire shape carries no `command`, so the three reserved ids are refused by name at
/// that boundary. A card that would legitimately reuse one of them on another command would have
/// to be added to the refusal table's `command` column and given its own path first.
fn reserved_refusal(action_id: &str) -> Option<&'static WebHumanActionRefusal> {
    WEB_HUMAN_REFUSALS
        .iter()
        .find(|refusal| refusal.action_id == action_id)
}

/// Effect paths are a bounded display projection; the authoritative write set stays in the
/// WorkPacket/PathLock/approval scope re-derived by the server.
pub const WEB_HUMAN_MAX_EFFECT_PATHS: usize = 8;
pub const WEB_HUMAN_MAX_EFFECT_PATH_BYTES: usize = 1_024;

/// One display-only mapping from an inbox action to the primary audit fact the authority commits
/// when the action is decided.  `action: None` matches every action of the command (used by the
/// recovery/feedback actions whose IDs carry role-specific labels).
pub struct WebHumanAuditRule {
    pub command: &'static str,
    pub action: Option<&'static str>,
    pub audit_event: &'static str,
    pub disposition: &'static str,
    pub dangerous: bool,
}

const fn audit_rule(
    command: &'static str,
    action: &'static str,
    audit_event: &'static str,
    disposition: &'static str,
    dangerous: bool,
) -> WebHumanAuditRule {
    WebHumanAuditRule {
        command,
        action: Some(action),
        audit_event,
        disposition,
        dangerous,
    }
}

/// `dangerous` marks the actions whose decision dispatches an effect beyond recording the human
/// fact: approving a staged capability resumes the run's approved effect, and dispatching a
/// closeout delivery authorizes the local delivery package.  The Web card renders those collapsed
/// with the effect scope in the summary.  `deny`/`reject`/`waive` are always decision-only, so a
/// denial can never re-execute the tool it refused.
pub const WEB_HUMAN_AUDIT_RULES: [WebHumanAuditRule; 14] = [
    audit_rule(
        "approval",
        "approve",
        "approval.approved",
        WEB_HUMAN_DISPOSITION_EFFECT_DISPATCH,
        true,
    ),
    audit_rule(
        "approval",
        "deny",
        "approval.denied",
        WEB_HUMAN_DISPOSITION_DECISION_ONLY,
        false,
    ),
    audit_rule(
        "company",
        "record_review",
        "company.ReviewRecorded",
        WEB_HUMAN_DISPOSITION_DECISION_ONLY,
        false,
    ),
    audit_rule(
        "company",
        "accept",
        "company.AcceptanceDecided",
        WEB_HUMAN_DISPOSITION_DECISION_ONLY,
        false,
    ),
    audit_rule(
        "company",
        "reject",
        "company.AcceptanceDecided",
        WEB_HUMAN_DISPOSITION_DECISION_ONLY,
        false,
    ),
    audit_rule(
        "company",
        "waive",
        "company.AcceptanceDecided",
        WEB_HUMAN_DISPOSITION_DECISION_ONLY,
        false,
    ),
    audit_rule(
        "company.business",
        "dispatch_delivery",
        "company.business.v2",
        WEB_HUMAN_DISPOSITION_EFFECT_DISPATCH,
        true,
    ),
    audit_rule(
        "company.business",
        "reconcile_delivery",
        "company.business.v2",
        WEB_HUMAN_DISPOSITION_DECISION_ONLY,
        false,
    ),
    audit_rule(
        "company.business",
        "confirm_delivery",
        "company.business.v2",
        WEB_HUMAN_DISPOSITION_DECISION_ONLY,
        false,
    ),
    audit_rule(
        "company.business",
        "decide_change",
        "company.business.v2",
        WEB_HUMAN_DISPOSITION_DECISION_ONLY,
        false,
    ),
    audit_rule(
        "company.business",
        "reconcile_cancel",
        "company.business.v2",
        WEB_HUMAN_DISPOSITION_DECISION_ONLY,
        false,
    ),
    audit_rule(
        "failure.reconcile",
        "reconcile",
        "failure.reconciled",
        WEB_HUMAN_DISPOSITION_DECISION_ONLY,
        false,
    ),
    WebHumanAuditRule {
        command: "failure.recovery",
        action: None,
        audit_event: "failure.recovery.transition",
        disposition: WEB_HUMAN_DISPOSITION_DECISION_ONLY,
        dangerous: false,
    },
    WebHumanAuditRule {
        command: "feedback.review",
        action: None,
        audit_event: "feedback.candidate_reviewed",
        disposition: WEB_HUMAN_DISPOSITION_DECISION_ONLY,
        dangerous: false,
    },
];

fn human_audit_rule(command: &str, action_id: &str) -> Option<&'static WebHumanAuditRule> {
    WEB_HUMAN_AUDIT_RULES.iter().find(|rule| {
        rule.command == command && (rule.action.is_none() || rule.action == Some(action_id))
    })
}

/// Validate a Web human action before it reaches the existing command facade.  A normal read
/// command is left untouched; `human.resolve` must carry the typed intent and cannot carry actor,
/// scope or approval-grant fields supplied by the browser.
pub(crate) fn validate_human_action_command(
    command: &str,
    arguments: &Value,
) -> Result<(), String> {
    if command != WEB_HUMAN_RESOLVE_COMMAND {
        return Ok(());
    }
    let Some(object) = arguments.as_object() else {
        return Err("web_human_action_intent_invalid".to_owned());
    };
    if let Some(field) = WEB_HUMAN_RESERVED_INTENT_FIELDS
        .iter()
        .find(|field| object.contains_key(**field))
    {
        return Err(format!("web_human_action_reserved_field:{field}"));
    }
    let intent: UiHumanActionIntentV1 = serde_json::from_value(arguments.clone())
        .map_err(|_| "web_human_action_intent_invalid".to_owned())?;
    if intent.schema != UI_HUMAN_ACTION_INTENT_SCHEMA {
        return Err("web_human_action_intent_schema_invalid".to_owned());
    }
    intent.validate()?;
    refuse_unsupported_resolve_action(&intent.action_id)
}

/// Refuse an intent for a decision the backend cannot honor.  The card projection never serves
/// these ids (see [`WEB_HUMAN_UNSUPPORTED_RESOLVE_ACTIONS`]), so a browser that fabricates one is
/// told the truth with a stable code instead of receiving an `unmapped` audit disposition or an
/// opaque `human_action_unavailable`.
fn refuse_unsupported_resolve_action(action_id: &str) -> Result<(), String> {
    if let Some(refusal) = reserved_refusal(action_id) {
        return Err(format!("{}:{action_id}", refusal.refusal_code));
    }
    Ok(())
}

/// Return a stable display classification for a command response.  `unknown` is kept distinct
/// from rejection so the browser can query the original command instead of retrying blindly.
#[allow(dead_code)]
pub(crate) fn human_result_class(response: &Value) -> &'static str {
    match response.get("status").and_then(Value::as_str) {
        Some("completed") => "applied",
        Some("accepted") | Some("running") | Some("awaiting_approval") => "accepted",
        Some("unknown") | Some("result_unknown") => "unknown",
        Some("blocked") | Some("denied") | Some("rejected") | Some("failed") => "rejected",
        _ => "unknown",
    }
}

/// Redacted, bounded effect scope for the card's danger summary.  Paths come from the already
/// redacted server action arguments; invalid or URL-looking entries are dropped instead of
/// rendered, and at most [`WEB_HUMAN_MAX_EFFECT_PATHS`] entries survive.
fn human_effect_scope(detail: &Value) -> Value {
    let mut candidates: Vec<&str> = Vec::new();
    if let Some(arguments) = detail.get("arguments") {
        if let Some(paths) = arguments.get("path_allow").and_then(Value::as_array) {
            candidates.extend(paths.iter().filter_map(Value::as_str));
        }
        if let Some(path) = arguments.get("path").and_then(Value::as_str) {
            candidates.push(path);
        }
    }
    let mut paths: Vec<String> = Vec::new();
    let mut truncated = false;
    for candidate in candidates {
        let candidate = candidate.trim();
        if candidate.is_empty()
            || candidate.len() > WEB_HUMAN_MAX_EFFECT_PATH_BYTES
            || candidate.contains('\0')
            || candidate.starts_with("http://")
            || candidate.starts_with("https://")
        {
            continue;
        }
        if paths.iter().any(|known| known == candidate) {
            continue;
        }
        if paths.len() == WEB_HUMAN_MAX_EFFECT_PATHS {
            truncated = true;
            break;
        }
        paths.push(candidate.to_owned());
    }
    json!({"paths": paths, "truncated": truncated})
}

fn human_audit_entry(command: &str, action_id: &str, effect: &Value) -> Value {
    let rule = human_audit_rule(command, action_id);
    // A decision the backend cannot honor is reported as refused, never as unmapped, so the card
    // never offers an edit/restore/continue that would silently become an authoritative-looking
    // no-op.  The resolve boundary refuses the same ids before any replay slot is claimed.
    let refusal = human_action_refusal(command, action_id);
    json!({
        "schema": WEB_HUMAN_AUDIT_SCHEMA,
        "action_id": action_id,
        "command": command,
        "dangerous": rule.is_some_and(|rule| rule.dangerous),
        "audit_event": rule.map(|rule| rule.audit_event),
        "audit_disposition": if refusal.is_some() {
            WEB_HUMAN_DISPOSITION_REFUSED
        } else {
            rule.map(|rule| rule.disposition)
                .unwrap_or(WEB_HUMAN_DISPOSITION_UNMAPPED)
        },
        "refusal_code": refusal.map(|refusal| format!("{}:{action_id}", refusal.refusal_code)),
        "refusal_reason": refusal.map(|refusal| refusal.reason),
        "effect": effect.clone()
    })
}

/// Additive server-owned metadata to the existing `human.inbox` response.  The original item and
/// action arguments remain available to the ControlPlane command path; the Web card contains only
/// redacted display fields and a digest, so the browser cannot treat its copy as authority.
pub(crate) fn annotate_human_inbox_response(response: &mut Value) {
    if response.get("status").and_then(Value::as_str) != Some("completed") {
        return;
    }
    let Some(output) = response.get_mut("output").and_then(Value::as_object_mut) else {
        return;
    };
    output.insert("schema".to_owned(), json!(UI_HUMAN_INBOX_SCHEMA));
    output.insert(
        "action_card_schema".to_owned(),
        json!(UI_HUMAN_ACTION_CARD_SCHEMA),
    );
    output.insert("audit_schema".to_owned(), json!(WEB_HUMAN_AUDIT_SCHEMA));
    let Some(items) = output.get_mut("items").and_then(Value::as_array_mut) else {
        return;
    };
    for item in items {
        let item_id = item
            .get("item_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let source_ref = item
            .get("source_ref")
            .and_then(Value::as_str)
            .unwrap_or(&item_id)
            .to_owned();
        let title = item
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Human action")
            .to_owned();
        let detail = item.get("detail").cloned().unwrap_or(Value::Null);
        let expected_revision = detail
            .get("expected_version")
            .and_then(Value::as_u64)
            .or_else(|| detail.get("company_revision").and_then(Value::as_u64));
        let expires_at_unix_ms = detail
            .get("challenge")
            .and_then(|challenge| challenge.get("expires_at_unix_ms"))
            .and_then(Value::as_u64);
        let reason = detail
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or(&title)
            .to_owned();
        let scope_summary = detail
            .get("scope")
            .and_then(|scope| scope.get("summary"))
            .and_then(Value::as_str)
            .unwrap_or(&source_ref)
            .to_owned();
        let scope_digest = json_digest(&json!({"item_id": &item_id, "source_ref": &source_ref}));
        let effect = human_effect_scope(&detail);
        let Some(actions) = item.get_mut("actions").and_then(Value::as_array_mut) else {
            continue;
        };
        let mut cards = Vec::with_capacity(actions.len());
        let mut audit = Vec::with_capacity(actions.len());
        for action in actions.iter() {
            let action_id = action
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let command = action
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or("human.resolve")
                .to_owned();
            let arguments = action.get("arguments").cloned().unwrap_or(Value::Null);
            let payload_digest = json_digest(&arguments);
            let fields = action
                .get("required_fields")
                .and_then(Value::as_array)
                .map(|fields| {
                    fields
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|name| {
                            json!({
                                "name": name,
                                "label": name,
                                "field_type": human_field_type(name),
                                "required": true,
                                "allowed_values": []
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            cards.push(json!({
                "schema": UI_HUMAN_ACTION_CARD_SCHEMA,
                "item_id": &item_id,
                "action_id": &action_id,
                "command": &command,
                "target_id": &source_ref,
                "reason": &reason,
                "scope": {"summary": &scope_summary, "digest": &scope_digest, "paths": []},
                "expires_at_unix_ms": expires_at_unix_ms,
                "revoked": false,
                "expected_revision": expected_revision,
                "fields": fields,
                "allowed_decisions": [&action_id],
                "payload_digest": &payload_digest
            }));
            audit.push(human_audit_entry(&command, &action_id, &effect));
        }
        if let Some(item_object) = item.as_object_mut() {
            item_object.insert("action_cards".to_owned(), Value::Array(cards));
            item_object.insert("action_audit".to_owned(), Value::Array(audit));
        }
    }
}

/// Display-only field type for a server-required field name.  The client validates submitted
/// values against this declared type and fails closed on anything unknown; the authority still
/// rechecks the resolved action payload.
fn human_field_type(name: &str) -> &'static str {
    match name {
        "approve" => "boolean",
        "evidence_refs" | "reasons" => "string_list",
        "review_id" | "waiver_ref" | "resolution" | "package_sha256" | "decision_ref" => "string",
        _ => "json",
    }
}

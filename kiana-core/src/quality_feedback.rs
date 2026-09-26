//! EQ-45 ControlPlane adapter for server-derived quality feedback.
//!
//! This is an admission/value adapter only.  It derives principal/project/session and source
//! provenance from the trusted request context, then delegates all schema and target checks to
//! `kiana-domain`.  It does not write policy, grants, approvals, receipts, routes, or memory ACLs.

use kiana_domain::{
    json_digest, QualityCanonicalTarget, QualityFeedback, QualityFeedbackPrivacyClass,
    QualityFeedbackServerContext, QualityFeedbackSubmission, RequestContext,
};
use serde_json::json;

pub const QUALITY_FEEDBACK_COMMAND: &str = "quality.feedback";

/// Derive a feedback record from a trusted ControlPlane request and committed source references.
///
/// `source_events` are opaque server facts.  The adapter stores only bounded hashed references in
/// the resulting provenance, so a client cannot inject raw paths, credentials, or a privacy scope.
pub fn derive_quality_feedback(
    context: &RequestContext,
    submission: QualityFeedbackSubmission,
    source_cursor: u64,
    source_events: &[String],
    captured_at_unix_ms: u64,
) -> Result<QualityFeedback, String> {
    if !context.project_trusted {
        return Err("quality_feedback_project_untrusted".to_owned());
    }
    let actor = context
        .actor_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "quality_feedback_actor_required".to_owned())?;
    if source_events.is_empty() {
        return Err("quality_feedback_source_events_required".to_owned());
    }
    let source_event_ids = source_events
        .iter()
        .map(|value| server_ref("event", value))
        .collect::<Vec<_>>();
    let source_event_digest = json_digest(&json!({
        "cursor": source_cursor,
        "events": source_event_ids,
    }));
    let server_context = QualityFeedbackServerContext {
        principal_ref: server_ref("principal", actor),
        project_ref: server_ref("project", &context.project_root),
        session_ref: server_ref("session", &context.session_id.to_string()),
        source_cursor,
        source_event_ids,
        source_event_digest,
        target_privacy_class: target_privacy_class(&submission.target),
        captured_at_unix_ms,
    };
    QualityFeedback::derive(submission, server_context)
}

fn target_privacy_class(target: &QualityCanonicalTarget) -> QualityFeedbackPrivacyClass {
    match target.target_type {
        // Memory facts may contain user/private content; keep feedback principal scoped by default.
        kiana_domain::QualityFeedbackTargetType::Memory => {
            QualityFeedbackPrivacyClass::Confidential
        }
        // Receipt facts are project-visible evidence but never editable through feedback.
        kiana_domain::QualityFeedbackTargetType::Receipt => QualityFeedbackPrivacyClass::Internal,
        _ => QualityFeedbackPrivacyClass::Internal,
    }
}

fn server_ref(prefix: &str, value: &str) -> String {
    format!("{prefix}:{}", json_digest(&json!({"value": value})))
}

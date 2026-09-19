//! Core-owned clarification admission and Human Inbox projection.
//!
//! This boundary validates a question answer against the original run/turn/step and returns a
//! continuation record.  It never calls the approval store, constructs a capability request or
//! changes the tool catalog.  The caller must append the returned fact through the normal EventLog
//! path before forwarding the continuation to the same Runner.

use kiana_domain::{
    ClarificationAnswer, ClarificationRequest, ClarificationResolution, HumanAction,
    HumanInboxItem, HumanInboxKind,
};
use serde_json::json;

pub const CLARIFICATION_CORE_SCHEMA: &str = "kiana.core-clarification.v1";

#[derive(Clone, Debug, PartialEq)]
pub struct ClarificationCommit {
    pub schema: &'static str,
    pub request: ClarificationRequest,
    pub resolution: ClarificationResolution,
}

/// Validate and prepare one answer for the original model step.
///
/// This is intentionally a pure core boundary.  It produces the exact fact to persist; it does
/// not treat an ordinary answer as an approval decision and does not issue a capability permit.
pub fn commit_clarification_answer(
    request: &ClarificationRequest,
    answer: ClarificationAnswer,
    now_unix_ms: u64,
) -> Result<ClarificationCommit, String> {
    let (next_request, resolution) = request.accept_answer(answer, now_unix_ms)?;
    resolution.validate()?;
    Ok(ClarificationCommit {
        schema: CLARIFICATION_CORE_SCHEMA,
        request: next_request,
        resolution,
    })
}

/// Build the common Human Inbox projection used by TTY and Web.  The action is a regular input
/// answer and has no approval ID, grant ID, capability request or tool arguments.
pub fn clarification_human_inbox_item(
    request: &ClarificationRequest,
) -> Result<HumanInboxItem, String> {
    let wait = request.waiting_view()?;
    Ok(HumanInboxItem {
        item_id: format!("clarification:{}", request.interaction_id),
        kind: HumanInboxKind::Question,
        title: request.question.clone(),
        source_ref: format!("clarification:{}", request.interaction_id),
        run_id: Some(request.run_id),
        detail: json!({
            "question": request.question,
            "wait": wait,
            "required": request.required,
            "expires_at_unix_ms": request.expires_at_unix_ms,
        }),
        actions: vec![HumanAction {
            id: "answer_clarification".to_owned(),
            label: "回答澄清".to_owned(),
            command: "run.clarification.answer".to_owned(),
            arguments: json!({
                "interaction_id": request.interaction_id,
                "run_id": request.run_id,
                "turn_id": request.turn_id,
                "step_id": request.step_id,
            }),
            required_fields: vec!["text".to_owned()],
        }],
    })
}

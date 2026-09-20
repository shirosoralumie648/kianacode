use crate::hook_outcome::HookOutcome;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const HOOK_RECEIPT_SCHEMA: &str = "kiana.hook-receipt.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookCleanupState {
    Confirmed,
    Unknown,
    NotStarted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookReceipt {
    pub schema: String,
    pub hook_id: String,
    pub snapshot_digest: String,
    pub input_digest: String,
    pub output_digest: String,
    pub outcome: HookOutcome,
    pub approval_ref: Option<String>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
    pub patch_before_digest: Option<String>,
    pub patch_after_digest: Option<String>,
    pub cleanup: HookCleanupState,
    pub replay_safe: bool,
    pub receipt_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookReplayView {
    pub hook_id: String,
    pub outcome: HookOutcome,
    pub cleanup: HookCleanupState,
    pub fenced: bool,
    pub requires_control_plane_decision: bool,
    pub executed: bool,
}

pub fn replay_hook_receipt(receipt: &HookReceipt) -> Result<HookReplayView, String> {
    if receipt.schema != HOOK_RECEIPT_SCHEMA
        || receipt.hook_id.trim().is_empty()
        || !receipt.snapshot_digest.starts_with("sha256:")
        || !receipt.input_digest.starts_with("sha256:")
        || !receipt.output_digest.starts_with("sha256:")
        || receipt.replay_safe
    {
        return Err("hook_receipt_invalid_or_not_replayable".to_owned());
    }
    let unknown = receipt.cleanup == HookCleanupState::Unknown
        || matches!(receipt.outcome, HookOutcome::Unknown { .. });
    Ok(HookReplayView {
        hook_id: receipt.hook_id.clone(),
        outcome: receipt.outcome.clone(),
        cleanup: receipt.cleanup,
        fenced: unknown,
        requires_control_plane_decision: unknown,
        executed: false,
    })
}

pub fn hook_receipt_digest(receipt: &HookReceipt) -> String {
    kiana_domain::json_digest(&serde_json::to_value(receipt).unwrap_or(Value::Null))
}

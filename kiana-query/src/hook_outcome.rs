use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const HOOK_OUTCOME_SCHEMA: &str = "kiana.hook-outcome.v1";
const MAX_PATCH_BYTES: usize = 16 * 1024;
const MAX_CONTEXT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookOutcome {
    Allow,
    Block {
        reason: String,
    },
    Ask {
        approval_ref: String,
        reason: String,
    },
    UpdateInput {
        patch: Value,
        reauthorize: bool,
    },
    AdditionalContext {
        items: Vec<String>,
    },
    Timeout {
        timeout_ms: u64,
    },
    Cancelled {
        reason: String,
    },
    Unknown {
        reason: String,
    },
}

impl HookOutcome {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Allow => Ok(()),
            Self::Block { reason } | Self::Cancelled { reason } | Self::Unknown { reason } => {
                bounded_text(reason, 4_096)
            }
            Self::Ask {
                approval_ref,
                reason,
            } => {
                bounded_text(approval_ref, 256)?;
                bounded_text(reason, 4_096)
            }
            Self::UpdateInput { patch, reauthorize } => {
                if !*reauthorize {
                    return Err("hook_update_requires_reauthorization".to_owned());
                }
                bounded_json(patch, MAX_PATCH_BYTES)
            }
            Self::AdditionalContext { items } => {
                if items.len() > 32
                    || items
                        .iter()
                        .any(|item| item.trim().is_empty() || item.len() > 4_096)
                    || items.iter().map(String::len).sum::<usize>() > MAX_CONTEXT_BYTES
                {
                    return Err("hook_context_limit_exceeded".to_owned());
                }
                Ok(())
            }
            Self::Timeout { timeout_ms } if *timeout_ms == 0 || *timeout_ms > 10_000 => {
                Err("hook_timeout_invalid".to_owned())
            }
            Self::Timeout { .. } => Ok(()),
        }
    }
}

pub fn parse_hook_outcome(raw: &str) -> Result<HookOutcome, String> {
    let value: Value = serde_json::from_str(raw).map_err(|_| "hook_output_invalid".to_owned())?;
    let object = value
        .as_object()
        .ok_or_else(|| "hook_output_invalid".to_owned())?;
    let allowed = BTreeSet::from([
        "decision",
        "reason",
        "approval_ref",
        "approvalRef",
        "update_input",
        "updated_input",
        "additional_context",
        "additionalContext",
        "timeout_ms",
        "cancelled",
        "unknown",
    ]);
    if object.keys().any(|key| !allowed.contains(key.as_str())) {
        return Err("hook_output_unknown_field".to_owned());
    }
    let reason = object
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("hook outcome")
        .to_owned();
    let decision = object.get("decision").and_then(Value::as_str);
    let outcome = match decision {
        Some("allow" | "approve") | None if object.get("unknown").is_none() => HookOutcome::Allow,
        Some("block" | "deny") => HookOutcome::Block { reason },
        Some("ask") => HookOutcome::Ask {
            approval_ref: object
                .get("approval_ref")
                .or_else(|| object.get("approvalRef"))
                .and_then(Value::as_str)
                .ok_or_else(|| "hook_approval_ref_missing".to_owned())?
                .to_owned(),
            reason,
        },
        Some("update_input") => HookOutcome::UpdateInput {
            patch: object
                .get("update_input")
                .or_else(|| object.get("updated_input"))
                .cloned()
                .ok_or_else(|| "hook_update_input_missing".to_owned())?,
            reauthorize: true,
        },
        Some("additional_context") => HookOutcome::AdditionalContext {
            items: object
                .get("additional_context")
                .or_else(|| object.get("additionalContext"))
                .and_then(Value::as_array)
                .ok_or_else(|| "hook_context_missing".to_owned())?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        },
        Some("timeout") => HookOutcome::Timeout {
            timeout_ms: object
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .ok_or_else(|| "hook_timeout_missing".to_owned())?,
        },
        Some("cancelled") => HookOutcome::Cancelled { reason },
        _ => HookOutcome::Unknown { reason },
    };
    outcome.validate()?;
    Ok(outcome)
}

fn bounded_text(value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err("hook_outcome_text_invalid".to_owned())
    } else {
        Ok(())
    }
}

fn bounded_json(value: &Value, max_bytes: usize) -> Result<(), String> {
    if serde_json::to_vec(value)
        .map_err(|_| "hook_patch_invalid".to_owned())?
        .len()
        > max_bytes
    {
        Err("hook_patch_limit_exceeded".to_owned())
    } else {
        Ok(())
    }
}

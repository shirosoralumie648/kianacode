//! Stable error classifications shared by capability results and client surfaces.
//!
//! Classification never authorizes a retry. Each new attempt must pass the control
//! plane again; an uncertain side effect requires reconciliation first. Existing
//! reason strings remain available for diagnostics, while these categories and
//! their policies are append-only wire contracts.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilityErrorPolicy {
    pub cli_exit: u8,
    pub http_status: u16,
    pub retryable: bool,
    pub requires_new_authorization: bool,
    pub requires_compensation: bool,
    pub requires_reconciliation: bool,
}

macro_rules! error_codes {
    ($( $variant:ident => ($name:literal, $exit:literal, $http:literal, $retry:literal, $auth:literal, $compensation:literal, $reconcile:literal) ),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum CapabilityErrorCode { $( $variant, )+ }

        impl CapabilityErrorCode {
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $name,)+ }
            }

            pub const fn policy(self) -> CapabilityErrorPolicy {
                match self {
                    $(Self::$variant => CapabilityErrorPolicy {
                        cli_exit: $exit,
                        http_status: $http,
                        retryable: $retry,
                        requires_new_authorization: $auth,
                        requires_compensation: $compensation,
                        requires_reconciliation: $reconcile,
                    },)+
                }
            }
        }
    };
}

error_codes! {
    InvalidArguments => ("invalid_arguments", 2, 400, false, true, false, false),
    SchemaUnsupported => ("schema_unsupported", 2, 400, false, true, false, false),
    PathEscape => ("path_escape", 3, 403, false, true, false, false),
    PermissionDenied => ("permission_denied", 3, 403, false, true, false, false),
    ProjectUntrusted => ("project_untrusted", 3, 403, false, true, false, false),
    ApprovalRequired => ("approval_required", 4, 409, false, true, false, false),
    ApprovalExpired => ("approval_expired", 4, 409, false, true, false, false),
    ApprovalConsumed => ("approval_consumed", 4, 409, false, true, false, false),
    ApprovalDenied => ("approval_denied", 3, 403, false, true, false, false),
    Conflict => ("conflict", 5, 409, false, true, false, false),
    NotFound => ("not_found", 5, 404, false, false, false, false),
    Unsupported => ("unsupported", 2, 501, false, false, false, false),
    BudgetExceeded => ("budget_exceeded", 6, 429, false, true, false, false),
    Cancelled => ("cancelled", 130, 409, false, true, false, false),
    TimedOut => ("timed_out", 7, 504, false, true, false, true),
    Unavailable => ("unavailable", 7, 503, false, true, false, false),
    ResultUnknown => ("result_unknown", 8, 409, false, true, false, true),
    CompensationRequired => ("compensation_required", 8, 409, false, true, true, true),
    ExecutionFailed => ("execution_failed", 1, 500, false, true, false, false),
}

impl CapabilityErrorCode {
    /// Interpret an existing reason without inspecting arbitrary diagnostic text.
    /// Only known transport wrappers are removed. Unknown reasons conservatively
    /// retain the execution-failed policy, which never permits automatic retry.
    pub fn from_reason(reason: &str) -> Self {
        let mut reason = reason.trim();
        let mut fallback = Self::ExecutionFailed;
        loop {
            let Some((prefix, detail)) = reason.split_once(':') else {
                break;
            };
            if matches!(prefix, "port_failed" | "port_conflict" | "port_unavailable") {
                fallback = match prefix {
                    "port_conflict" => Self::Conflict,
                    "port_unavailable" => Self::Unavailable,
                    _ => fallback,
                };
                reason = detail.trim();
            } else {
                break;
            }
        }
        let code = reason.split(':').next().unwrap_or_default();
        if let Some(known) = Self::ALL.iter().find(|known| known.as_str() == code) {
            return *known;
        }
        match code {
            "shell_result_unknown"
            | "capability_result_mismatch"
            | "run_terminal_conflict"
            | "result_event_persistence_failed" => Self::ResultUnknown,
            "harness_workdir_outside_project"
            | "apply_patch_path_outside_project"
            | "apply_patch_path_escape"
            | "path_outside_project" => Self::PathEscape,
            "workspace_write_requires_trusted_non_safe_profile" => Self::ProjectUntrusted,
            "hook_blocked"
            | "packet_path_denied"
            | "role_path_denied"
            | "memory_scope_denied"
            | "memory_access_denied"
            | "role_tool_denied"
            | "sandbox_read_only"
            | "authority_epoch_stale"
            | "capability_blocked"
            | "cell_capability_grant_denied"
            | "cell_capability_scope_mismatch"
            | "cell_capability_scope_incomplete" => Self::PermissionDenied,
            "hook_ask_unattended" => Self::ApprovalRequired,
            "cell_capability_grant_expired" => Self::PermissionDenied,
            "approval_already_consumed" | "approval_replayed" => Self::ApprovalConsumed,
            "approval_continuation_unavailable" | "run_resume_unavailable" => Self::Unavailable,
            "run_budget_exceeded"
            | "max_steps_per_turn"
            | "repeated_tool_call"
            | "context_budget_exceeded"
            | "cell_capability_concurrency_exceeded" => Self::BudgetExceeded,
            "session_not_found" | "run_not_found" | "approval_not_found" => Self::NotFound,
            "mcp_transport_unsupported" | "sandbox_unsupported" => Self::Unsupported,
            "model_unavailable"
            | "model_gateway_unavailable"
            | "sandbox_unavailable"
            | "capability_unregistered" => Self::Unavailable,
            "protocol_schema_unsupported" | "unknown_schema" | "schema_version_incompatible" => {
                Self::SchemaUnsupported
            }
            "prompt_required" | "packet_invalid" | "role_unknown" => Self::InvalidArguments,
            "cell_capability_invocation_duplicate" | "run_already_exists" => Self::Conflict,
            "timeout" | "shell_timeout" => Self::TimedOut,
            _ => fallback,
        }
    }
}

impl std::fmt::Display for CapabilityErrorCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

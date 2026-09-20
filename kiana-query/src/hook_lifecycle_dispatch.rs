use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const HOOK_LIFECYCLE_DISPATCH_SCHEMA: &str = "kiana.hook-lifecycle-dispatch.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookLifecycleEventKind {
    SessionStart,
    UserPromptSubmit,
    BeforeModel,
    PreToolUse,
    PostToolUse,
    PostToolFailure,
    Compaction,
    Stop,
    SessionEnd,
    Terminal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookLifecycleEvent {
    pub schema: String,
    pub kind: HookLifecycleEventKind,
    pub session_id: String,
    pub run_id: String,
    pub snapshot_id: String,
    pub sequence: u64,
    pub payload_digest: String,
    pub result_committed: bool,
    pub capability_dispatch_allowed: bool,
    pub event_digest: String,
}

#[derive(Default)]
pub struct HookLifecycleDispatcher;

impl HookLifecycleDispatcher {
    pub fn dispatch(
        &self,
        kind: HookLifecycleEventKind,
        session_id: impl Into<String>,
        run_id: impl Into<String>,
        snapshot_id: impl Into<String>,
        sequence: u64,
        payload: &Value,
        result_committed: bool,
    ) -> Result<HookLifecycleEvent, String> {
        let session_id = session_id.into();
        let run_id = run_id.into();
        let snapshot_id = snapshot_id.into();
        if session_id.trim().is_empty()
            || run_id.trim().is_empty()
            || snapshot_id.trim().is_empty()
            || sequence == 0
        {
            return Err("hook_lifecycle_identity_invalid".to_owned());
        }
        if matches!(
            kind,
            HookLifecycleEventKind::PostToolUse | HookLifecycleEventKind::PostToolFailure
        ) && !result_committed
        {
            return Err("hook_post_tool_result_not_committed".to_owned());
        }
        let mut event = HookLifecycleEvent {
            schema: HOOK_LIFECYCLE_DISPATCH_SCHEMA.to_owned(),
            kind,
            session_id,
            run_id,
            snapshot_id,
            sequence,
            payload_digest: json_digest(payload),
            result_committed,
            capability_dispatch_allowed: false,
            event_digest: String::new(),
        };
        event.event_digest = event.digest();
        Ok(event)
    }
}

impl HookLifecycleEvent {
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "kind": self.kind,
            "session_id": self.session_id,
            "run_id": self.run_id,
            "snapshot_id": self.snapshot_id,
            "sequence": self.sequence,
            "payload_digest": self.payload_digest,
            "result_committed": self.result_committed,
            "capability_dispatch_allowed": self.capability_dispatch_allowed,
        }))
    }
}

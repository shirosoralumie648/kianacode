use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const HOOK_CANCELLATION_SCHEMA: &str = "kiana.hook-cancellation.v1";
pub const MAX_HOOK_RECURSION_DEPTH: u8 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookRunRole {
    Guard,
    Observer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookRunTerminal {
    Completed,
    Blocked,
    Cancelled,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookInvocationIdentity {
    pub schema: String,
    pub run_id: String,
    pub hook_id: String,
    pub invocation_id: String,
    pub idempotency_key: String,
    pub recursion_depth: u8,
    pub visited: BTreeSet<String>,
    pub identity_digest: String,
}

pub fn prepare_hook_invocation(
    run_id: &str,
    hook_id: &str,
    invocation_id: &str,
    parent: Option<&HookInvocationIdentity>,
) -> Result<HookInvocationIdentity, String> {
    if [run_id, hook_id, invocation_id]
        .iter()
        .any(|value| value.trim().is_empty() || value.len() > 256 || value.contains('\0'))
    {
        return Err("hook_invocation_identity_invalid".to_owned());
    }
    let recursion_depth = parent.map_or(0, |value| value.recursion_depth.saturating_add(1));
    if recursion_depth > MAX_HOOK_RECURSION_DEPTH {
        return Err("hook_recursion_limit".to_owned());
    }
    let key = format!("{run_id}:{hook_id}:{invocation_id}");
    let mut visited = parent.map_or_else(BTreeSet::new, |value| value.visited.clone());
    if !visited.insert(key.clone()) {
        return Err("hook_recursion_visited".to_owned());
    }
    let mut identity = HookInvocationIdentity {
        schema: HOOK_CANCELLATION_SCHEMA.to_owned(),
        run_id: run_id.to_owned(),
        hook_id: hook_id.to_owned(),
        invocation_id: invocation_id.to_owned(),
        idempotency_key: format!("hook:{key}"),
        recursion_depth,
        visited,
        identity_digest: String::new(),
    };
    identity.identity_digest = json_digest(&serde_json::to_value(&identity).unwrap_or_default());
    Ok(identity)
}

pub fn cancellation_terminal(role: HookRunRole, stop_confirmed: Option<bool>) -> HookRunTerminal {
    match (role, stop_confirmed) {
        (_, Some(true)) => HookRunTerminal::Cancelled,
        (HookRunRole::Observer, None) => HookRunTerminal::Unknown,
        _ => HookRunTerminal::Unknown,
    }
}

pub fn retry_allowed(role: HookRunRole, terminal: HookRunTerminal) -> bool {
    role == HookRunRole::Observer && matches!(terminal, HookRunTerminal::Unknown)
}

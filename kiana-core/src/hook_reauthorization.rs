use kiana_domain::{capability_request_paths, json_digest, CapabilityRequest, RequestId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const HOOK_REAUTHORIZATION_SCHEMA: &str = "kiana.hook-reauthorization.v1";
pub const MAX_HOOK_RECURSION_DEPTH: u8 = 2;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookReauthorizationMaterial {
    pub schema: String,
    pub original_request_id: RequestId,
    pub reauthorized_request_id: RequestId,
    pub original_args_digest: String,
    pub updated_args_digest: String,
    pub original_scope_digest: String,
    pub updated_scope_digest: String,
    pub recursion_depth: u8,
    pub approval_invalidated: bool,
    pub broker_not_called: bool,
    pub material_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HookReauthorizationResult {
    pub request: CapabilityRequest,
    pub material: HookReauthorizationMaterial,
}

pub fn reauthorize_hook_update(
    original: &CapabilityRequest,
    updated_arguments: Value,
    recursion_depth: u8,
) -> Result<HookReauthorizationResult, String> {
    if recursion_depth >= MAX_HOOK_RECURSION_DEPTH {
        return Err("hook_reauthorization_recursion_limit".to_owned());
    }
    if !updated_arguments.is_object() {
        return Err("hook_updated_input_object_required".to_owned());
    }
    let original_args_digest = json_digest(&original.arguments);
    let updated_args_digest = json_digest(&updated_arguments);
    let original_scope_digest = scope_digest(original);
    let mut request = original.clone();
    request.request_id = RequestId::new();
    request.arguments = updated_arguments;
    request.execution_scope = None;
    request.cell_id = None;
    request.capability_grant_id = None;
    request.budget_lease_id = None;
    let updated_scope_digest = scope_digest(&request);
    let mut material = HookReauthorizationMaterial {
        schema: HOOK_REAUTHORIZATION_SCHEMA.to_owned(),
        original_request_id: original.request_id,
        reauthorized_request_id: request.request_id,
        original_args_digest,
        updated_args_digest,
        original_scope_digest,
        updated_scope_digest,
        recursion_depth: recursion_depth + 1,
        approval_invalidated: true,
        broker_not_called: true,
        material_digest: String::new(),
    };
    material.material_digest = json_digest(&serde_json::to_value(&material).unwrap_or(Value::Null));
    Ok(HookReauthorizationResult { request, material })
}

fn scope_digest(request: &CapabilityRequest) -> String {
    json_digest(&serde_json::json!({
        "capability": request.capability,
        "operation": request.operation,
        "risk": request.risk,
        "paths": capability_request_paths(request),
        "arguments": request.arguments,
    }))
}

//! Server-owned contracts for the operations admitted by the product composition root.
use crate::{
    CapabilityErrorCode, CapabilityExecutionState, CapabilityKind, CapabilityRequest,
    CapabilityResult, DynamicRiskRule, RequestId, RiskLevel, ACTION_OPERATIONS,
};
use serde_json::{json, Value};
use std::collections::HashSet;

pub const ACTION_CATALOG_SCHEMA: &str = "kiana.action-catalog.v1";
pub const ACTION_HANDLER_BINDING_VERSION: &str = "kiana.handler-binding.v1";
pub const ACTION_CATALOG_SCHEMA_VERSION: crate::SchemaVersion = crate::SchemaVersion::new(1, 0);

#[derive(Clone, Debug, serde::Serialize)]
pub struct CapabilityActionDescriptor {
    pub operation: &'static str,
    pub binding_version: &'static str,
    pub capability: CapabilityKind,
    pub minimum_risk: RiskLevel,
    pub argument_schema: Value,
    pub result_schema: Value,
    pub resource_fields: &'static [&'static str],
    pub effect: &'static str,
    pub cancellation: &'static str,
    pub reconciliation: &'static str,
    pub idempotency: &'static str,
}

/// Validate the closed catalog before it is used to hash or prepare an action.
///
/// The catalog is a server-owned contract: a missing descriptor, duplicate operation, malformed
/// schema, or unbounded metadata must fail closed rather than be silently treated as a generic
/// read operation.
pub fn validate_action_catalog() -> Result<(), String> {
    if ACTION_OPERATIONS.is_empty() {
        return Err("action_catalog_empty".to_owned());
    }
    let mut operations = HashSet::new();
    for operation in ACTION_OPERATIONS {
        if operation.trim().is_empty() || !operations.insert(*operation) {
            return Err("action_catalog_operation_duplicate_or_empty".to_owned());
        }
        let descriptor = capability_action_descriptor(operation)
            .ok_or_else(|| format!("action_descriptor_missing:{operation}"))?;
        if descriptor.operation != *operation
            || descriptor.binding_version != ACTION_HANDLER_BINDING_VERSION
            || descriptor.effect.trim().is_empty()
            || descriptor.cancellation.trim().is_empty()
            || descriptor.reconciliation.trim().is_empty()
            || descriptor.idempotency.trim().is_empty()
            || descriptor
                .resource_fields
                .iter()
                .any(|field| field.trim().is_empty())
        {
            return Err(format!("action_descriptor_incomplete:{operation}"));
        }
        if descriptor
            .resource_fields
            .iter()
            .collect::<HashSet<_>>()
            .len()
            != descriptor.resource_fields.len()
        {
            return Err(format!("action_resource_duplicate:{operation}"));
        }
        crate::validate_schema_contract(&descriptor.argument_schema)
            .map_err(|error| format!("action_argument_schema_invalid:{operation}:{error}"))?;
        crate::validate_schema_contract(&descriptor.result_schema)
            .map_err(|error| format!("action_result_schema_invalid:{operation}:{error}"))?;
        if descriptor
            .argument_schema
            .get("additionalProperties")
            .is_none()
            || descriptor
                .result_schema
                .get("additionalProperties")
                .is_none()
        {
            return Err(format!("action_schema_boundary_unspecified:{operation}"));
        }
    }
    crate::validate_tool_action_bindings()?;
    Ok(())
}

pub fn capability_action_descriptor(operation: &str) -> Option<CapabilityActionDescriptor> {
    let operation = canonical_action_operation(operation)?;
    let spec = crate::operation_spec(operation)?;
    let mut argument_schema = match spec.model {
        Some(model) => (model.parameters)(),
        None => crate::operator_argument_schema(spec),
    };
    argument_schema["additionalProperties"] = json!(true);
    Some(CapabilityActionDescriptor {
        operation: spec.operation,
        binding_version: spec.binding_version,
        capability: spec.capability.clone(),
        minimum_risk: spec.minimum_risk,
        argument_schema,
        result_schema: crate::action_result_schema(),
        resource_fields: spec.resource_fields,
        effect: spec.effect,
        cancellation: spec.cancellation,
        reconciliation: spec.reconciliation,
        idempotency: spec.idempotency,
    })
}

/// Private fields make the value immutable after normalization. It carries no dispatch authority.
#[derive(Clone, Debug, serde::Serialize)]
pub struct PreparedAction {
    request: CapabilityRequest,
    catalog_digest: String,
    digest: String,
    input_digest: String,
}

impl PreparedAction {
    pub fn new(mut request: CapabilityRequest) -> Result<Self, &'static str> {
        validate_action_catalog().map_err(|_| "action_catalog_invalid")?;
        normalize_capability_action(&mut request)?;
        let input_digest =
            canonical_action_input_digest(&request).map_err(|_| "action_input_digest_invalid")?;
        let action = Self {
            catalog_digest: capability_action_catalog_digest(),
            digest: capability_action_digest(&request),
            input_digest,
            request,
        };
        action.validate()?;
        Ok(action)
    }
    pub fn request(&self) -> &CapabilityRequest {
        &self.request
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn catalog_digest(&self) -> &str {
        &self.catalog_digest
    }

    pub fn input_digest(&self) -> &str {
        &self.input_digest
    }

    /// Verify that a prepared action still matches the current closed catalog and normalized
    /// request. This is a consistency check, not a dispatch permit or authorization decision.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.catalog_digest != capability_action_catalog_digest() {
            return Err("action_catalog_changed");
        }
        let mut request = self.request.clone();
        normalize_capability_action(&mut request)?;
        if request != self.request {
            return Err("prepared_action_not_normalized");
        }
        if self.digest != capability_action_digest(&self.request) {
            return Err("prepared_action_digest_mismatch");
        }
        if self.input_digest != canonical_action_input_digest(&self.request)? {
            return Err("prepared_action_input_digest_mismatch");
        }
        Ok(())
    }
    pub fn into_request(self) -> CapabilityRequest {
        self.request
    }
}

pub fn canonical_action_operation(operation: &str) -> Option<&'static str> {
    crate::model_operation_spec(operation)
        .or_else(|| crate::operation_spec(operation))
        .map(|spec| spec.operation)
}

/// Exact operation and capability class, plus the minimum effect for these arguments.
pub fn operator_only_action(operation: &str) -> bool {
    canonical_action_operation(operation)
        .and_then(crate::operation_spec)
        .is_some_and(|spec| spec.operator_only)
}

pub fn capability_action_contract(request: &CapabilityRequest) -> Result<RiskLevel, &'static str> {
    let operation =
        canonical_action_operation(&request.operation).ok_or("action_operation_unknown")?;
    if operator_only_action(operation)
        && (request.cell_id.is_some() || request.arguments["operator_authorized"] != true)
    {
        return Err("action_operator_required");
    }
    let spec = crate::operation_spec(operation).ok_or("action_operation_unknown")?;
    let minimum = capability_action_minimum_risk(operation, &request.arguments)?;
    if request.capability != spec.capability {
        return Err("action_capability_mismatch");
    }
    if risk_rank(request.risk) < risk_rank(minimum) {
        return Err("action_risk_downgrade");
    }
    Ok(minimum)
}

/// Compute the argument-dependent floor from the closed operation specification.
///
/// Request adapters use this when constructing a request; the full contract below additionally
/// checks operator authority, capability kind, and that the supplied risk does not understate it.
pub fn capability_action_minimum_risk(
    operation: &str,
    arguments: &Value,
) -> Result<RiskLevel, &'static str> {
    let operation = canonical_action_operation(operation).ok_or("action_operation_unknown")?;
    let spec = crate::operation_spec(operation).ok_or("action_operation_unknown")?;
    let minimum = match spec.risk_rule {
        DynamicRiskRule::Static => spec.minimum_risk,
        DynamicRiskRule::SandboxMode => match arguments["sandbox"].as_str() {
            Some("workspace-write") => RiskLevel::LocalWrite,
            Some("read-only") | None => RiskLevel::ReadOnly,
            _ => return Err("sandbox_unsupported"),
        },
        DynamicRiskRule::WorkspaceTransactionAction => match arguments["action"].as_str() {
            Some("list" | "inspect") => RiskLevel::ReadOnly,
            Some("recover" | "rollback") => RiskLevel::Critical,
            _ => return Err("workspace_transaction_action_invalid"),
        },
        DynamicRiskRule::ListOrInspectExternal => {
            if matches!(arguments["action"].as_str(), Some("list" | "inspect")) {
                RiskLevel::ReadOnly
            } else {
                RiskLevel::ExternalSideEffect
            }
        }
        DynamicRiskRule::ListExternal => {
            if arguments["action"] == "list" {
                RiskLevel::ReadOnly
            } else {
                RiskLevel::ExternalSideEffect
            }
        }
        DynamicRiskRule::ConnectorInvocation => {
            crate::connector_invocation_risk_for_arguments(arguments)?
        }
    };
    Ok(minimum)
}

fn risk_rank(risk: RiskLevel) -> u8 {
    match risk {
        RiskLevel::ReadOnly => 0,
        RiskLevel::LocalWrite => 1,
        RiskLevel::ExternalSideEffect => 2,
        RiskLevel::Critical => 3,
    }
}

/// Normalize optional values before hashing or presenting an approval. JSON is already
/// decoded here; duplicate-key rejection belongs at the versioned wire parser.
pub fn normalize_capability_action(request: &mut CapabilityRequest) -> Result<(), &'static str> {
    crate::validate_json_limits(&request.arguments).map_err(|_| "action_json_limit")?;
    request.operation = canonical_action_operation(&request.operation)
        .ok_or("action_operation_unknown")?
        .to_owned();
    let arguments = request
        .arguments
        .as_object_mut()
        .ok_or("action_arguments_object_required")?;
    arguments.retain(|_, value| !value.is_null());
    let path_keys: &[&str] = if request.operation.starts_with("context.") {
        &["root", "cache", "source", "store"]
    } else if request.operation == "apply_patch" {
        &["path"]
    } else if request.operation == "local.package" {
        &["destination"]
    } else {
        &[]
    };
    for &key in path_keys {
        if let Some(path) = arguments.get(key) {
            let path = path
                .as_str()
                .filter(|path| !path.trim().is_empty())
                .ok_or("action_path_invalid")?;
            let normalized = crate::normalize_role_path(path).ok_or("action_path_invalid")?;
            arguments.insert(key.to_owned(), json!(normalized));
        }
    }
    if matches!(request.operation.as_str(), "shell.exec" | "process.start") {
        arguments.entry("workdir").or_insert(json!("."));
        if !arguments.get("command").is_some_and(|command| {
            command
                .as_str()
                .is_some_and(|text| !text.trim().is_empty() && !text.contains('\0'))
                || command.as_array().is_some_and(|parts| {
                    !parts.is_empty()
                        && parts[0]
                            .as_str()
                            .is_some_and(|text| !text.trim().is_empty())
                        && parts
                            .iter()
                            .all(|part| part.as_str().is_some_and(|text| !text.contains('\0')))
                })
        }) {
            return Err("action_command_required");
        }
        if arguments["workdir"]
            .as_str()
            .is_none_or(|path| path.is_empty() || path.contains('\0'))
        {
            return Err("action_workdir_invalid");
        }
    }
    if request.operation == "mcp.call" {
        if arguments
            .get("tool")
            .zip(arguments.get("tool_name"))
            .is_some_and(|(tool, alias)| tool != alias)
        {
            return Err("action_tool_alias_conflict");
        }
        if !arguments.contains_key("tool") {
            if let Some(tool) = arguments.remove("tool_name") {
                arguments.insert("tool".to_owned(), tool);
            }
        }
        arguments.remove("tool_name");
        arguments.entry("arguments").or_insert(json!({}));
        if !arguments["arguments"].is_object() {
            return Err("action_tool_arguments_object_required");
        }
        require_text(arguments, "tool")?;
    }
    let arguments = request.arguments.as_object_mut().expect("checked object");
    for key in [
        "timeout_ms",
        "limit",
        "max_tokens",
        "max_bytes_per_file",
        "max_snippet_lines",
    ] {
        if arguments
            .get(key)
            .is_some_and(|value| value.as_u64().is_none_or(|number| number == 0))
        {
            return Err("action_numeric_argument_invalid");
        }
    }
    let descriptor =
        capability_action_descriptor(&request.operation).ok_or("action_operation_unknown")?;
    crate::validate_schema_value(&request.arguments, &descriptor.argument_schema)
        .map_err(|_| "action_arguments_invalid")?;
    let arguments = request.arguments.as_object_mut().expect("checked object");
    match request.operation.as_str() {
        "local.package" => {
            uuid::Uuid::parse_str(
                arguments["package_id"]
                    .as_str()
                    .ok_or("package_id_required")?,
            )
            .map_err(|_| "package_id_invalid")?;
        }
        "workspace.transaction" => {
            if arguments["action"] != "list" {
                require_text(arguments, "transaction_id")?;
            }
            if arguments["action"] == "recover" {
                require_text(arguments, "resolution")?;
            }
        }
        "apply_patch" => require_text(arguments, "patch")?,
        "memory.search" => require_text(arguments, "query")?,
        "memory.write" => {
            require_text(arguments, "collection")?;
            require_text(arguments, "text")?;
            require_text(arguments, "source")?;
        }
        "memory.review" | "data.governance" | "extension.manage" | "connector.manage" => {
            require_text(arguments, "action")?
        }
        "workspace.checkpoint.restore"
            if !arguments.get("snapshot").is_some_and(Value::is_object) =>
        {
            return Err("action_snapshot_required")
        }
        _ => {}
    }
    if let Some(timeout) = arguments.get("timeout_ms").and_then(Value::as_u64) {
        arguments.insert("timeout_ms".to_owned(), json!(timeout.min(60_000)));
    }
    capability_action_contract(request)?;
    Ok(())
}

fn require_text(arguments: &serde_json::Map<String, Value>, key: &str) -> Result<(), &'static str> {
    if arguments
        .get(key)
        .and_then(Value::as_str)
        .is_none_or(|value| value.trim().is_empty())
    {
        return Err("action_required_argument_missing");
    }
    Ok(())
}

pub fn capability_action_digest(request: &CapabilityRequest) -> String {
    crate::json_digest(&json!({"catalog":capability_action_catalog_digest(),"request":request}))
}

/// Digest only the execution-affecting normalized input and its descriptor binding. Request and
/// correlation IDs are deliberately excluded so equivalent calls can be compared across ingress
/// retries while a changed command/path/server/schema still produces a new digest.
pub fn canonical_action_input_digest(request: &CapabilityRequest) -> Result<String, &'static str> {
    let descriptor =
        capability_action_descriptor(&request.operation).ok_or("action_operation_unknown")?;
    Ok(crate::json_digest(&json!({
        "operation": descriptor.operation,
        "binding_version": descriptor.binding_version,
        "capability": request.capability,
        "arguments": request.arguments,
    })))
}

pub fn capability_action_catalog_digest() -> String {
    // Both descriptors and parser semantics belong to the contract. A validator change must
    // invalidate a previously prepared action even if its outward JSON schema stayed equal.
    crate::json_digest(&json!({"schema":ACTION_CATALOG_SCHEMA,
        "contracts":ACTION_OPERATIONS.iter().map(|operation|capability_action_descriptor(operation).expect("closed catalog")).collect::<Vec<_>>(),
        "operation_specs_source":include_str!("operation_catalog.rs"),
        "tool_catalog_digest": crate::tool_catalog_digest(),
        "normalizer_source":include_str!("actions.rs"),"schema_validator_source":include_str!("tool_catalog.rs")}))
}

/// A handler cannot report success for the wrong request or for an unknown/cancelled effect.
pub fn normalize_capability_result(
    expected: RequestId,
    mut result: CapabilityResult,
) -> CapabilityResult {
    if result.request_id != expected {
        let mut failed = CapabilityResult::failure_with_code(
            expected,
            CapabilityErrorCode::ResultUnknown,
            Some("capability_result_mismatch"),
        );
        failed.evidence_refs = result.evidence_refs;
        return failed;
    }
    if crate::validate_json_limits(&result.output).is_err()
        || result.evidence_refs.len() > 256
        || result
            .evidence_refs
            .iter()
            .any(|reference| reference.len() > 4096)
    {
        return CapabilityResult::failure_with_code(
            expected,
            CapabilityErrorCode::ResultUnknown,
            Some("capability_output_limit"),
        );
    }
    // A handler cannot claim success while reporting a non-zero subprocess exit or an
    // unconfirmed effect. Preserve the process evidence, but force the stable failure code
    // before any adapter sees the result.
    let nonzero_exit = result
        .output
        .get("exit_code")
        .and_then(Value::as_i64)
        .is_some_and(|code| code != 0);
    if result.success && nonzero_exit && result.output.get("cancelled") != Some(&Value::Bool(true))
    {
        result.success = false;
        if !result.output.is_object() {
            result.output = json!({"detail": result.output});
        }
        result.output["error"] = json!("execution_failed:shell_exit");
    }
    if result.success && result.output.get("effect_known") == Some(&Value::Bool(false)) {
        result.success = false;
        if !result.output.is_object() {
            result.output = json!({"detail": result.output});
        }
        result.output["error"] = json!("result_unknown:effect_unconfirmed");
    }
    if result.success {
        if let Some(code) = result
            .output
            .get("error_code")
            .and_then(Value::as_str)
            .map(CapabilityErrorCode::from_reason)
        {
            result.success = false;
            if !result.output.is_object() {
                result.output = json!({"detail": result.output});
            }
            result.output["error"] = json!(code.as_str());
        }
    }
    let reason = result
        .output
        .get("error")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let cancelled = result.output["cancelled"] == true;
    if cancelled {
        let confirmed =
            result.output["stop_confirmed"] == true || result.output["not_executed"] == true;
        result.success = false;
        result.output["error"] = json!(if confirmed {
            "cancelled:handler_stopped"
        } else {
            "result_unknown:cancel_stop_unconfirmed"
        });
    } else if result.success && reason.as_deref().is_some_and(|reason| !reason.is_empty()) {
        result.success = false;
        result.output["error"] = json!("result_unknown:capability_success_error_conflict");
    }
    if !result.success {
        if !result.output.is_object() {
            result.output = json!({"detail":result.output});
        }
        if result
            .output
            .get("error")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            result.output["error"] = json!("execution_failed:capability_failed");
        }
        let code = result
            .failure_code()
            .unwrap_or(CapabilityErrorCode::ExecutionFailed);
        result.output["error_code"] = json!(code);
        result.output["requires_reconciliation"] = json!(code.policy().requires_reconciliation);
        result.output["retryable"] = json!(code.policy().retryable);
    }
    if result.output.is_object() {
        let dimensions = result.dimensions();
        result.output["outcome"] = json!({
            "schema":crate::CAPABILITY_OUTCOME_SCHEMA,
            "capability_succeeded":result.success,
            "process_state":dimensions.process,
            "process_exit_code":dimensions.exit_code,
            "stop_state":dimensions.stop,
            "effect_state":dimensions.effect,
            "failure_code":dimensions.failure_code,
            "effect_committed":result.output.get("effect_committed"),
            "execution_status":match dimensions.execution_state() {
                CapabilityExecutionState::Succeeded => "succeeded",
                CapabilityExecutionState::Cancelled => "cancelled",
                CapabilityExecutionState::Unknown => "result_unknown",
                _ => "failed",
            },
            "retry_policy":"no_automatic_effect_retry"
        });
    }
    result
}

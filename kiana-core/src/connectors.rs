//! Operator commands resolve a persisted local binding and re-enter normal authorization.

use super::*;
use kiana_domain::{
    connector_bindings, ConnectorBindingSnapshot, CONNECTOR_HEALTH_OPERATION,
    CONNECTOR_INVOKE_OPERATION, CONNECTOR_MANAGE_OPERATION, CONNECTOR_STREAM,
};

impl ControlPlane {
    pub(crate) async fn handle_connector_command(
        &self,
        context: RequestContext,
        intent: CommandIntent,
    ) -> Result<CoreResponse, CoreError> {
        let normalized = self.normalize_connector_command(&context, &intent).await;
        let (arguments, risk) = match normalized {
            Ok(value) => value,
            Err(reason) => {
                self.append_event(
                    context.request_id,
                    1,
                    "request.accepted",
                    json!({"command":intent.name}),
                )
                .await?;
                self.append_event(
                    context.request_id,
                    2,
                    "command.rejected",
                    json!({"reason":reason}),
                )
                .await?;
                return Ok(CoreResponse::blocked(context.request_id, reason));
            }
        };
        let request = CapabilityRequest::new(
            context.request_id,
            CapabilityKind::Tool,
            intent.name,
            arguments,
        )
        .with_risk(risk);
        self.authorize_and_execute(&context, request).await
    }

    async fn normalize_connector_command(
        &self,
        context: &RequestContext,
        intent: &CommandIntent,
    ) -> Result<(Value, RiskLevel), String> {
        if context.cell_id.is_some()
            || context
                .actor_id
                .as_deref()
                .is_none_or(|s| s.trim().is_empty())
        {
            return Err("connector_operator_required".to_owned());
        }
        // Resolve no configuration or local fixture before ProjectTrust succeeds.
        if !context.project_trusted {
            return Err("project_untrusted".to_owned());
        }
        let mut arguments = intent
            .arguments
            .as_object()
            .cloned()
            .ok_or("connector_arguments_invalid")?;
        let risk = if intent.name == CONNECTOR_HEALTH_OPERATION {
            if arguments.keys().any(|key| key != "binding_id") {
                return Err("connector_health_arguments_invalid".to_owned());
            }
            let binding_id = arguments
                .get("binding_id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or("connector_binding_id_required")?
                .to_owned();
            let history = self
                .events
                .read_stream(CONNECTOR_STREAM, &context.project_root)
                .await
                .map_err(|e| format!("connector_registry_unavailable:{e}"))?;
            let (_, bindings) = connector_bindings(&history).map_err(str::to_owned)?;
            let binding = bindings
                .get(&binding_id)
                .ok_or("connector_binding_missing")?;
            if binding.project_root != context.project_root {
                return Err("connector_binding_scope_mismatch".to_owned());
            }
            if binding.status != "active" {
                return Err("connector_binding_revoked".to_owned());
            }
            arguments.insert("binding_snapshot".to_owned(), json!(binding));
            arguments.insert("binding_authorized".to_owned(), json!(true));
            arguments.insert("probe_kind".to_owned(), json!("read_only"));
            RiskLevel::ReadOnly
        } else if intent.name == CONNECTOR_INVOKE_OPERATION {
            if arguments.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "binding_id" | "operation" | "payload" | "idempotency_key"
                )
            }) || !arguments.contains_key("payload")
            {
                return Err("connector_arguments_invalid".to_owned());
            }
            let binding_id = arguments
                .get("binding_id")
                .and_then(Value::as_str)
                .ok_or("connector_binding_id_required")?
                .to_owned();
            let operation = arguments
                .get("operation")
                .and_then(Value::as_str)
                .ok_or("connector_operation_required")?
                .to_owned();
            let history = self
                .events
                .read_stream(CONNECTOR_STREAM, &context.project_root)
                .await
                .map_err(|e| format!("connector_registry_unavailable:{e}"))?;
            let (_, bindings) = connector_bindings(&history).map_err(str::to_owned)?;
            let binding = bindings
                .get(&binding_id)
                .ok_or("connector_binding_missing")?;
            if binding.project_root != context.project_root {
                return Err("connector_binding_scope_mismatch".to_owned());
            }
            let risk = binding.operation(&operation).map_err(str::to_owned)?.risk();
            arguments.insert("binding_snapshot".to_owned(), json!(binding));
            arguments.insert("binding_authorized".to_owned(), json!(true));
            risk
        } else if intent.name == CONNECTOR_MANAGE_OPERATION {
            if arguments.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "action"
                        | "definition"
                        | "binding"
                        | "binding_id"
                        | "expected_registry_version"
                        | "idempotency_key"
                        | "reason"
                        | "invocation_event_id"
                        | "receipt_path"
                        | "receipt_sha256"
                )
            }) {
                return Err("connector_arguments_invalid".to_owned());
            }
            let action = arguments
                .get("action")
                .and_then(Value::as_str)
                .ok_or("connector_action_invalid")?;
            if !matches!(action, "list" | "bind" | "revoke" | "reconcile") {
                return Err("connector_action_invalid".to_owned());
            }
            if action == "list" {
                RiskLevel::ReadOnly
            } else {
                if arguments
                    .get("expected_registry_version")
                    .and_then(Value::as_u64)
                    .is_none()
                    || arguments
                        .get("reason")
                        .and_then(Value::as_str)
                        .is_none_or(|s| s.trim().is_empty() || s.len() > 4096)
                {
                    return Err("connector_mutation_fields_required".to_owned());
                }
                if action == "bind" {
                    let snapshot = ConnectorBindingSnapshot {
                        definition: serde_json::from_value(
                            arguments.get("definition").cloned().unwrap_or(Value::Null),
                        )
                        .map_err(|_| "connector_definition_invalid")?,
                        binding: serde_json::from_value(
                            arguments.get("binding").cloned().unwrap_or(Value::Null),
                        )
                        .map_err(|_| "connector_binding_invalid")?,
                        project_root: context.project_root.clone(),
                        revision: 0,
                        status: "active".to_owned(),
                    };
                    snapshot.validate().map_err(str::to_owned)?;
                }
                RiskLevel::ExternalSideEffect
            }
        } else {
            return Err("connector_command_invalid".to_owned());
        };
        if intent.name == CONNECTOR_INVOKE_OPERATION || risk != RiskLevel::ReadOnly {
            if arguments
                .get("idempotency_key")
                .and_then(Value::as_str)
                .is_none_or(|s| {
                    s.trim().is_empty() || s.len() > 128 || s.chars().any(char::is_control)
                })
            {
                return Err("connector_idempotency_key_required".to_owned());
            }
        }
        if serde_json::to_vec(&arguments)
            .map_err(|_| "connector_arguments_invalid")?
            .len()
            > 128 * 1024
        {
            return Err("connector_payload_too_large".to_owned());
        }
        arguments.insert("operator_authorized".to_owned(), json!(true));
        arguments.insert("project_root".to_owned(), json!(context.project_root));
        arguments.insert("actor_id".to_owned(), json!(context.actor_id));
        arguments.insert("role_id".to_owned(), json!(context.role_id));
        arguments.insert("department_id".to_owned(), json!(context.department_id));
        Ok((Value::Object(arguments), risk))
    }
}

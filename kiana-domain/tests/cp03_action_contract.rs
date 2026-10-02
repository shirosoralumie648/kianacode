use kiana_domain::{
    capability_action_contract, capability_action_descriptor, capability_action_digest,
    normalize_capability_action, operator_only_action, parse_bounded_json, validate_action_catalog,
    CapabilityKind, CapabilityRequest, PreparedAction, RequestId, RiskLevel, ACTION_CATALOG_SCHEMA,
    ACTION_OPERATIONS,
};
use serde_json::json;

#[test]
fn action_catalog_has_complete_server_owned_descriptors() {
    validate_action_catalog().unwrap();
    assert!(!ACTION_OPERATIONS.is_empty());
    for operation in ACTION_OPERATIONS {
        let descriptor = capability_action_descriptor(operation).expect("catalog descriptor");
        assert_eq!(descriptor.operation, *operation);
        assert_eq!(descriptor.binding_version, "kiana.handler-binding.v1");
        assert!(!descriptor
            .resource_fields
            .iter()
            .any(|field| field.is_empty()));
        assert!(!descriptor.effect.is_empty());
        assert!(descriptor
            .argument_schema
            .get("additionalProperties")
            .is_some());
        assert!(descriptor
            .result_schema
            .get("additionalProperties")
            .is_some());
    }
    assert_eq!(ACTION_CATALOG_SCHEMA, "kiana.action-catalog.v1");
}

#[test]
fn forged_readonly_risk_cannot_downgrade_registered_effect() {
    let cases = [
        (
            CapabilityKind::Filesystem,
            "apply_patch",
            json!({"patch":"*** Begin Patch\n*** Add File: x\n+ok\n*** End Patch\n"}),
        ),
        (
            CapabilityKind::Network,
            "mcp.call",
            json!({"tool":"echo","arguments":{}}),
        ),
        (
            CapabilityKind::Filesystem,
            "memory.write",
            json!({"collection":"project","text":"fact","source":"user"}),
        ),
    ];
    for (capability, operation, arguments) in cases {
        let mut request =
            CapabilityRequest::new(RequestId::new(), capability, operation, arguments);
        assert_eq!(
            normalize_capability_action(&mut request).unwrap_err(),
            "action_risk_downgrade"
        );
    }
}

#[test]
fn spec_kind_drives_policy_for_preview_and_mcp_handshake() {
    let cases = [
        (CapabilityKind::Filesystem, "apply_patch.preview"),
        (CapabilityKind::Tool, "connector.mcp_handshake"),
    ];
    for (kind, operation) in cases {
        let descriptor = capability_action_descriptor(operation).unwrap();
        assert_eq!(descriptor.capability, kind);
        assert_eq!(descriptor.minimum_risk, RiskLevel::ReadOnly);
        assert!(!operator_only_action(operation));

        let request = CapabilityRequest::new(RequestId::new(), kind, operation, json!({}))
            .with_risk(RiskLevel::ReadOnly);
        assert_eq!(
            capability_action_contract(&request),
            Ok(RiskLevel::ReadOnly)
        );

        let wrong_kind = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Query,
            operation,
            json!({}),
        )
        .with_risk(RiskLevel::ReadOnly);
        assert_eq!(
            capability_action_contract(&wrong_kind),
            Err("action_capability_mismatch")
        );
    }
}

#[test]
fn spec_risk_rules_preserve_sandbox_and_operator_action_floors() {
    let workspace_write = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Process,
        "process.start",
        json!({"operator_authorized":true,"sandbox":"workspace-write"}),
    )
    .with_risk(RiskLevel::LocalWrite);
    assert_eq!(
        capability_action_contract(&workspace_write),
        Ok(RiskLevel::LocalWrite)
    );

    let forged_readonly = workspace_write.clone().with_risk(RiskLevel::ReadOnly);
    assert_eq!(
        capability_action_contract(&forged_readonly),
        Err("action_risk_downgrade")
    );

    let transaction_read = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Filesystem,
        "workspace.transaction",
        json!({"operator_authorized":true,"action":"inspect"}),
    )
    .with_risk(RiskLevel::ReadOnly);
    assert_eq!(
        capability_action_contract(&transaction_read),
        Ok(RiskLevel::ReadOnly)
    );

    let transaction_recovery = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Filesystem,
        "workspace.transaction",
        json!({"operator_authorized":true,"action":"rollback"}),
    )
    .with_risk(RiskLevel::Critical);
    assert_eq!(
        capability_action_contract(&transaction_recovery),
        Ok(RiskLevel::Critical)
    );
}

#[test]
fn cp_preview_and_execution_use_identical_action() {
    let request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Process,
        "shell.exec",
        json!({"command":"printf ok","sandbox":"read-only"}),
    );
    let prepared = PreparedAction::new(request.clone()).unwrap();
    assert_eq!(prepared.request().operation, "shell.exec");
    assert_eq!(prepared.request().arguments["workdir"], ".");
    assert_eq!(
        prepared.catalog_digest(),
        kiana_domain::capability_action_catalog_digest()
    );
    assert_eq!(
        prepared.digest(),
        capability_action_digest(prepared.request())
    );
    prepared.validate().unwrap();
    assert_ne!(prepared.request(), &request);
}

#[test]
fn duplicate_json_and_invalid_numeric_arguments_fail_closed() {
    let duplicate = br#"{"command":"printf ok","command":"rm -rf /"}"#;
    assert!(parse_bounded_json(duplicate).is_err());
    let mut request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Process,
        "shell.exec",
        json!({"command":"printf ok","timeout_ms":0}),
    )
    .with_risk(RiskLevel::ReadOnly);
    assert_eq!(
        normalize_capability_action(&mut request).unwrap_err(),
        "action_numeric_argument_invalid"
    );
}

use kiana_domain::{
    CapabilityKind, CapabilityRequest, ExtensionEffect, ExtensionExecutionContract, RequestId,
    RiskLevel,
};
use serde_json::json;
use std::collections::BTreeSet;

#[test]
fn skill_allowed_tools_cannot_grant_shell() {
    let request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Process,
        "shell.exec",
        json!({"role_id": "builder", "command": "echo fixture"}),
    );
    let contract = ExtensionExecutionContract {
        extension_id: "review-skill".to_owned(),
        package_sha256: "a".repeat(64),
        content_hash: "b".repeat(64),
        effect: ExtensionEffect::ReadOnly,
        required_capabilities: BTreeSet::from(["shell.exec".to_owned()]),
        network_policy: kiana_domain::ExtensionNetworkPolicy::Deny,
        supported_roles: BTreeSet::from(["builder".to_owned()]),
        handler_effect: ExtensionEffect::ReadWrite,
    };
    assert_eq!(
        contract.check(&request),
        Err("extension_read_only_write_denied")
    );

    let query = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Query,
        "memory.search",
        json!({"role_id": "builder"}),
    );
    let read_contract = ExtensionExecutionContract {
        required_capabilities: BTreeSet::from(["memory.search".to_owned()]),
        handler_effect: ExtensionEffect::ReadOnly,
        ..contract
    };
    assert!(read_contract.check(&query).is_ok());
    assert_eq!(request.risk, RiskLevel::ReadOnly);
}

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
        publisher: "fixture-publisher".to_owned(),
        package_sha256: "a".repeat(64),
        content_hash: "b".repeat(64),
        registry_generation: 1,
        role_id: "builder".to_owned(),
        effect: ExtensionEffect::ReadOnly,
        required_capabilities: BTreeSet::from(["shell.exec".to_owned()]),
        network_policy: kiana_domain::ExtensionNetworkPolicy::Deny,
        supported_roles: BTreeSet::from(["builder".to_owned()]),
        capability_diff_digest: "sha256:".to_owned() + &"c".repeat(64),
        network_policy_digest: "sha256:".to_owned() + &"d".repeat(64),
        secret_policy_digest: "sha256:".to_owned() + &"e".repeat(64),
        issued_at_unix_ms: 1,
        expires_at_unix_ms: 2,
        scope_digest: "sha256:".to_owned() + &"f".repeat(64),
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

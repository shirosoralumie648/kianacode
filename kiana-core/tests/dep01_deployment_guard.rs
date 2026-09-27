#[test]
fn dep01_deployment_contract_stays_pure_and_deny_first() {
    let domain = include_str!("../../kiana-domain/src/deployment.rs");
    let ids = include_str!("../../kiana-domain/src/ids.rs");
    for marker in [
        "DeploymentProfile",
        "EmbeddedLocal",
        "ManagedLocal",
        "Container",
        "Orchestrated",
        "EnvironmentProfile",
        "DeploymentRevision",
        "StorageRootId",
        "InstanceId",
        "project_trusted",
        "deployment_project_untrusted",
        "deployment_project_root_mismatch",
        "profile_digest",
        "absolute_path",
    ] {
        assert!(
            domain.contains(marker) || ids.contains(marker),
            "DEP-01 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::process::Command",
        "tokio::",
        "CapabilityBroker",
        "EventStore::append",
        "read_project_trust",
        "SecretValue",
    ] {
        assert!(
            !domain.contains(forbidden),
            "DEP-01 deployment domain crossed an adapter boundary: {forbidden}"
        );
    }
}

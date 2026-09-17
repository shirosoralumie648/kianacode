#[test]
fn provider_route_admission_is_fenced_before_network_and_stays_actor_blind() {
    let domain = include_str!("../../kiana-domain/src/model.rs");
    let provider = include_str!("../../kiana-provider/src/lib.rs");
    let config = include_str!("../../kiana-provider/src/config.rs");
    let transport = include_str!("../../kiana-provider/src/transport.rs");
    let budget = include_str!("../src/model_budget.rs");
    let daemon = include_str!("../../kiana-daemon/src/model_client.rs");
    for marker in [
        "route_digest: Option<String>",
        "configuration_revision: Option<String>",
        "authority_revision: Option<String>",
        "credential_revision: Option<String>",
        "provider_account: Option<String>",
        "validate_for_prepared",
        "model_route_admission_drift",
        "model_authority_revision_drift",
        "model_route_admission_missing",
    ] {
        assert!(
            domain.contains(marker),
            "model binding marker missing: {marker}"
        );
    }
    for marker in [
        "permit.validate_for_prepared",
        "current_revision",
        "model_credential_revision_changed",
        "consume_prepared",
    ] {
        assert!(
            provider.contains(marker),
            "gateway admission marker missing: {marker}"
        );
    }
    assert!(
        provider.find("permit.validate_for_prepared") < provider.find("consume_prepared"),
        "provider must validate route binding before budget consume"
    );
    assert!(config.contains("provider_account"));
    assert!(config.contains("credential_revision"));
    assert!(!config.contains("credential: Option<String>"));
    for marker in [
        "x-kiana-provider-account",
        "model_credential_revision_changed",
        "credential_store.issue",
    ] {
        assert!(
            transport.contains(marker),
            "transport binding marker missing: {marker}"
        );
    }
    for marker in [
        "route_digest: Some(prepared.route.digest())",
        "configuration_revision: Some(prepared.route.configuration_revision.clone())",
        "credential_revision: prepared.credential_revision.clone()",
        "provider_account: prepared.provider_account.clone()",
        "authority_revision",
    ] {
        assert!(
            budget.contains(marker),
            "budget permit marker missing: {marker}"
        );
    }
    assert!(daemon.contains("ProviderGateway"));
    assert!(!provider.contains("actor_id"));
    assert!(!transport.contains("actor_id"));
}

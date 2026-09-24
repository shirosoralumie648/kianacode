use std::fs;

#[test]
fn provider_recovery_reuses_history_resume_and_single_execution_spine() {
    let recovery = fs::read_to_string("../kiana-domain/src/provider_recovery.rs")
        .expect("provider recovery source");
    for marker in [
        "PROVIDER_RECOVERY_SCHEMA",
        "ProviderResumeBinding",
        "PROVIDER_RESUME_BINDING_SCHEMA",
        "validate_for_resume",
        "ProtectedReplayMaterial",
        "missing_reasoning_material_blocks_resume",
        "stale_route_authority_cannot_resume",
        "ProviderHistoryFact",
        "ProviderHistoryProjection",
        "validate_model_history",
        "ProviderInFlightPhase",
        "ProviderReconciliationCase",
        "ReconcileModelUnknown",
        "ReconcileCapabilityUnknown",
        "automatic_retry_allowed",
        "PROVIDER_RECOVERY_NO_MODEL_RESUBMIT",
    ] {
        assert!(
            recovery.contains(marker),
            "P4-J7-27 marker missing: {marker}"
        );
    }
    for forbidden in [
        "tokio::spawn",
        "reqwest::Client",
        "TcpStream",
        "CapabilityBrokerPort",
        "EventStorePort",
        "ControlPlane",
        "std::fs",
    ] {
        assert!(
            !recovery.contains(forbidden),
            "provider recovery contract introduces an authority/effect path: {forbidden}"
        );
    }
}

#[test]
fn provider_recovery_uses_committed_facts_and_explicit_resume_claim() {
    let history = fs::read_to_string("../kiana-core/src/history.rs").expect("history source");
    assert!(history.contains("model_protocol_history"));
    assert!(history.contains("read_all_events"));
    assert!(history.contains("validate_model_history"));

    let recovery = fs::read_to_string("../kiana-core/src/recovery.rs").expect("recovery source");
    for marker in [
        "pub async fn resume_run",
        "run.resume_prepared",
        "self.runner.restore",
        "run_resume_claim_conflict",
        "run_resume_authority_changed",
        "run_resume_data_revoked",
    ] {
        assert!(recovery.contains(marker), "resume marker missing: {marker}");
    }
    let lifecycle = fs::read_to_string("../kiana-core/src/lifecycle.rs").expect("lifecycle source");
    assert!(lifecycle.contains("self.drive_run"));
    assert!(lifecycle.contains("pub(crate) async fn drive_run"));
}

#[test]
fn provider_continuation_is_validated_at_wire_boundary_without_replay_authority() {
    let request = fs::read_to_string("../kiana-provider/src/request.rs").expect("request source");
    for marker in [
        "continuation.validate()",
        "opaque_item_cannot_cross_provider",
        "provider_continuation_unsupported",
    ] {
        assert!(
            request.contains(marker),
            "continuation marker missing: {marker}"
        );
    }
    let replay = fs::read_to_string("../kiana-domain/src/protected_replay.rs")
        .expect("protected replay source");
    assert!(replay.contains("missing_reasoning_material_blocks_resume"));
    assert!(replay.contains("validate_for_call"));
}

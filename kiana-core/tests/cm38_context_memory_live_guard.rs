//! CM-38 source guard for fake Provider context/memory golden path and live opt-in.

#[test]
fn context_memory_golden_path_stays_fake_bounded_and_live_opt_in() {
    let fake = include_str!("../../kiana-daemon/tests/eq10_fake_provider.rs");
    let runtime = include_str!("../../kiana-daemon/src/eval_runtime.rs");
    let memory = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let eventstore = include_str!("../../kiana-domain/src/memory_journal.rs");
    let evidence = include_str!("../../kiana-domain/src/context_memory_evidence.rs");
    let live = include_str!("../../kiana-domain/src/live_handoff.rs");
    let baseline = include_str!("../../docs/roadmap/cm38-context-memory-live-baseline.md");
    for marker in [
        "FakeProviderAdapter",
        "FakeProviderScenario",
        "without_network",
        "KianaHarness",
        "memory.search",
        "memory.write",
        "candidate",
        "receipt",
        "Memory",
        "ContextMemoryGoldenPathEvidence",
        "ContextMemoryStageDigests",
        "context_memory_verified_stages_missing",
        "context_memory_fake_cannot_claim_live",
        "scope_digest",
        "redaction_profile_digest",
        "provider_live_evidence_digest",
        "recovery_digest",
    ] {
        assert!(
            fake.contains(marker)
                || runtime.contains(marker)
                || memory.contains(marker)
                || eventstore.contains(marker)
                || evidence.contains(marker),
            "CM-38 fake path marker missing: {marker}"
        );
    }
    for marker in [
        "LiveHandoffManifest",
        "NotSupported",
        "OptedIn",
        "operator_approval_ref",
        "provider_receipt_ref",
        "cleanup_plan",
    ] {
        assert!(live.contains(marker), "CM-38 live marker missing: {marker}");
    }
    for marker in [
        "context_memory_golden_path_is_durable",
        "live_provider_evidence_keeps_scope_and_redaction",
        "fake",
        "live opt-in",
        "candidate",
        "approval",
        "projection",
        "recovery",
        "partial",
    ] {
        assert!(
            baseline.contains(marker),
            "CM-38 baseline marker missing: {marker}"
        );
    }
    assert!(!runtime.contains("reqwest::"));
    assert!(!runtime.contains("std::net"));
}

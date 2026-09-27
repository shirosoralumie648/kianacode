//! DEP-23 source guard: activation mints a new lease and fence, keeps the old root read-only,
//! and is a source decision rather than an effect.

#[test]
fn dep23_activation_mints_a_new_fence_and_keeps_the_old_root_read_only() {
    let source = include_str!("../src/restore_activation.rs");
    for marker in [
        "RestoreActivationRequest",
        "RestoreActivationReport",
        "RestoreActivationRecord",
        "RestoreActivationStatus",
        "SupersededRootWriter",
        "RootWriteMode",
        "RootWriteMode::ReadOnly",
        "OperationLeaseCas",
        "FenceTokenId",
        "restore_activation_old_writer_not_fenced",
        "restore_activation_old_writer_fence_unconfirmed",
        "restore_activation_old_root_not_read_only",
        "restore_activation_old_root_not_retained",
        "restore_activation_fence_token_reused",
        "restore_activation_instance_identity_reused",
        "restore_authority_epoch_not_advanced",
        "restore_data_epoch_not_advanced",
        "restore_activation_command_admitted_before_ready",
        "restore_activation_explicit_activate_required",
        "restore_activation_root_not_eligible",
        "restore_activation_superseded_root_write",
        "restore_activation_not_ready",
        "restore_activation_identity_digest_conflict",
    ] {
        assert!(
            source.contains(marker),
            "DEP-23 source marker missing: {marker}"
        );
    }
    // The old token is burned, not reissued: the record and the reducer both compare it.
    assert!(
        source.contains("self.new_fence_token == self.superseded_fence_token"),
        "DEP-23 must refuse reissuing the replaced writer's fence token"
    );
    assert!(
        source.contains("request.new_fence_token == request.superseded.fence_token"),
        "DEP-23 must compare the minted token against the replaced one before activating"
    );
    // The old root is retained and read-only in the sealed record, never removed.
    assert!(
        source.contains("superseded_write_mode: RootWriteMode::ReadOnly"),
        "DEP-23 must record the replaced root as read-only"
    );
    assert!(
        source.contains("superseded_retained: true"),
        "DEP-23 must retain the replaced root for audit rather than deleting it"
    );
    assert!(!source.contains("superseded_retained: false"));
}

#[test]
fn dep23_activation_is_a_decision_not_an_effect() {
    let source = include_str!("../src/restore_activation.rs");
    // The lease is minted as a value through the existing CAS; nothing here writes a byte, takes
    // an OS lock, appends a fact or dispatches a capability.
    for forbidden in [
        "std::fs",
        "std::process",
        "std::net",
        "tokio::",
        "PathBuf",
        "remove_dir",
        "remove_file",
        "rename(",
        "EventStorePort",
        "ArtifactStorePort",
        "CapabilityBroker",
        "Command::new",
    ] {
        assert!(
            !source.contains(forbidden),
            "DEP-23 restore activation crossed effect boundary: {forbidden}"
        );
    }
    // A reducer with a fixed decision order, re-derived on every validation.
    assert!(
        source.contains("fn derive(request: &RestoreActivationRequest)"),
        "DEP-23 must derive the activation decision in one fixed-order function"
    );
    assert!(
        source.contains("report.validate_against(request)"),
        "DEP-23 must re-derive the report on validation rather than trust its fields"
    );
}

#[test]
fn dep23_activation_is_explicit_and_never_implicit() {
    let source = include_str!("../src/restore_activation.rs");
    assert!(
        source.contains("if !request.explicit_activate"),
        "DEP-23 must require an explicit activate decision"
    );
    // Both the digest-sealed constructor and the reducer require it; a report cannot present
    // itself as an activation without naming the rule that refused it.
    assert!(
        source.contains("fn may_activate(&self) -> bool"),
        "DEP-23 must expose a single may_activate gate"
    );
    assert!(
        source.contains("if !report.may_activate()"),
        "DEP-23 must refuse to mint a lease for a blocked report"
    );
    // The old root stays auditable: reads are admitted where writes are refused.
    assert!(
        source.contains("fn admit_audit_read_after_activation"),
        "DEP-23 must keep the replaced root readable for audit"
    );
}

#[test]
fn dep23_the_baseline_states_what_is_not_proven() {
    let baseline = include_str!("../../docs/roadmap/dep23-restore-activation-baseline.md");
    for marker in [
        "restore activation",
        "fence",
        "read-only",
        "explicit activate",
        "readiness",
        "source",
    ] {
        assert!(
            baseline.contains(marker),
            "DEP-23 baseline marker missing: {marker}"
        );
    }
    // The baseline must not over-claim. These are the phrasings a promotion would need; the
    // honest limitations section is allowed to say the opposite in a negation.
    for forbidden in [
        "is durable",
        "runs in production",
        "verified end to end",
        "proves activation",
    ] {
        assert!(
            !baseline.contains(forbidden),
            "DEP-23 baseline over-claims: {forbidden}"
        );
    }
    // And it must say plainly what it does not prove.
    assert!(
        baseline.contains("does not"),
        "DEP-23 baseline must state what it does not prove"
    );
}

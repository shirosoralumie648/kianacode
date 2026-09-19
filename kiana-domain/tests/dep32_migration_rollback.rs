use kiana_domain::{
    MigrationCompatibilityWindow, MigrationPrecondition, MigrationRegistry,
    MigrationReleaseBinding, MigrationRollbackAction, MigrationRollbackFacts,
    MigrationRollbackKind, MigrationRollbackReceipt, MigrationRollbackStatus, MigrationStep,
};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const DIGEST_D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn registry() -> MigrationRegistry {
    MigrationRegistry::new(
        MigrationReleaseBinding {
            release_manifest_digest: DIGEST_A.to_owned(),
            artifact_digest: DIGEST_B.to_owned(),
            signature_digest: DIGEST_C.to_owned(),
        },
        vec![MigrationStep {
            step_id: "step-1".to_owned(),
            ordinal: 1,
            from_format_version: 1,
            to_format_version: 2,
            checksum: DIGEST_A.to_owned(),
            owner: "owner".to_owned(),
            backup_required: true,
            precondition: MigrationPrecondition {
                source_schema_digest: DIGEST_A.to_owned(),
                expected_source_revision: 1,
                required_owner: "owner".to_owned(),
                requires_verified_backup: true,
            },
            compatibility_window: MigrationCompatibilityWindow {
                min_reader_version: 1,
                max_reader_version: 2,
                expires_at_unix_ms: 1_900_000_000_000,
            },
            upcaster: "upcaster-1".to_owned(),
        }],
    )
    .unwrap()
}

fn facts(registry: &MigrationRegistry) -> MigrationRollbackFacts {
    MigrationRollbackFacts {
        registry_digest: registry.registry_digest.clone(),
        old_revision_digest: DIGEST_A.to_owned(),
        old_root_digest: DIGEST_B.to_owned(),
        current_revision_digest: DIGEST_C.to_owned(),
        verified_backup: true,
        active_writer_count: 0,
        unknown_effect_count: 0,
        old_revision_compatible: true,
        new_revision_fenced: true,
        old_root_retained: true,
        restore_verified: true,
        external_effect_reconciled: true,
    }
}

#[test]
fn rollback_receipt_allows_only_verified_binary_data_and_effect_paths() {
    let registry = registry();
    let binary = MigrationRollbackReceipt::evaluate(
        &registry,
        MigrationRollbackKind::Binary,
        &facts(&registry),
    )
    .unwrap();
    assert_eq!(binary.status, MigrationRollbackStatus::Allowed);
    assert_eq!(
        binary.action,
        MigrationRollbackAction::StartCompatibleOldRevision
    );
    let data = MigrationRollbackReceipt::evaluate(
        &registry,
        MigrationRollbackKind::Data,
        &facts(&registry),
    )
    .unwrap();
    assert_eq!(data.action, MigrationRollbackAction::RestoreVerifiedRoot);
    let effect = MigrationRollbackReceipt::evaluate(
        &registry,
        MigrationRollbackKind::EffectReconciliation,
        &facts(&registry),
    )
    .unwrap();
    assert_eq!(
        effect.action,
        MigrationRollbackAction::ReconcileExternalEffect
    );
}

#[test]
fn rollback_receipt_blocks_backup_writer_unknown_and_compatibility_failures() {
    let registry = registry();
    let mut missing_backup = facts(&registry);
    missing_backup.verified_backup = false;
    assert_eq!(
        MigrationRollbackReceipt::evaluate(&registry, MigrationRollbackKind::Data, &missing_backup)
            .unwrap()
            .reason,
        "rollback_backup_unverified"
    );
    let mut writer = facts(&registry);
    writer.active_writer_count = 1;
    assert_eq!(
        MigrationRollbackReceipt::evaluate(&registry, MigrationRollbackKind::Data, &writer)
            .unwrap()
            .reason,
        "rollback_writer_active"
    );
    let mut unknown = facts(&registry);
    unknown.unknown_effect_count = 1;
    assert_eq!(
        MigrationRollbackReceipt::evaluate(&registry, MigrationRollbackKind::Data, &unknown)
            .unwrap()
            .reason,
        "rollback_unknown_effects"
    );
    let mut incompatible = facts(&registry);
    incompatible.old_revision_compatible = false;
    assert_eq!(
        MigrationRollbackReceipt::evaluate(&registry, MigrationRollbackKind::Binary, &incompatible)
            .unwrap()
            .reason,
        "rollback_binary_incompatible"
    );
}

#[test]
fn rollback_receipt_blocks_unverified_restore_effect_and_strict_fields() {
    let registry = registry();
    let mut restore = facts(&registry);
    restore.restore_verified = false;
    assert_eq!(
        MigrationRollbackReceipt::evaluate(&registry, MigrationRollbackKind::Data, &restore)
            .unwrap()
            .reason,
        "rollback_restore_unverified"
    );
    let mut effect = facts(&registry);
    effect.external_effect_reconciled = false;
    assert_eq!(
        MigrationRollbackReceipt::evaluate(
            &registry,
            MigrationRollbackKind::EffectReconciliation,
            &effect
        )
        .unwrap()
        .reason,
        "rollback_effect_unreconciled"
    );
    let mut retained = facts(&registry);
    retained.old_root_retained = false;
    let receipt =
        MigrationRollbackReceipt::evaluate(&registry, MigrationRollbackKind::Data, &retained)
            .unwrap();
    assert_eq!(receipt.action, MigrationRollbackAction::Deny);
    let mut value = serde_json::to_value(receipt).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<MigrationRollbackReceipt>(value).is_err());
    assert_ne!(DIGEST_D, DIGEST_A);
}

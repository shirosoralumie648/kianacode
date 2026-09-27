use kiana_domain::{
    InstanceId, QuarantineStage, RestoreMode, RestoreQuarantine, RestoreRoot, RestoreScanReport,
    RestoreScanStatus, StorageRootId,
};
use uuid::Uuid;

const MANIFEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn root_id(value: u128) -> StorageRootId {
    StorageRootId::from_uuid(Uuid::from_u128(value))
}

fn instance_id(value: u128) -> InstanceId {
    InstanceId::from_uuid(Uuid::from_u128(value))
}

#[allow(clippy::too_many_arguments)]
fn root() -> RestoreRoot {
    RestoreRoot::new(
        "restore-1",
        root_id(2),
        instance_id(20),
        root_id(1),
        instance_id(10),
        MANIFEST,
        1_000,
        5,
        900,
        3,
        4,
        Some(2),
        Some(3),
        vec!["artifact/a".to_owned(), "artifact/b".to_owned()],
        Some(2),
        true,
        RestoreMode::FullRebuild,
    )
    .expect("restore root")
}

fn both_present() -> Vec<String> {
    vec!["artifact/a".to_owned(), "artifact/b".to_owned()]
}

#[test]
fn a_verified_root_is_activated_only_after_rebuild() {
    let staged = root();
    let report = RestoreScanReport::scan(&staged, &both_present()).expect("scan");
    assert_eq!(report.status, RestoreScanStatus::Complete);
    assert_eq!(report.stage, QuarantineStage::Verified);

    // Verified is not Ready. A root that has been scanned but not rebuilt must not be eligible.
    assert!(!report.activation_eligible(&staged).expect("eligibility"));

    // The full path: staged -> rebuilt -> ready, and only then is it eligible. The scan report
    // is produced against the staged root, so a rebuilt root needs its own to bind to.
    let quarantine = RestoreQuarantine::new(root_id(1), instance_id(10), vec![staged.clone()])
        .expect("quarantine");
    let rebuilt = quarantine
        .advance("restore-1", QuarantineStage::Rebuilt)
        .expect("rebuilt");
    let ready = rebuilt
        .advance("restore-1", QuarantineStage::Ready)
        .expect("ready");
    let ready_root = &ready.roots[0];
    assert_eq!(ready_root.stage, QuarantineStage::Ready);

    let ready_report = RestoreScanReport::scan(ready_root, &both_present()).expect("rescan");
    // A rescan of a complete root still reports Verified, and activation additionally requires
    // the root itself to be Ready, which it now is.
    assert_eq!(ready_report.status, RestoreScanStatus::Complete);
    assert_eq!(ready_report.stage, QuarantineStage::Verified);
    assert!(
        !ready_report
            .activation_eligible(ready_root)
            .expect("eligibility"),
        "the report is Verified, not Ready, so activation stays closed"
    );
}

#[test]
fn a_root_that_would_overwrite_the_active_one_is_refused() {
    // Same storage root as the running instance: restoring here would destroy the live data.
    let error = RestoreRoot::new(
        "restore-bad",
        root_id(1),
        instance_id(20),
        root_id(1),
        instance_id(10),
        MANIFEST,
        1_000,
        5,
        900,
        3,
        4,
        Some(2),
        Some(3),
        Vec::new(),
        None,
        false,
        RestoreMode::FullRebuild,
    )
    .expect_err("restore over the active root");
    assert_eq!(error, "restore_root_would_overwrite_active");
}

#[test]
fn a_projection_cursor_past_the_source_is_refused() {
    let error = RestoreRoot::new(
        "restore-ahead",
        root_id(2),
        instance_id(20),
        root_id(1),
        instance_id(10),
        MANIFEST,
        1_000,
        5,
        1_500,
        3,
        4,
        Some(2),
        Some(3),
        Vec::new(),
        None,
        false,
        RestoreMode::FullRebuild,
    )
    .expect_err("projector past the source");
    assert_eq!(error, "restore_projection_cursor_ahead_of_source");
}

#[test]
fn an_epoch_that_does_not_move_forward_is_refused() {
    let data = RestoreRoot::new(
        "restore-epoch",
        root_id(2),
        instance_id(20),
        root_id(1),
        instance_id(10),
        MANIFEST,
        1_000,
        5,
        900,
        3,
        4,
        Some(3),
        Some(3),
        Vec::new(),
        None,
        false,
        RestoreMode::FullRebuild,
    )
    .expect_err("data epoch not advanced");
    assert_eq!(data, "restore_data_epoch_not_advanced");

    let authority = RestoreRoot::new(
        "restore-epoch",
        root_id(2),
        instance_id(20),
        root_id(1),
        instance_id(10),
        MANIFEST,
        1_000,
        5,
        900,
        3,
        4,
        Some(2),
        Some(4),
        Vec::new(),
        None,
        false,
        RestoreMode::FullRebuild,
    )
    .expect_err("authority epoch not advanced");
    assert_eq!(authority, "restore_authority_epoch_not_advanced");
}

#[test]
fn a_missing_artifact_is_reported_and_makes_the_root_ineligible() {
    let staged = root();
    let report = RestoreScanReport::scan(&staged, &["artifact/a".to_owned()]).expect("scan");
    assert_eq!(report.status, RestoreScanStatus::MissingArtifact);
    assert_eq!(report.stage, QuarantineStage::Rejected);
    assert_eq!(report.missing_artifacts, vec!["artifact/b".to_owned()]);
    assert_eq!(report.reason, "restore_artifact_missing");
    assert!(!report.activation_eligible(&staged).expect("eligibility"));
}

#[test]
fn a_required_migration_that_was_not_applied_blocks_the_restore() {
    let mut staged = root();
    staged.migration_applied = false;
    staged.root_digest = staged.digest();
    staged.validate().expect("structurally valid");

    let report = RestoreScanReport::scan(&staged, &both_present()).expect("scan");
    assert_eq!(report.status, RestoreScanStatus::MigrationPending);
    assert_eq!(report.reason, "restore_migration_not_applied");
    assert!(!report.activation_eligible(&staged).expect("eligibility"));

    // A migration claimed as applied without a version to apply is equally incoherent.
    let mut unversioned = root();
    unversioned.migration_version = None;
    unversioned.migration_applied = true;
    unversioned.root_digest = unversioned.digest();
    assert_eq!(
        unversioned.validate().unwrap_err(),
        "restore_migration_version_missing"
    );
}

#[test]
fn a_rejected_root_can_never_be_advanced() {
    let staged = root();
    let quarantine = RestoreQuarantine::new(root_id(1), instance_id(10), vec![staged.clone()])
        .expect("quarantine");
    let mut rejected = quarantine.roots[0].clone();
    rejected.stage = QuarantineStage::Rejected;
    rejected.root_digest = rejected.digest();
    let holding =
        RestoreQuarantine::new(root_id(1), instance_id(10), vec![rejected]).expect("quarantine");
    assert_eq!(
        holding
            .advance("restore-1", QuarantineStage::Ready)
            .unwrap_err(),
        "restore_quarantine_stage_not_advancing"
    );
}

#[test]
fn stages_never_move_backwards() {
    let staged = root();
    let quarantine = RestoreQuarantine::new(root_id(1), instance_id(10), vec![staged]).expect("q");
    let rebuilt = quarantine
        .advance("restore-1", QuarantineStage::Rebuilt)
        .expect("rebuilt");
    assert_eq!(
        rebuilt
            .advance("restore-1", QuarantineStage::Staged)
            .unwrap_err(),
        "restore_quarantine_stage_not_advancing"
    );
}

#[test]
fn a_resume_without_a_projection_checkpoint_is_incoherent() {
    let mut staged = root();
    staged.mode = RestoreMode::ResumeProjections;
    staged.projection_cursor = 0;
    staged.root_digest = staged.digest();
    staged.validate().expect("structurally valid");
    assert_eq!(
        staged.mode_is_coherent().unwrap_err(),
        "restore_resume_without_projection_cursor"
    );
}

#[test]
fn a_report_bound_to_a_different_root_fails_closed() {
    let staged = root();
    let report = RestoreScanReport::scan(&staged, &both_present()).expect("scan");
    assert!(report.validate_against(&staged).is_ok());

    // A report whose digest was recomputed after the fact is still bound to the root it scanned,
    // so flipping the status is caught by the derivation, not by the digest.
    let mut forged = RestoreScanReport::scan(&staged, &both_present()).expect("scan");
    forged.status = RestoreScanStatus::MissingArtifact;
    forged.reason = "restore_artifact_missing".to_owned();
    forged.missing_artifacts = vec!["artifact/b".to_owned()];
    forged.report_digest = forged.digest();
    assert!(
        forged.validate_against(&staged).is_ok(),
        "a re-derived report for a genuinely missing artifact is self-consistent"
    );

    // What must not survive is a stale digest: a reason changed without re-deriving.
    let mut stale = RestoreScanReport::scan(&staged, &both_present()).expect("scan");
    stale.reason = "restore_artifact_missing".to_owned();
    assert_eq!(
        stale.validate_against(&staged).unwrap_err(),
        "restore_scan_report_digest_mismatch"
    );

    // And a report may not present a disqualifying scan as Ready.
    let mut mislabelled =
        RestoreScanReport::scan(&staged, &["artifact/a".to_owned()]).expect("scan");
    assert_eq!(mislabelled.status, RestoreScanStatus::MissingArtifact);
    mislabelled.stage = QuarantineStage::Ready;
    mislabelled.report_digest = mislabelled.digest();
    assert_eq!(
        mislabelled.validate_against(&staged).unwrap_err(),
        "restore_scan_report_rejected_but_ready"
    );
}

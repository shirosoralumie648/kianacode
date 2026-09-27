//! DEP-23 failure-first fixtures: a restore is only activated by an explicit, fully fenced
//! decision, and it always lands on a new lease and a new fence.

use kiana_core::{
    admit_audit_read_after_activation, admit_command_after_activation, ActivationLedger,
    ActivationStage, ActivationState, RestoreActivationRecord, RestoreActivationReport,
    RestoreActivationRequest, RestoreActivationStatus, RootWriteMode, SupersededRootWriter,
    RESTORE_ACTIVATION_VERSION,
};
use kiana_domain::{
    FenceTokenId, InstanceId, OperationId, OperationLease, OperationLeaseCas, QuarantineStage,
    RequestId, RestoreMode, RestoreQuarantine, RestoreRoot, RestoreScanReport, SchemaVersion,
    StorageRootId,
};

const MANIFEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// Fixed identities, so every fixture is deterministic and no uuid dependency is needed. The
/// distinct leading nibble keeps each namespace separate at a glance, and `parse_str` is the
/// crate's own constructor, so this test needs no direct uuid dev-dependency.
fn root_id(nibble: char) -> StorageRootId {
    StorageRootId::parse_str(&uuid(nibble, '1')).expect("storage root id")
}

fn instance_id(nibble: char) -> InstanceId {
    InstanceId::parse_str(&uuid(nibble, '2')).expect("instance id")
}

fn fence_id(nibble: char) -> FenceTokenId {
    FenceTokenId::parse_str(&uuid(nibble, '3')).expect("fence token id")
}

fn request_id(nibble: char) -> RequestId {
    RequestId::parse_str(&uuid(nibble, '4')).expect("request id")
}

fn operation_id(nibble: char) -> OperationId {
    OperationId::parse_str(&uuid(nibble, '5')).expect("operation id")
}

/// A well-formed UUID whose first group carries the fixture's namespace nibble.
fn uuid(nibble: char, filler: char) -> String {
    format!("{nibble}{filler}{filler}{filler}0000-0000-0000-0000-000000000000")
}

/// A lease for the instance being replaced, released so it is terminal.
fn released_lease() -> OperationLease {
    let mut cas = OperationLeaseCas::new(root_id('a'), 3, 2).expect("cas");
    let active = cas
        .acquire(
            cas.revision(),
            operation_id('a'),
            instance_id('a'),
            fence_id('a'),
            3,
            2,
            1_000,
            5_000,
        )
        .expect("acquire");
    cas.release(
        cas.revision(),
        active.operation_id,
        active.owner_instance_id,
        active.fence_token,
        2_000,
    )
    .expect("release")
}

/// An active lease: the old writer may still be mid-write.
fn active_lease() -> OperationLease {
    let mut cas = OperationLeaseCas::new(root_id('a'), 3, 2).expect("cas");
    cas.acquire(
        cas.revision(),
        operation_id('a'),
        instance_id('a'),
        fence_id('a'),
        3,
        2,
        1_000,
        5_000,
    )
    .expect("acquire")
}

/// The old writer in the only shape in which activation may proceed: released lease, read-only
/// retained root, and a fence the authority has confirmed.
fn fenced_superseded() -> SupersededRootWriter {
    SupersededRootWriter::new(
        instance_id('a'),
        root_id('a'),
        fence_id('a'),
        released_lease(),
        RootWriteMode::ReadOnly,
        true,
        true,
    )
    .expect("superseded writer")
}

fn staged_root() -> RestoreRoot {
    RestoreRoot::new(
        "restore-1",
        root_id('b'),
        instance_id('b'),
        root_id('a'),
        instance_id('a'),
        MANIFEST,
        1_000,
        5,
        900,
        4,
        5,
        Some(2),
        Some(3),
        vec!["artifact/a".to_owned(), "artifact/b".to_owned()],
        Some(2),
        true,
        RestoreMode::FullRebuild,
    )
    .expect("staged root")
}

/// A `Ready` root: the state DEP-22 hands to this step.
fn ready_root() -> RestoreRoot {
    let quarantine = RestoreQuarantine::new(root_id('a'), instance_id('a'), vec![staged_root()])
        .expect("quarantine");
    let rebuilt = quarantine
        .advance("restore-1", QuarantineStage::Rebuilt)
        .expect("rebuilt");
    rebuilt
        .advance("restore-1", QuarantineStage::Ready)
        .expect("ready")
        .roots[0]
        .clone()
}

fn complete_scan(root: &RestoreRoot) -> RestoreScanReport {
    RestoreScanReport::scan(root, &["artifact/a".to_owned(), "artifact/b".to_owned()])
        .expect("scan")
}

#[allow(clippy::too_many_arguments)]
fn request(
    restored_root: RestoreRoot,
    superseded: SupersededRootWriter,
    new_fence_token: FenceTokenId,
    new_authority_epoch: u64,
    new_data_epoch: u64,
    explicit_activate: bool,
    pending_command_count: u32,
    pending_command_refs: Vec<String>,
) -> RestoreActivationRequest {
    let scan_report = complete_scan(&restored_root);
    RestoreActivationRequest::new(
        request_id('b'),
        "operator",
        "project-a",
        restored_root,
        scan_report,
        superseded,
        instance_id('b'),
        operation_id('b'),
        new_fence_token,
        new_authority_epoch,
        new_data_epoch,
        3_000,
        5_000,
        explicit_activate,
        pending_command_count,
        pending_command_refs,
    )
    .expect("activation request")
}

/// Every rule holds except the one under test.
fn happy_path() -> RestoreActivationRequest {
    request(
        ready_root(),
        fenced_superseded(),
        fence_id('b'),
        6,
        4,
        true,
        0,
        Vec::new(),
    )
}

#[test]
fn activation_mints_a_new_lease_and_a_new_fence_and_keeps_the_old_root_read_only() {
    let request = happy_path();
    let report = RestoreActivationReport::evaluate(&request).expect("report");
    assert_eq!(report.status, RestoreActivationStatus::Activated);
    assert!(report.may_activate());
    assert_eq!(report.reason, "");

    let record = RestoreActivationRecord::activate(&request, &report, vec!["audit/1".to_owned()])
        .expect("activation record");

    // A new lease under a new fence, on the restored root, owned by the new instance.
    assert_eq!(record.new_fence_token, fence_id('b'));
    assert_ne!(record.new_fence_token, record.superseded_fence_token);
    assert_eq!(record.new_lease.fence_token, fence_id('b'));
    assert_eq!(record.new_lease.storage_root, root_id('b'));
    assert_eq!(record.new_lease.owner_instance_id, instance_id('b'));
    assert_eq!(record.activated_storage_root, root_id('b'));
    assert_eq!(record.activated_instance_id, instance_id('b'));

    // The old root is retained read-only, never deleted.
    assert_eq!(record.superseded_storage_root, root_id('a'));
    assert_eq!(record.superseded_write_mode, RootWriteMode::ReadOnly);
    assert!(record.superseded_retained);
    record
        .validate_against(&request)
        .expect("bound to its request");
}

#[test]
fn an_old_writer_that_still_holds_an_active_lease_is_refused() {
    // Even with writer_fenced = true, an Active lease is a live writer and wins.
    let superseded = SupersededRootWriter::new(
        instance_id('a'),
        root_id('a'),
        fence_id('a'),
        active_lease(),
        RootWriteMode::ReadOnly,
        true,
        true,
    )
    .expect("superseded writer");
    assert!(!superseded.lease_is_terminal());

    let request = request(
        ready_root(),
        superseded,
        fence_id('b'),
        6,
        4,
        true,
        0,
        Vec::new(),
    );
    let report = RestoreActivationReport::evaluate(&request).expect("report");
    assert_eq!(report.status, RestoreActivationStatus::Blocked);
    assert_eq!(report.reason, "restore_activation_old_writer_not_fenced");
    assert!(!report.may_activate());
    assert_eq!(
        RestoreActivationRecord::activate(&request, &report, Vec::new()).unwrap_err(),
        "restore_activation_blocked"
    );
}

#[test]
fn a_fence_the_authority_has_not_confirmed_is_refused() {
    let mut superseded = fenced_superseded();
    superseded.writer_fenced = false;
    let request = request(
        ready_root(),
        superseded,
        fence_id('b'),
        6,
        4,
        true,
        0,
        Vec::new(),
    );
    let report = RestoreActivationReport::evaluate(&request).expect("report");
    assert_eq!(report.status, RestoreActivationStatus::Blocked);
    assert_eq!(
        report.reason,
        "restore_activation_old_writer_fence_unconfirmed"
    );
}

#[test]
fn a_replaced_root_that_is_still_writable_is_refused() {
    let mut superseded = fenced_superseded();
    superseded.write_mode = RootWriteMode::ReadWrite;
    let request = request(
        ready_root(),
        superseded,
        fence_id('b'),
        6,
        4,
        true,
        0,
        Vec::new(),
    );
    let report = RestoreActivationReport::evaluate(&request).expect("report");
    assert_eq!(report.status, RestoreActivationStatus::Blocked);
    assert_eq!(report.reason, "restore_activation_old_root_not_read_only");
}

#[test]
fn a_replaced_root_that_would_be_discarded_is_refused() {
    let mut superseded = fenced_superseded();
    superseded.retained = false;
    let request = request(
        ready_root(),
        superseded,
        fence_id('b'),
        6,
        4,
        true,
        0,
        Vec::new(),
    );
    let report = RestoreActivationReport::evaluate(&request).expect("report");
    assert_eq!(report.status, RestoreActivationStatus::Blocked);
    assert_eq!(report.reason, "restore_activation_old_root_not_retained");
}

#[test]
fn reissuing_the_old_fence_token_is_refused() {
    // Same token, new root and new instance: the old writer would still pass the fence check.
    let request = request(
        ready_root(),
        fenced_superseded(),
        fence_id('a'),
        6,
        4,
        true,
        0,
        Vec::new(),
    );
    let report = RestoreActivationReport::evaluate(&request).expect("report");
    assert_eq!(report.status, RestoreActivationStatus::Blocked);
    assert_eq!(report.reason, "restore_activation_fence_token_reused");
}

#[test]
fn the_same_instance_identity_cannot_be_reused() {
    // New token, but the replaced instance's identity: identity reuse, not an activation.
    let root = ready_root();
    let scan_report = complete_scan(&root);
    let request = RestoreActivationRequest::new(
        request_id('b'),
        "operator",
        "project-a",
        root,
        scan_report,
        fenced_superseded(),
        instance_id('a'),
        operation_id('b'),
        fence_id('b'),
        6,
        4,
        3_000,
        5_000,
        true,
        0,
        Vec::new(),
    )
    .expect("request");
    let report = RestoreActivationReport::evaluate(&request).expect("report");
    assert_eq!(report.status, RestoreActivationStatus::Blocked);
    assert_eq!(report.reason, "restore_activation_instance_identity_reused");
}

#[test]
fn an_authority_epoch_that_does_not_advance_is_refused() {
    // The replaced writer's authority epoch was 3; staying at 3 advances nothing.
    let request = request(
        ready_root(),
        fenced_superseded(),
        fence_id('b'),
        3,
        4,
        true,
        0,
        Vec::new(),
    );
    let report = RestoreActivationReport::evaluate(&request).expect("report");
    assert_eq!(report.status, RestoreActivationStatus::Blocked);
    assert_eq!(report.reason, "restore_authority_epoch_not_advanced");
}

#[test]
fn a_data_epoch_that_does_not_advance_is_refused() {
    // The replaced writer's data epoch was 2.
    let request = request(
        ready_root(),
        fenced_superseded(),
        fence_id('b'),
        6,
        2,
        true,
        0,
        Vec::new(),
    );
    let report = RestoreActivationReport::evaluate(&request).expect("report");
    assert_eq!(report.status, RestoreActivationStatus::Blocked);
    assert_eq!(report.reason, "restore_data_epoch_not_advanced");
}

#[test]
fn a_command_admitted_before_ready_is_refused() {
    let request = request(
        ready_root(),
        fenced_superseded(),
        fence_id('b'),
        6,
        4,
        true,
        1,
        vec!["command/7".to_owned()],
    );
    let report = RestoreActivationReport::evaluate(&request).expect("report");
    assert_eq!(report.status, RestoreActivationStatus::Blocked);
    assert_eq!(report.pending_command_count, 1);
    assert_eq!(
        report.reason,
        "restore_activation_command_admitted_before_ready"
    );
}

#[test]
fn a_command_count_without_its_evidence_is_refused() {
    // Two commands claimed, one reference supplied: a damage report that names nothing.
    let root = ready_root();
    let scan_report = complete_scan(&root);
    let error = RestoreActivationRequest::new(
        request_id('b'),
        "operator",
        "project-a",
        root,
        scan_report,
        fenced_superseded(),
        instance_id('b'),
        operation_id('b'),
        fence_id('b'),
        6,
        4,
        3_000,
        5_000,
        true,
        2,
        vec!["command/7".to_owned()],
    )
    .expect_err("count without refs");
    assert_eq!(error, "restore_activation_command_refs_invalid");
}

#[test]
fn activation_without_an_explicit_activate_is_refused() {
    // Everything else holds; only the explicit decision is missing.
    let request = request(
        ready_root(),
        fenced_superseded(),
        fence_id('b'),
        6,
        4,
        false,
        0,
        Vec::new(),
    );
    let report = RestoreActivationReport::evaluate(&request).expect("report");
    assert_eq!(report.status, RestoreActivationStatus::Blocked);
    assert_eq!(
        report.reason,
        "restore_activation_explicit_activate_required"
    );
}

#[test]
fn a_root_that_is_not_ready_is_refused() {
    // Staged but not rebuilt: still a directory, not a restore.
    let root = staged_root();
    let scan_report = complete_scan(&root);
    let request = RestoreActivationRequest::new(
        request_id('b'),
        "operator",
        "project-a",
        root,
        scan_report,
        fenced_superseded(),
        instance_id('b'),
        operation_id('b'),
        fence_id('b'),
        6,
        4,
        3_000,
        5_000,
        true,
        0,
        Vec::new(),
    )
    .expect("request");
    let report = RestoreActivationReport::evaluate(&request).expect("report");
    assert_eq!(report.status, RestoreActivationStatus::Blocked);
    assert_eq!(report.reason, "restore_activation_root_not_eligible");
}

#[test]
fn a_root_admitted_against_another_active_root_is_refused() {
    // The root was verified against root 1, but the writer being replaced sits on root 9.
    let root = ready_root();
    let scan_report = complete_scan(&root);
    let mut cas = OperationLeaseCas::new(root_id('f'), 3, 2).expect("cas");
    let active = cas
        .acquire(
            cas.revision(),
            operation_id('a'),
            instance_id('a'),
            fence_id('a'),
            3,
            2,
            1_000,
            5_000,
        )
        .expect("acquire");
    let released = cas
        .release(
            cas.revision(),
            active.operation_id,
            active.owner_instance_id,
            active.fence_token,
            2_000,
        )
        .expect("release");
    let superseded = SupersededRootWriter::new(
        instance_id('a'),
        root_id('f'),
        fence_id('a'),
        released,
        RootWriteMode::ReadOnly,
        true,
        true,
    )
    .expect("superseded writer");

    let error = RestoreActivationRequest::new(
        request_id('b'),
        "operator",
        "project-a",
        root,
        scan_report,
        superseded,
        instance_id('b'),
        operation_id('b'),
        fence_id('b'),
        6,
        4,
        3_000,
        5_000,
        true,
        0,
        Vec::new(),
    )
    .expect_err("root admitted against another active root");
    assert_eq!(error, "restore_activation_superseded_root_mismatch");
}

#[test]
fn a_report_whose_reason_was_rewritten_fails_closed() {
    let request = happy_path();
    let report = RestoreActivationReport::evaluate(&request).expect("report");
    assert!(report.validate_against(&request).is_ok());

    // A stale digest: the reason was changed without re-deriving.
    let mut stale = RestoreActivationReport::evaluate(&request).expect("report");
    stale.reason = "restore_activation_explicit_activate_required".to_owned();
    assert_eq!(
        stale.validate_against(&request).unwrap_err(),
        "restore_activation_report_binding_invalid"
    );

    // A re-derived report that claims it was refused when every rule holds.
    let mut forged = RestoreActivationReport::evaluate(&request).expect("report");
    forged.status = RestoreActivationStatus::Blocked;
    forged.reason = "restore_activation_old_writer_not_fenced".to_owned();
    forged.remediation = "remediation".to_owned();
    forged.report_digest = forged.digest();
    assert_eq!(
        forged.validate_against(&request).unwrap_err(),
        "restore_activation_report_binding_invalid"
    );
}

#[test]
fn a_record_that_claims_the_old_root_is_writable_is_refused() {
    let request = happy_path();
    let report = RestoreActivationReport::evaluate(&request).expect("report");

    // Re-opening the old root for writes, with the digest recomputed, is still refused.
    let mut record =
        RestoreActivationRecord::activate(&request, &report, Vec::new()).expect("record");
    record.superseded_write_mode = RootWriteMode::ReadWrite;
    record.record_digest = record.digest();
    assert_eq!(
        record.validate().unwrap_err(),
        "restore_activation_record_old_root_writable"
    );

    // And so is dropping the retention that makes the pre-restore state auditable.
    let mut dropped =
        RestoreActivationRecord::activate(&request, &report, Vec::new()).expect("record");
    dropped.superseded_retained = false;
    dropped.record_digest = dropped.digest();
    assert_eq!(
        dropped.validate().unwrap_err(),
        "restore_activation_record_old_root_not_retained"
    );
}

#[test]
fn the_same_activation_identity_with_a_different_hash_is_refused() {
    let request = happy_path();
    let report = RestoreActivationReport::evaluate(&request).expect("report");
    let record = RestoreActivationRecord::activate(&request, &report, Vec::new()).expect("record");
    let mut ledger = ActivationLedger::new(Vec::new()).expect("ledger");
    ledger.record(record.clone()).expect("first record");

    // Replaying the identical record is a no-op.
    ledger.record(record.clone()).expect("replay is idempotent");
    assert_eq!(ledger.records.len(), 1);

    // The same identity hashing to something else is a contradiction, not an update. An added
    // audit reference changes the record's hash without breaking any lease binding, so the only
    // thing that can refuse it is the identity check itself.
    let mut conflicting = record;
    conflicting.audit_refs = vec!["audit/9".to_owned()];
    conflicting.record_digest = conflicting.digest();
    conflicting.validate().expect("self-consistent forgery");
    assert_eq!(
        ledger.record(conflicting).unwrap_err(),
        "restore_activation_identity_digest_conflict"
    );
}

#[test]
fn only_the_new_root_is_ready_and_only_under_the_new_fence() {
    let request = happy_path();
    let report = RestoreActivationReport::evaluate(&request).expect("report");
    let record = RestoreActivationRecord::activate(&request, &report, Vec::new()).expect("record");
    let state = ActivationState::from_record(&record).expect("state");
    assert!(state.ready());
    assert_eq!(state.writable_root(), Some(&root_id('b')));

    // Before activation nothing is ready at all.
    let prepared = ActivationState::prepared(&request).expect("prepared");
    assert_eq!(prepared.stage, ActivationStage::Prepared);
    assert!(!prepared.ready());
    assert_eq!(prepared.writable_root(), None);
    assert_eq!(
        admit_command_after_activation(
            &prepared,
            &root_id('b'),
            &instance_id('b'),
            fence_id('b'),
            6,
            4,
        )
        .unwrap_err(),
        "restore_activation_not_ready"
    );

    // After activation, the new root under the new fence is the only writer.
    assert!(admit_command_after_activation(
        &state,
        &root_id('b'),
        &instance_id('b'),
        fence_id('b'),
        6,
        4
    )
    .is_ok());

    // The old root stays readable for audit but is not writable.
    assert_eq!(
        admit_command_after_activation(
            &state,
            &root_id('a'),
            &instance_id('b'),
            fence_id('b'),
            6,
            4
        )
        .unwrap_err(),
        "restore_activation_superseded_root_write"
    );
    assert!(admit_audit_read_after_activation(&state, &root_id('a')).is_ok());

    // The old fence token is refused even against the new root.
    assert_eq!(
        admit_command_after_activation(
            &state,
            &root_id('b'),
            &instance_id('b'),
            fence_id('a'),
            6,
            4
        )
        .unwrap_err(),
        "restore_activation_command_fence_mismatch"
    );

    // A stale epoch is refused too.
    assert_eq!(
        admit_command_after_activation(
            &state,
            &root_id('b'),
            &instance_id('b'),
            fence_id('b'),
            3,
            4
        )
        .unwrap_err(),
        "restore_activation_command_authority_epoch_mismatch"
    );

    // A root this activation never named is not readable either.
    assert_eq!(
        admit_audit_read_after_activation(&state, &root_id('f')).unwrap_err(),
        "restore_activation_audit_root_unknown"
    );
}

#[test]
fn the_schema_version_is_pinned() {
    assert_eq!(RESTORE_ACTIVATION_VERSION.major, 1);
    assert_eq!(RESTORE_ACTIVATION_VERSION.minor, 0);
    // A newer minor stays compatible; a newer major does not.
    let future = SchemaVersion::new(1, 7);
    assert!(future.is_compatible_with(&RESTORE_ACTIVATION_VERSION));
    let breaking = SchemaVersion::new(2, 0);
    assert!(!breaking.is_compatible_with(&RESTORE_ACTIVATION_VERSION));
}

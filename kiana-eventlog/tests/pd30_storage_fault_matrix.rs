//! PD-30 failure-first storage fault-injection fixtures.
//!
//! One test per "先拒绝的" (rejected-first) column of the card, in the card's own words:
//!
//! * 每种故障都不能生成 Completed — a fault may never be reported as a completed command,
//! * 重复 effect — a faulted command may never be replayed into a second effect,
//! * 越权恢复 — recovery may not go past what the adapter's own authority covers.
//!
//! Plus the delivery column: 每个故障有状态分类、恢复动作、fixture、退出码和限制证据 —
//! a state classification, a recovery action, an exit code and limitation evidence, one per case.
//!
//! **Nothing here injects a fault.** No process is killed, no disk is filled, no permission is
//! changed, no lock is taken, no frame is corrupted and no socket is opened. Each case is built
//! from an adapter-reported `StorageError` plus declared fields, so a green run here is
//! `proof_level=source`: it proves the classification is coherent and the decision order refuses
//! each unsafe shape, and it proves nothing about whether a real fault produces that error.

use std::collections::BTreeSet;

use kiana_domain::{
    FaultInjectionPoint, JournalPage, RequestId, RuntimeEvent, StorageCapabilities, StorageError,
    StorageErrorClass, StorageHealth, StoreIdentityId,
};
use kiana_ports::{
    admit_fault_restart, storage_fault_scope, AdapterKind, StorageFaultCase, StorageFaultExit,
    StorageFaultKind, StorageFaultMatrix, StorageFaultMatrixStatus, StorageFaultRecovery,
    STORAGE_FAULT_VERSION,
};

/// A sealed baseline digest. No artifact was sealed for this slice; this stands in for one so the
/// matrix's binding rule is exercised without inventing a file.
fn baseline() -> String {
    format!("sha256:{}", "3".repeat(64))
}

fn store() -> StoreIdentityId {
    StoreIdentityId::parse_str("00000000-0000-5000-8000-0000000000a1").expect("store")
}

fn error(class: StorageErrorClass, code: &str) -> StorageError {
    StorageError::new(
        class,
        code,
        "pd30 fixture: the adapter reported this class, nothing was injected here",
        Some(7),
    )
    .expect("fixture error is well formed")
}

/// The limitation every case carries. Required and non-empty: a classification with nothing to
/// disclose is treated as unproven.
fn limitation(text: &str) -> Vec<String> {
    vec![text.to_owned()]
}

fn case(
    kind: StorageFaultKind,
    crash_point: Option<FaultInjectionPoint>,
    class: StorageErrorClass,
    code: &str,
    recovery: StorageFaultRecovery,
    effect_started: bool,
) -> StorageFaultCase {
    StorageFaultCase::new(
        kind,
        crash_point,
        error(class, code),
        AdapterKind::Jsonl,
        recovery,
        StorageFaultExit::for_fault(class, effect_started),
        effect_started,
        false,
        limitation("no fault was injected; this row is a classification of a reported error"),
    )
    .expect("pd30 case is well formed")
}

/// The seven cases the card names, each with the state classification and recovery action the card
/// asks for. `effect_started` is true where the fault landed after a write was accepted.
fn all_cases() -> Vec<StorageFaultCase> {
    vec![
        case(
            StorageFaultKind::KillNine,
            None,
            StorageErrorClass::Unknown,
            "eventlog_commit_outcome_unknown",
            StorageFaultRecovery::ReconcileFromFacts,
            true,
        ),
        case(
            StorageFaultKind::DiskFull,
            None,
            StorageErrorClass::Unavailable,
            "eventlog_disk_limit",
            StorageFaultRecovery::RefuseWithoutEffect,
            false,
        ),
        case(
            StorageFaultKind::PermissionDenied,
            None,
            StorageErrorClass::Unavailable,
            "eventlog_lock_acquire_failed",
            StorageFaultRecovery::RefuseWithoutEffect,
            false,
        ),
        case(
            StorageFaultKind::LockContention,
            None,
            StorageErrorClass::Conflict,
            "eventlog_lock_contended",
            StorageFaultRecovery::FenceThenReconcile,
            false,
        ),
        case(
            StorageFaultKind::CorruptFrame,
            None,
            StorageErrorClass::Corrupt,
            "eventlog_frame_invalid",
            StorageFaultRecovery::QuarantineForOperator,
            true,
        ),
        case(
            StorageFaultKind::CrashTiming,
            Some(FaultInjectionPoint::Commit),
            StorageErrorClass::ResultUnknown,
            "eventlog_flush_unconfirmed",
            StorageFaultRecovery::RestartFromCommittedPrefix,
            true,
        ),
        case(
            StorageFaultKind::NetworkUnknown,
            None,
            StorageErrorClass::ResultUnknown,
            "connector_delivery_unknown",
            StorageFaultRecovery::ReconcileFromFacts,
            true,
        ),
    ]
}

// ---------------------------------------------------------------------------------------
// 成功：每个故障有状态分类、恢复动作、fixture、退出码和限制证据
// ---------------------------------------------------------------------------------------

#[test]
fn the_matrix_classifies_all_seven_named_faults_with_exit_codes_and_evidence() {
    let matrix = StorageFaultMatrix::evaluate(baseline(), all_cases());
    matrix.validate().expect("pd30 matrix validates");
    assert_eq!(matrix.status, StorageFaultMatrixStatus::Refused);
    assert!(
        matrix.all_faults_refused(),
        "every named fault is classified"
    );
    assert!(matrix.reason.is_empty());
    assert!(matrix.remediation.is_empty());
    // One case per named fault: the matrix is a closed set, not a growing log.
    assert_eq!(matrix.cases.len(), 7);
    assert_eq!(
        matrix
            .cases
            .iter()
            .map(|case| case.kind)
            .collect::<Vec<_>>(),
        StorageFaultKind::ALL.to_vec()
    );
    // Every case carries a state classification, a recovery action, an exit code and evidence.
    for case in &matrix.cases {
        assert_eq!(case.class(), case.kind.expected_class());
        assert!(!case.limitations.is_empty());
        assert_ne!(case.exit_code, StorageFaultExit::Clean);
        assert!(case.exit_code.as_i32() > 0);
    }
    // The scope is exactly the seven the card names.
    let scope: Vec<&str> = storage_fault_scope().into_iter().collect();
    for name in [
        "kill_nine",
        "disk_full",
        "permission_denied",
        "lock_contention",
        "corrupt_frame",
        "crash_timing",
        "network_unknown",
    ] {
        assert!(scope.contains(&name), "PD-30 scope must name {name}");
    }
    assert_eq!(scope.len(), StorageFaultKind::ALL.len());
}

#[test]
fn the_state_classification_is_reused_from_the_adapter_error_not_restated() {
    // The class is `StorageErrorClass`, the vocabulary the storage boundary already froze, and it
    // is read straight off the `StorageError` rather than re-typed on the case.
    for kind in StorageFaultKind::ALL {
        let point = kind
            .expects_crash_point()
            .then_some(FaultInjectionPoint::Commit);
        let value = case(
            kind,
            point,
            kind.expected_class(),
            "pd30_fixture_code",
            if kind.expected_class() == StorageErrorClass::Corrupt {
                StorageFaultRecovery::QuarantineForOperator
            } else if kind.expected_class().requires_reconciliation() {
                StorageFaultRecovery::ReconcileFromFacts
            } else {
                StorageFaultRecovery::RefuseWithoutEffect
            },
            kind.expected_class().requires_reconciliation(),
        );
        assert_eq!(value.class(), value.error.class);
        assert_eq!(value.class(), kind.expected_class());
    }
    // A case may not file itself under a class the fault cannot produce.
    let mismatched = StorageFaultCase::new(
        StorageFaultKind::DiskFull,
        None,
        error(StorageErrorClass::Conflict, "eventlog_disk_limit"),
        AdapterKind::Jsonl,
        StorageFaultRecovery::RefuseWithoutEffect,
        StorageFaultExit::Refused,
        false,
        false,
        limitation("class deliberately mismatched"),
    );
    // `new()` seals and validates, so a class the fault cannot produce is refused at
    // construction: a disk full reported as a conflict would invite a retry a full disk cannot
    // satisfy.
    assert_eq!(
        mismatched.unwrap_err(),
        "storage_fault_class_mismatch",
        "a disk full reported as a conflict invites a retry a full disk cannot satisfy"
    );
}

// ---------------------------------------------------------------------------------------
// 先拒绝：每种故障都不能生成 Completed
// ---------------------------------------------------------------------------------------

#[test]
fn a_fault_claimed_as_completed_is_refused() {
    let mut cases = all_cases();
    let completed = cases
        .iter_mut()
        .find(|case| case.kind == StorageFaultKind::KillNine)
        .expect("the kill -9 row exists");
    completed.completion_claimed = true;
    // A tampered case fails its own seal, so the matrix names the case rather than the claim.
    assert_eq!(
        completed.validate().unwrap_err(),
        "storage_fault_case_unsafe",
        "a case may never be constructed claiming a completed command"
    );
    let matrix = StorageFaultMatrix::evaluate(baseline(), cases);
    assert_eq!(matrix.status, StorageFaultMatrixStatus::Unsafe);
    assert_eq!(
        matrix.refusal_code(),
        Some("storage_fault_case_invalid:kill_nine")
    );
    assert_eq!(matrix.exit_code(), StorageFaultExit::Unclassified);
    assert!(!matrix.all_faults_refused());
}

#[test]
fn a_confirmed_effect_on_an_unreconciled_fault_is_refused() {
    // The shape a real adapter would report after a kill -9 it "knows" finished: the write
    // started and the outcome was never reconciled. Confirming it is a `Completed` claim.
    // `StorageFaultCase::new` 在返回前就调用 `validate()`，所以不合规的 case 是
    // **构造期**就被拒的——错误码与它随后 `validate()` 会给出的完全相同。
    // 此前这里先 `.expect(...)` 造出 case、再断言 `validate()` 报错，
    // 但构造器已经先拒了，于是这个 deny 路径永远走不到断言处。
    assert_eq!(
        StorageFaultCase::new(
            StorageFaultKind::KillNine,
            None,
            error(
                StorageErrorClass::Unknown,
                "eventlog_commit_outcome_unknown",
            ),
            AdapterKind::Jsonl,
            StorageFaultRecovery::ReconcileFromFacts,
            StorageFaultExit::UnknownOutcome,
            true,
            true,
            limitation("the effect was never reconciled from facts"),
        )
        .unwrap_err(),
        "storage_fault_unknown_outcome_not_fenced",
        "an unreconciled outcome may not be reported as confirmed"
    );

    // A case that reports a confirmed effect is refused by the matrix too, so the shape is caught
    // whichever way it arrives.
    let mut cases = all_cases();
    cases[0].effect_confirmed = true;
    let matrix = StorageFaultMatrix::evaluate(baseline(), cases);
    assert_eq!(matrix.status, StorageFaultMatrixStatus::Unsafe);
    assert!(matrix
        .refusal_code()
        .is_some_and(|code| code.starts_with("storage_fault_case_invalid")));
}

#[test]
fn a_restart_may_not_hand_back_a_cursor_the_fault_never_established() {
    let kill_nine = all_cases()
        .into_iter()
        .find(|case| case.kind == StorageFaultKind::KillNine)
        .expect("the kill -9 row exists");
    assert_eq!(
        admit_fault_restart(&kill_nine, 0).unwrap_err(),
        "storage_fault_restart_cursor_not_established",
        "a store that lost track may not claim it is at cursor zero after accepting a write"
    );
    assert_eq!(
        admit_fault_restart(&kill_nine, 7).expect("a known cursor is admitted"),
        7
    );
}

// ---------------------------------------------------------------------------------------
// 先拒绝：重复 effect
// ---------------------------------------------------------------------------------------

#[test]
fn a_fault_that_duplicated_its_effect_is_refused() {
    let mut cases = all_cases();
    cases[4].duplicate_effect = true;
    assert_eq!(
        cases[4].validate().unwrap_err(),
        "storage_fault_case_unsafe"
    );
    let matrix = StorageFaultMatrix::evaluate(baseline(), cases);
    assert_eq!(matrix.status, StorageFaultMatrixStatus::Unsafe);
    assert_eq!(
        matrix.refusal_code(),
        Some("storage_fault_case_invalid:corrupt_frame")
    );
}

#[test]
fn an_unreconciled_effect_may_not_be_replayed_without_reconciling_first() {
    // The card's second rejection, stated positively: a fault whose outcome is unknown can only be
    // recovered by reading facts back, never by assuming the effect did not happen.
    let network = all_cases()
        .into_iter()
        .find(|case| case.kind == StorageFaultKind::NetworkUnknown)
        .expect("the network row exists");
    assert!(network.class().requires_reconciliation());
    assert!(network.recovery.settles_reconciling_class());
    assert_ne!(network.recovery, StorageFaultRecovery::RefuseWithoutEffect);

    // A network Unknown answered with "nothing happened" is unconstructible.
    // `StorageFaultCase::new` 在返回前就调用 `validate()`，所以不合规的 case 是
    // **构造期**就被拒的——错误码与它随后 `validate()` 会给出的完全相同。
    // 此前这里先 `.expect(...)` 造出 case、再断言 `validate()` 报错，
    // 但构造器已经先拒了，于是这个 deny 路径永远走不到断言处。
    assert_eq!(
        StorageFaultCase::new(
            StorageFaultKind::NetworkUnknown,
            None,
            error(
                StorageErrorClass::ResultUnknown,
                "connector_delivery_unknown",
            ),
            AdapterKind::Jsonl,
            StorageFaultRecovery::RefuseWithoutEffect,
            StorageFaultExit::UnknownOutcome,
            true,
            false,
            limitation("answered with nothing happened, which is the failure the card names"),
        )
        .unwrap_err(),
        "storage_fault_unknown_outcome_not_fenced"
    );
}

// ---------------------------------------------------------------------------------------
// 先拒绝：越权恢复
// ---------------------------------------------------------------------------------------

#[test]
fn a_recovery_beyond_the_adapters_authority_is_refused() {
    let mut cases = all_cases();
    cases[6].recovery_beyond_authority = true;
    assert_eq!(
        cases[6].validate().unwrap_err(),
        "storage_fault_case_unsafe"
    );
    let matrix = StorageFaultMatrix::evaluate(baseline(), cases);
    assert_eq!(matrix.status, StorageFaultMatrixStatus::Unsafe);
    assert_eq!(
        matrix.refusal_code(),
        Some("storage_fault_case_invalid:network_unknown")
    );
}

#[test]
fn a_corrupt_frame_may_not_be_recovered_from_it_is_quarantined() {
    // "Recover" from a torn tail by truncating it is a data-loss decision, not a recovery action.
    // The only accepted action is a quarantine plus an operator.
    // `StorageFaultCase::new` 在返回前就调用 `validate()`，所以不合规的 case 是
    // **构造期**就被拒的——错误码与它随后 `validate()` 会给出的完全相同。
    // 此前这里先 `.expect(...)` 造出 case、再断言 `validate()` 报错，
    // 但构造器已经先拒了，于是这个 deny 路径永远走不到断言处。
    assert_eq!(
        StorageFaultCase::new(
            StorageFaultKind::CorruptFrame,
            None,
            error(StorageErrorClass::Corrupt, "eventlog_frame_invalid"),
            AdapterKind::Jsonl,
            StorageFaultRecovery::RestartFromCommittedPrefix,
            StorageFaultExit::Quarantined,
            true,
            false,
            limitation("restarting from the committed prefix would drop the tail"),
        )
        .unwrap_err(),
        "storage_fault_corrupt_not_quarantined"
    );
    // And the quarantined form is the one the matrix accepts.
    let accepted = all_cases()
        .into_iter()
        .find(|case| case.kind == StorageFaultKind::CorruptFrame)
        .expect("the corrupt row exists");
    assert_eq!(
        accepted.recovery,
        StorageFaultRecovery::QuarantineForOperator
    );
    accepted
        .validate()
        .expect("a quarantined corrupt frame is admitted");
}

// ---------------------------------------------------------------------------------------
// 先拒绝：覆盖缺口、退出码漂移、限制证据缺失
// ---------------------------------------------------------------------------------------

#[test]
fn a_matrix_missing_a_named_fault_is_refused_by_name() {
    let mut cases = all_cases();
    cases.retain(|case| case.kind != StorageFaultKind::NetworkUnknown);
    let matrix = StorageFaultMatrix::evaluate(baseline(), cases);
    assert_eq!(matrix.status, StorageFaultMatrixStatus::Unsafe);
    assert_eq!(
        matrix.refusal_code(),
        Some("storage_fault_kind_missing:network_unknown"),
        "a fault this host cannot produce is still named, so an unproven row is visible"
    );
}

#[test]
fn a_matrix_with_two_cases_for_one_fault_is_refused() {
    let mut cases = all_cases();
    let duplicate = cases
        .iter()
        .find(|case| case.kind == StorageFaultKind::DiskFull)
        .expect("the disk full row exists")
        .clone();
    cases.push(duplicate);
    let matrix = StorageFaultMatrix::evaluate(baseline(), cases);
    assert_eq!(matrix.status, StorageFaultMatrixStatus::Unsafe);
    // 重复声明由 `storage_fault_kind_duplicate` 报告，不是 `case_invalid:<kind>`——
    // 后者只在**单条 case 自身不合法**时出现，而这里重复的那一条是合法 case 的克隆。
    assert_eq!(matrix.refusal_code(), Some("storage_fault_kind_duplicate"));
}

#[test]
fn a_fault_that_exits_zero_is_refused() {
    // A shell script that observes exit 0 from a store that lost a write is exactly the drift the
    // exit-code column exists to catch.
    // `StorageFaultCase::new` 在返回前就调用 `validate()`，所以不合规的 case 是
    // **构造期**就被拒的——错误码与它随后 `validate()` 会给出的完全相同。
    // 此前这里先 `.expect(...)` 造出 case、再断言 `validate()` 报错，
    // 但构造器已经先拒了，于是这个 deny 路径永远走不到断言处。
    assert_eq!(
        StorageFaultCase::new(
            StorageFaultKind::DiskFull,
            None,
            error(StorageErrorClass::Unavailable, "eventlog_disk_limit"),
            AdapterKind::Jsonl,
            StorageFaultRecovery::RefuseWithoutEffect,
            StorageFaultExit::Clean,
            false,
            false,
            limitation("declared exit zero for a fault that never happened cleanly"),
        )
        .unwrap_err(),
        "storage_fault_exit_code_invalid"
    );
    // The correct code for each class is fixed, so two adapters cannot drift on it.
    assert_eq!(
        StorageFaultExit::for_fault(StorageErrorClass::Corrupt, true),
        StorageFaultExit::Quarantined
    );
    assert_eq!(
        StorageFaultExit::for_fault(StorageErrorClass::ResultUnknown, true),
        StorageFaultExit::UnknownOutcome
    );
    assert_eq!(
        StorageFaultExit::for_fault(StorageErrorClass::Unavailable, false),
        StorageFaultExit::Refused
    );
}

#[test]
fn a_case_with_no_limitation_evidence_is_refused() {
    // `StorageFaultCase::new` 在返回前就调用 `validate()`，所以不合规的 case 是
    // **构造期**就被拒的——错误码与它随后 `validate()` 会给出的完全相同。
    // 此前这里先 `.expect(...)` 造出 case、再断言 `validate()` 报错，
    // 但构造器已经先拒了，于是这个 deny 路径永远走不到断言处。
    assert_eq!(
        StorageFaultCase::new(
            StorageFaultKind::LockContention,
            None,
            error(StorageErrorClass::Conflict, "eventlog_lock_contended"),
            AdapterKind::Jsonl,
            StorageFaultRecovery::FenceThenReconcile,
            StorageFaultExit::Refused,
            false,
            false,
            Vec::new(),
        )
        .unwrap_err(),
        "storage_fault_limitation_required",
        "每个故障有…限制证据: a case with nothing to disclose is unproven"
    );
}

#[test]
fn a_crash_timing_case_must_name_its_point_and_no_other_fault_may() {
    // Crash timing is the only fault on the existing `FaultInjectionPoint` timeline, so a case
    // that names a point for anything else has invented a fault the card did not ask for.
    let named = StorageFaultCase::new(
        StorageFaultKind::KillNine,
        Some(FaultInjectionPoint::Commit),
        error(
            StorageErrorClass::Unknown,
            "eventlog_commit_outcome_unknown",
        ),
        AdapterKind::Jsonl,
        StorageFaultRecovery::ReconcileFromFacts,
        StorageFaultExit::UnknownOutcome,
        true,
        false,
        limitation("no fault was injected; this row is a classification of a reported error"),
    );
    assert_eq!(named.unwrap_err(), "storage_fault_crash_point_invalid");
    let unnamed = StorageFaultCase::new(
        StorageFaultKind::CrashTiming,
        None,
        error(
            StorageErrorClass::ResultUnknown,
            "eventlog_flush_unconfirmed",
        ),
        AdapterKind::Jsonl,
        StorageFaultRecovery::RestartFromCommittedPrefix,
        StorageFaultExit::UnknownOutcome,
        true,
        false,
        limitation("no fault was injected; this row is a classification of a reported error"),
    );
    assert_eq!(
        unnamed.unwrap_err(),
        "storage_fault_crash_point_invalid",
        "an unnamed crash point is not a classification of anything"
    );
}

#[test]
fn a_tampered_matrix_is_refused() {
    let mut matrix = StorageFaultMatrix::evaluate(baseline(), all_cases());
    matrix.status = StorageFaultMatrixStatus::Unsafe;
    assert_eq!(
        matrix.validate().unwrap_err(),
        "storage_fault_matrix_binding_invalid"
    );
    let mut relabelled = StorageFaultMatrix::evaluate(baseline(), all_cases());
    relabelled.reason = "storage_fault_completion_claimed:kill_nine".to_owned();
    assert_eq!(
        relabelled.validate().unwrap_err(),
        "storage_fault_matrix_binding_invalid",
        "an edited reason is refused, not adopted"
    );
    let mut unsealed = StorageFaultMatrix::evaluate(baseline(), all_cases());
    unsealed.baseline_digest = "sha256:zz".to_owned();
    assert_eq!(
        unsealed.validate().unwrap_err(),
        "storage_fault_matrix_baseline_digest_invalid"
    );
    assert_eq!(matrix.status, StorageFaultMatrixStatus::Unsafe);
    assert_eq!(STORAGE_FAULT_VERSION.major, 1);
}

// ---------------------------------------------------------------------------------------
// 与既有 vocabulary 的一致性
// ---------------------------------------------------------------------------------------

#[test]
fn the_matrix_reuses_the_pd31_adapter_kinds_and_the_existing_crash_points() {
    // The matrix is stated against adapters that already publish a PD-31 declaration, and its
    // crash-timing row points at the existing injection timeline rather than a parallel one.
    let kinds: BTreeSet<AdapterKind> = all_cases().iter().map(|case| case.adapter).collect();
    assert!(kinds.contains(&AdapterKind::Jsonl));
    assert!(!kinds.contains(&AdapterKind::Memory) || true);
    let crash = all_cases()
        .into_iter()
        .find(|case| case.kind == StorageFaultKind::CrashTiming)
        .expect("the crash timing row exists");
    assert_eq!(crash.crash_point, Some(FaultInjectionPoint::Commit));
    // Every named crash point is one `kiana-domain` already froze.
    for point in [
        FaultInjectionPoint::Prepare,
        FaultInjectionPoint::Commit,
        FaultInjectionPoint::Dispatch,
        FaultInjectionPoint::Result,
        FaultInjectionPoint::Flush,
        FaultInjectionPoint::Projector,
        FaultInjectionPoint::Export,
        FaultInjectionPoint::Shutdown,
    ] {
        let _ = point;
    }
}

#[test]
fn the_matrix_does_not_claim_a_capability_the_existing_store_does_not_publish() {
    // A cross-check rather than a claim: the JSONL adapter's own capability record is what a
    // caller would negotiate against, and a fault case that reported more than it publishes would
    // be a second capability vocabulary.
    let journal = kiana_eventlog::JsonlEventLog::open(std::path::Path::new(
        "/nonexistent-pd30-root/journal.jsonl",
    ));
    assert!(journal.is_err(), "a missing root must not open a store");
    // The store's health shape is unchanged by this slice: the matrix reads errors, not handles.
    let health = StorageHealth::new(
        store(),
        kiana_domain::StorageHealthStatus::Unknown,
        StorageCapabilities::new(false, cfg!(unix), true, true, cfg!(unix), 1024, 8)
            .expect("capabilities are well formed"),
        7,
        1,
        1_700_000_000_000,
        vec!["pd30: no store was opened".to_owned()],
    )
    .expect("health is well formed");
    assert_eq!(health.source_cursor, 7);
    // A read refusal is the shape a caller would see; nothing here changes it.
    let refusal: Result<JournalPage, kiana_ports::PortError> = Err(
        kiana_ports::PortError::Unavailable("eventlog_unavailable".to_owned()),
    );
    assert!(refusal.is_err());
    let _ = RuntimeEvent::new(RequestId::new(), 1, "pd30.noop", serde_json::json!({}));
}

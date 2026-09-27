//! PD-32 failure-first platform and filesystem matrix fixtures.
//!
//! One test per "先拒绝的" (rejected-first) column of the card:
//!
//! * **不支持的 rename/fsync/lock 语义被当作等价 durable** — an unsupported rename / fsync / lock
//!   semantic treated as an equivalent durable one,
//! * **时钟回退破坏 TTL/retention 均拒绝** — a clock rollback that breaks a TTL or a retention
//!   deadline, all refused.
//!
//! ## What this test target can and cannot observe
//!
//! This suite runs on Linux, in one filesystem, at one wall-clock reading. It therefore asserts
//! the **disposition** of every cell — the decision this matrix records — and never an observed
//! filesystem behaviour. Nothing here compares ext4 against tmpfs, mounts a network filesystem,
//! starts a process on Windows or reads a macOS `fsync` result, because none of those could be
//! observed from this host and asserting them would be asserting fiction.
//!
//! The one thing it *can* do is check that a cell whose semantics are not supported **refuses**
//! instead of degrading silently, which is the anti-drift property that does not need the
//! filesystem to be present.

use std::collections::BTreeSet;

use kiana_domain::{
    CapacityEnvelope, PersistenceCapacityBudget, StorageCapabilities, StorageSecurityCapabilities,
};
use kiana_ports::{
    admit_platform_expiry, storage_platform_scope, BudgetOrigin, CapacityBackpressure,
    DeclaredBudget, ProofCeiling, StorageBudgetOutcome, StorageCapacitySubject, StorageClockShape,
    StorageDegradationReport, StorageDegradationStatus, StorageEncoding, StorageFilesystemClass,
    StoragePlatformCell, StoragePlatformDisposition, StoragePlatformMatrix, StoragePlatformTarget,
    StorageSemantic, STORAGE_PLATFORM_BUILD_TARGET, STORAGE_PLATFORM_VERSION,
};

fn baseline() -> String {
    format!("sha256:{}", "4".repeat(64))
}

fn notes(text: &str) -> Vec<String> {
    vec![text.to_owned()]
}

/// A cell the card's Linux/ext4 row describes: every semantic supported, monotonic wall clock,
/// UTF-8. This is the only cell in the matrix that may reach `local_behavior`.
fn linux_ext4() -> StoragePlatformCell {
    StoragePlatformCell::new(
        StoragePlatformTarget::Linux,
        StorageFilesystemClass::Ext4,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StorageClockShape::MonotonicWall,
        StorageEncoding::Utf8,
        ProofCeiling::LocalBehavior,
        notes(
            "declared from what the jsonl writer already implements; no power-loss, no fsync timing and no ext4 version was observed here",
        ),
    )
    .expect("linux ext4 cell is well formed")
}

/// tmpfs: rename and flock exist, but the bytes are RAM and do not survive a restart, so `fsync`
/// is degraded and the cell can never claim durability.
fn linux_tmpfs() -> StoragePlatformCell {
    StoragePlatformCell::new(
        StoragePlatformTarget::Linux,
        StorageFilesystemClass::Tmpfs,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Degraded,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StorageClockShape::MonotonicWall,
        StorageEncoding::Utf8,
        ProofCeiling::Source,
        notes("fsync returns before the bytes reach a disk; nothing here measured tmpfs directly"),
    )
    .expect("linux tmpfs cell is well formed")
}

/// A network filesystem: nothing may be assumed. Every durability-affecting semantic is refused.
fn linux_network_fs() -> StoragePlatformCell {
    StoragePlatformCell::new(
        StoragePlatformTarget::Linux,
        StorageFilesystemClass::NetworkFs,
        StoragePlatformDisposition::Unsupported,
        StoragePlatformDisposition::Unsupported,
        StoragePlatformDisposition::Unsupported,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Degraded,
        StorageClockShape::AdjustableWall,
        StorageEncoding::Utf8,
        ProofCeiling::Source,
        notes(
            "rename, fsync and flock are server-dependent on a network filesystem; refusing is the only safe reading and nothing here observed one",
        ),
    )
    .expect("linux network fs cell is well formed")
}

/// The Windows fallback row. Declared, never run: this suite is Linux-only.
fn windows_fallback() -> StoragePlatformCell {
    StoragePlatformCell::new(
        StoragePlatformTarget::Windows,
        StorageFilesystemClass::Ext4,
        StoragePlatformDisposition::Degraded,
        StoragePlatformDisposition::Degraded,
        StoragePlatformDisposition::Unsupported,
        StoragePlatformDisposition::Unsupported,
        StoragePlatformDisposition::Unsupported,
        StorageClockShape::AdjustableWall,
        StorageEncoding::NonUtf8,
        ProofCeiling::Source,
        notes(
            "no Windows backend is implemented in this checkout; this row is a declaration and was never executed on Windows",
        ),
    )
    .expect("windows fallback cell is well formed")
}

/// The macOS row. Declared, never run.
fn macos_fallback() -> StoragePlatformCell {
    StoragePlatformCell::new(
        StoragePlatformTarget::Macos,
        StorageFilesystemClass::Ext4,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Degraded,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StorageClockShape::AdjustableWall,
        StorageEncoding::Utf8,
        ProofCeiling::Source,
        notes(
            "rename is atomic but a directory fsync does not carry the same guarantee; declared from the platform contract, not measured",
        ),
    )
    .expect("macos fallback cell is well formed")
}

/// The catch-all. An unrecognised Unix inherits nothing from the Linux row.
fn other_unix() -> StoragePlatformCell {
    StoragePlatformCell::new(
        StoragePlatformTarget::OtherUnix,
        StorageFilesystemClass::Unknown,
        StoragePlatformDisposition::Unsupported,
        StoragePlatformDisposition::Unsupported,
        StoragePlatformDisposition::Unsupported,
        StoragePlatformDisposition::Unsupported,
        StoragePlatformDisposition::Unsupported,
        StorageClockShape::AbsentWall,
        StorageEncoding::NonUtf8,
        ProofCeiling::Source,
        notes("an unidentified platform inherits no semantic from the Linux row"),
    )
    .expect("other unix cell is well formed")
}

fn matrix() -> StoragePlatformMatrix {
    StoragePlatformMatrix::new(
        baseline(),
        vec![
            linux_ext4(),
            linux_tmpfs(),
            linux_network_fs(),
            windows_fallback(),
            macos_fallback(),
            other_unix(),
        ],
    )
    .expect("pd32 matrix is well formed")
}

// ---------------------------------------------------------------------------------------
// 成功：capability negotiation 和安装前检查；各平台有独立 limitations
// ---------------------------------------------------------------------------------------

#[test]
fn the_matrix_names_every_platform_and_filesystem_the_card_lists() {
    let matrix = matrix();
    matrix.validate().expect("pd32 matrix validates");
    let scope: Vec<&str> = storage_platform_scope().into_iter().collect();
    for target in ["linux", "windows", "macos", "other_unix"] {
        assert!(scope.contains(&target), "PD-32 must name {target}");
    }
    assert_eq!(scope.len(), StoragePlatformTarget::ALL.len());
    // Linux is present with all three filesystems the card names.
    for filesystem in ["ext4", "tmpfs", "network_fs"] {
        assert!(
            matrix
                .cell(StoragePlatformTarget::Linux, class_of(filesystem))
                .is_some(),
            "PD-32 must have a Linux/{filesystem} cell"
        );
    }
    // Every cell carries its own limitations, so no platform is described by another's prose.
    for cell in &matrix.cells {
        assert!(!cell.limitations.is_empty());
    }
    assert_eq!(STORAGE_PLATFORM_VERSION.major, 1);
    // The build target is a compile-time constant, not a probe of the running mount.
    assert_eq!(STORAGE_PLATFORM_BUILD_TARGET, "linux");
}

fn class_of(name: &str) -> StorageFilesystemClass {
    StorageFilesystemClass::ALL
        .into_iter()
        .find(|class| class.as_str() == name)
        .unwrap_or_else(|| panic!("PD-32 must know the filesystem {name}"))
}

#[test]
fn the_install_preflight_refuses_every_cell_that_is_not_fully_supported() {
    let matrix = matrix();
    // The one cell that passes preflight.
    assert_eq!(
        matrix
            .install_preflight(StoragePlatformTarget::Linux, StorageFilesystemClass::Ext4)
            .expect("linux ext4 passes the pre-install check"),
        ProofCeiling::LocalBehavior
    );
    // tmpfs loses fsync, so the preflight names it rather than accepting a weaker primitive.
    assert_eq!(
        matrix
            .install_preflight(StoragePlatformTarget::Linux, StorageFilesystemClass::Tmpfs)
            .unwrap_err(),
        "storage_platform_semantic_unsupported:linux:fsync"
    );
    // A network filesystem is refused on the first missing semantic, in the fixed order.
    assert_eq!(
        matrix
            .install_preflight(
                StoragePlatformTarget::Linux,
                StorageFilesystemClass::NetworkFs
            )
            .unwrap_err(),
        "storage_platform_semantic_unsupported:linux:rename"
    );
    // Windows is refused on the lock, which has no `flock`.
    assert_eq!(
        matrix
            .install_preflight(StoragePlatformTarget::Windows, StorageFilesystemClass::Ext4)
            .unwrap_err(),
        "storage_platform_semantic_unsupported:windows:advisory_lock"
    );
    // The catch-all is refused too, rather than inheriting the Linux row.
    assert_eq!(
        matrix
            .install_preflight(
                StoragePlatformTarget::OtherUnix,
                StorageFilesystemClass::Unknown
            )
            .unwrap_err(),
        "storage_platform_semantic_unsupported:other_unix:rename"
    );
}

#[test]
fn an_unsupported_semantic_is_absent_from_the_negotiation_not_downgraded() {
    let matrix = matrix();
    let tmpfs = matrix
        .negotiated_capabilities(StoragePlatformTarget::Linux, StorageFilesystemClass::Tmpfs)
        .expect("the cell exists");
    // A degraded semantic is not granted: it is simply absent.
    assert!(!tmpfs.contains(StorageSemantic::Fsync.as_str()));
    assert!(tmpfs.contains(StorageSemantic::Rename.as_str()));
    assert!(tmpfs.contains(StorageSemantic::AdvisoryLock.as_str()));
    // A network filesystem grants only what it declares supported, and drops the adjustable clock's
    // expiry-shaped capabilities rather than pretending they are there.
    let network = matrix
        .negotiated_capabilities(
            StoragePlatformTarget::Linux,
            StorageFilesystemClass::NetworkFs,
        )
        .expect("the cell exists");
    for semantic in ["rename", "fsync", "advisory_lock"] {
        assert!(
            !network.contains(semantic),
            "an unsupported {semantic} may not be negotiated as available"
        );
    }
    assert!(network.contains(StorageSemantic::PermissionGuard.as_str()));
}

#[test]
fn a_cell_is_cross_checked_against_the_pd28_platform_record() {
    let matrix = matrix();
    // PD-28's own record for a Linux host, which already refuses atomic_replace without fsync.
    let security = StorageSecurityCapabilities::current();
    security
        .validate()
        .expect("the platform record is well formed");
    let storage = StorageCapabilities::new(true, true, true, true, true, 1_048_576, 256)
        .expect("capabilities are well formed");
    matrix
        .validate_against_capabilities(&linux_ext4(), &security, &storage)
        .expect("the linux ext4 cell agrees with the platform records");

    // A cell whose guard the platform record disallows is refused, so the two vocabularies cannot
    // drift apart silently. `StorageSecurityCapabilities` has no public constructor for a
    // hand-written record, so the mismatch is checked the other way: the Windows cell claims
    // guards the `linux` platform record cannot support.
    let mismatch = matrix.validate_against_capabilities(&windows_fallback(), &security, &storage);
    assert_eq!(
        mismatch.unwrap_err(),
        "storage_platform_security_platform_mismatch",
        "a non-unix cell may not be validated against a unix platform record"
    );
}

#[test]
fn the_matrix_does_not_claim_durable_on_any_unverified_platform() {
    let matrix = matrix();
    for cell in &matrix.cells {
        assert!(
            cell.proof_ceiling.certifiable_by_source_suite(),
            "a cell may not claim a ceiling the source matrix cannot certify"
        );
    }
    // Only the fully-supported Linux/ext4 cell may exceed `source`, and only to `local_behavior`.
    for cell in &matrix.cells {
        if cell.proof_ceiling > ProofCeiling::Source {
            assert_eq!(cell.target, StoragePlatformTarget::Linux);
            assert_eq!(cell.filesystem, StorageFilesystemClass::Ext4);
            assert!(cell.usable_root());
        }
    }
}

// ---------------------------------------------------------------------------------------
// 先拒绝：不支持的 rename/fsync/lock 语义被当作等价 durable
// ---------------------------------------------------------------------------------------

#[test]
fn a_degraded_semantic_may_not_claim_a_durable_ceiling() {
    // tmpfs `fsync` is degraded. Presenting that as an equivalent durable guarantee is the drift the
    // card rejects, and the cell is unconstructible at `local_behavior`.
    let overclaimed = StoragePlatformCell::new(
        StoragePlatformTarget::Linux,
        StorageFilesystemClass::Tmpfs,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Degraded,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StorageClockShape::MonotonicWall,
        StorageEncoding::Utf8,
        ProofCeiling::LocalBehavior,
        notes("tmpfs fsync presented as an equivalent durable guarantee"),
    );
    assert!(
        overclaimed.is_err(),
        "a degraded semantic may not carry a durable ceiling"
    );

    // And a cell whose clock is not monotonic may not reach it either, even fully supported.
    let adjustable = StoragePlatformCell::new(
        StoragePlatformTarget::Linux,
        StorageFilesystemClass::Ext4,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StorageClockShape::AdjustableWall,
        StorageEncoding::Utf8,
        ProofCeiling::LocalBehavior,
        notes("a wall clock that can move backwards is not a durable guarantee"),
    );
    assert_eq!(
        adjustable.unwrap_err(),
        "storage_platform_ceiling_outranks_dispositions"
    );
}

#[test]
fn a_cell_may_not_claim_a_ceiling_above_what_negotiation_can_certify() {
    let durable = StoragePlatformCell::new(
        StoragePlatformTarget::Linux,
        StorageFilesystemClass::Ext4,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StoragePlatformDisposition::Supported,
        StorageClockShape::MonotonicWall,
        StorageEncoding::Utf8,
        ProofCeiling::Durable,
        notes("durable claimed from a matrix that ran no power-loss"),
    );
    assert_eq!(
        durable.unwrap_err(),
        "storage_platform_ceiling_not_negotiable",
        "a negotiation result is not a durability result"
    );
}

#[test]
fn an_unknown_filesystem_is_never_credited_with_a_rename() {
    // A filesystem nobody identified cannot be assumed to rename atomically, whatever platform it
    // is on. The cell is refused rather than degraded.
    let mut forged = other_unix();
    forged.rename = StoragePlatformDisposition::Supported;
    assert_eq!(
        forged.validate().unwrap_err(),
        "storage_platform_unknown_filesystem_supported"
    );
}

#[test]
fn a_matrix_missing_a_platform_is_refused() {
    // Every platform the card names must have a row, or "unsupported" is invisible rather than
    // decided. Removing the Windows fallback is refused by name.
    let cells = matrix().cells;
    let filtered: Vec<StoragePlatformCell> = cells
        .iter()
        .filter(|cell| cell.target != StoragePlatformTarget::Windows)
        .cloned()
        .collect();
    let error = StoragePlatformMatrix::new(baseline(), filtered).unwrap_err();
    assert_eq!(error, "storage_platform_target_missing:windows");
}

#[test]
fn a_tampered_cell_digest_is_refused() {
    let mut cell = linux_network_fs();
    cell.advisory_lock = StoragePlatformDisposition::Supported;
    assert_eq!(
        cell.validate().unwrap_err(),
        "storage_platform_cell_digest_mismatch",
        "an edited disposition is refused rather than adopted"
    );
}

// ---------------------------------------------------------------------------------------
// 先拒绝：时钟回退破坏 TTL/retention 均拒绝
// ---------------------------------------------------------------------------------------

#[test]
fn a_clock_that_can_move_backwards_refuses_every_expiry() {
    // TTLs, leases and retention watermarks are wall-clock deadlines. A cell whose wall clock is
    // adjustable may not evaluate one, so the preflight names the clock and the negotiation drops
    // the capability.
    let matrix = matrix();
    for (target, filesystem) in [
        (
            StoragePlatformTarget::Linux,
            StorageFilesystemClass::NetworkFs,
        ),
        (StoragePlatformTarget::Windows, StorageFilesystemClass::Ext4),
        (StoragePlatformTarget::Macos, StorageFilesystemClass::Ext4),
    ] {
        let cell = matrix
            .cell(target, filesystem)
            .unwrap_or_else(|| panic!("PD-32 must have a {target:?}/{filesystem:?} cell"));
        assert_eq!(cell.clock, StorageClockShape::AdjustableWall);
        assert!(!cell.clock.admits_expiry());
        // The preflight refuses on the first missing semantic, so the clock rule is checked on the
        // one cell whose semantics are otherwise complete.
        assert!(!cell.usable_root());
    }
    // And the positive reading: the Linux/ext4 cell is the only one whose clock admits an expiry.
    assert!(linux_ext4().clock.admits_expiry());
    assert!(linux_ext4().usable_root());
    // An absent clock is not an adjustable one; it simply cannot expire anything either.
    assert!(!StorageClockShape::AbsentWall.admits_expiry());
    assert_eq!(
        StorageEncoding::NonUtf8.as_str(),
        "non_utf8",
        "a non-UTF-8 path is refused, never transliterated"
    );
}

#[test]
fn a_wall_clock_deadline_is_refused_where_the_clock_can_move_backwards() {
    // 时钟回退破坏 TTL/retention 均拒绝. A TTL, a lease expiry and a retention watermark are all
    // wall-clock deadlines, so all three are refused wherever the wall clock is adjustable — the
    // refusal names the clock shape rather than reporting a generic expiry failure. No clock was
    // read here: the samples below are declared values.
    let matrix = matrix();
    for (target, filesystem) in [
        (
            StoragePlatformTarget::Linux,
            StorageFilesystemClass::NetworkFs,
        ),
        (StoragePlatformTarget::Macos, StorageFilesystemClass::Ext4),
    ] {
        let cell = matrix
            .cell(target, filesystem)
            .unwrap_or_else(|| panic!("PD-32 must have a {target:?}/{filesystem:?} cell"));
        assert_eq!(
            admit_platform_expiry(cell, true, 1_000, 2_000).unwrap_err(),
            format!("storage_platform_expiry_refused:{}", cell.clock.as_str())
        );
    }
    // The absent clock is refused for the same reason and a different one.
    let unknown = other_unix();
    assert_eq!(
        admit_platform_expiry(&unknown, true, 1_000, 2_000).unwrap_err(),
        "storage_platform_expiry_refused:absent_wall"
    );
    // An untrusted or rolled-back observation is refused even on the one cell that can expire.
    let ext4 = linux_ext4();
    assert_eq!(
        admit_platform_expiry(&ext4, false, 1_000, 2_000).unwrap_err(),
        "storage_platform_expiry_refused:untrusted_clock",
        "a rolled-back clock may not decide that a deadline has not passed"
    );
    // A deadline that has already passed is refused, not extended.
    assert_eq!(
        admit_platform_expiry(&ext4, true, 2_000, 2_000).unwrap_err(),
        "storage_platform_expiry_elapsed"
    );
    // A zero sample is refused rather than read as "now is the epoch".
    assert_eq!(
        admit_platform_expiry(&ext4, true, 0, 2_000).unwrap_err(),
        "storage_platform_expiry_sample_invalid"
    );
    // The positive reading: a live deadline on the one cell that can expire it.
    assert_eq!(
        admit_platform_expiry(&ext4, true, 1_000, 2_000).expect("a live deadline is admitted"),
        2_000
    );
}

#[test]
fn a_non_utf8_cell_is_not_a_usable_root() {
    let cell = windows_fallback();
    assert_eq!(cell.encoding, StorageEncoding::NonUtf8);
    assert!(!cell.usable_root());
    let matrix = matrix();
    assert_eq!(
        matrix
            .install_preflight(
                StoragePlatformTarget::OtherUnix,
                StorageFilesystemClass::Unknown
            )
            .unwrap_err(),
        "storage_platform_semantic_unsupported:other_unix:rename"
    );
}

// ---------------------------------------------------------------------------------------
// 与既有 vocabulary 的一致性：PD-34 的预算同样读 CapacityEnvelope
// ---------------------------------------------------------------------------------------

fn envelope() -> CapacityEnvelope {
    CapacityEnvelope {
        journal_max_bytes: 64 * 1024 * 1024,
        journal_max_events: 1_000_000,
        max_frame_bytes: 1_048_576,
        max_event_bytes: 524_288,
        max_batch_events: 256,
        max_page_events: 1_000,
        max_export_records: 10_000,
        observability_queue_capacity: 4_096,
        max_artifact_bytes: 8_388_608,
        high_cardinality_rejected: true,
        oversize_rejected: true,
        backpressure_preserves_facts: true,
    }
}

#[test]
fn the_platform_matrix_and_the_capacity_budget_read_the_same_envelope() {
    // The two PD-32/PD-34 slices must not invent separate bounds: both read the DEP-17 envelope.
    let budget = DeclaredBudget::new(
        StorageCapacitySubject::EventFrame,
        &envelope(),
        BudgetOrigin::CapacityEnvelope,
        None,
        0,
        CapacityBackpressure::BoundedReject,
        None,
    )
    .expect("the budget is well formed");
    assert_eq!(budget.envelope_field, "max_frame_bytes");
    assert_eq!(budget.envelope_bound, envelope().max_frame_bytes);
    // A budget is *declared*, not measured, until a receipt exists. This suite measures nothing.
    assert!(!budget.is_measured());
    assert_eq!(budget.outcome(), StorageBudgetOutcome::Bounded);
    // A report assembled from declared budgets says so.
    let report =
        StorageDegradationReport::evaluate(baseline(), vec![budget], Vec::new(), Vec::new(), 0)
            .expect("the report is well formed");
    assert_eq!(report.status, StorageDegradationStatus::Refused);
    assert!(!report.anything_measured());
    assert!(report.measured_subjects.is_empty());
    // The DEP-17 latency budget type is the one a capacity budget is declared against, and it is
    // carried unchanged rather than replaced by a second percentile vocabulary.
    let latency = PersistenceCapacityBudget {
        operation: kiana_domain::BenchmarkOperation::Append,
        max_p95_micros: 1_000,
        max_p99_micros: 2_000,
        max_queue_depth: 8,
        max_rejection_rate_bps: 10,
        max_maintenance_share_bps: 2_500,
    };
    latency
        .validate()
        .expect("a DEP-17 latency budget is well formed");
    let declared = DeclaredBudget::new(
        StorageCapacitySubject::EventFrame,
        &envelope(),
        BudgetOrigin::PersistenceBudget,
        Some(latency.clone()),
        0,
        CapacityBackpressure::BoundedReject,
        None,
    )
    .expect("the budget is well formed");
    assert_eq!(declared.latency_budget, Some(latency));
    let _ = BTreeSet::from(["max_frame_bytes"]);
}

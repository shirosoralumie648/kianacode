//! PD-30 / PD-32 / PD-34 source guard: the fault matrix, the platform matrix and the capacity
//! budgets are decisions, and none of the three performs an effect or an observation.
//!
//! Every marker below was grepped out of the real source before this test was written, and the
//! exact-line assertions were checked to occur exactly once so a rename cannot silently satisfy
//! them. The forbidden list is checked against the same three files: a matrix that killed a
//! process, statfs'd a mount or measured a latency would be claiming runtime evidence from a
//! source contract.

/// The three files this guard covers, read once so the tests below stay short.
fn sources() -> [(&'static str, &'static str); 3] {
    [
        ("PD-30", include_str!("../src/storage_fault_matrix.rs")),
        ("PD-32", include_str!("../src/storage_platform_matrix.rs")),
        ("PD-34", include_str!("../src/storage_capacity_budget.rs")),
    ]
}

fn assert_markers(step: &str, source: &str, markers: &[&str]) {
    for marker in markers {
        assert!(source.contains(marker), "{step} marker missing: {marker}");
    }
}

/// Assert an exact line occurs **exactly once** in the source. A `contains` on a short line such
/// as `Self::Fsync,` would pass even if the ordered list were rearranged, so the guard pins the
/// exact text and the count.
fn assert_exact_once(step: &str, source: &str, line: &str) {
    let count = source.matches(line).count();
    assert_eq!(
        count, 1,
        "{step} must contain exactly one `{line}`, found {count}"
    );
}

#[test]
fn pd30_32_34_are_registered_in_the_ports_crate_root() {
    let lib = include_str!("../src/lib.rs");
    for marker in [
        "mod storage_fault_matrix;",
        "mod storage_platform_matrix;",
        "mod storage_capacity_budget;",
        "pub use storage_fault_matrix::{",
        "pub use storage_platform_matrix::{",
        "pub use storage_capacity_budget::{",
        "admit_fault_restart,",
        "admit_platform_expiry,",
        "storage_fault_scope,",
        "storage_platform_scope,",
        "StorageDegradationReport",
        "StoragePlatformMatrix",
        "StorageFaultMatrix",
    ] {
        assert!(lib.contains(marker), "PD-30/32/34 export missing: {marker}");
    }
}

#[test]
fn pd30_the_fault_matrix_covers_every_named_fault_and_refuses_every_rejection() {
    let source = sources()[0].1;
    assert_markers(
        "PD-30",
        source,
        &[
            // the card's seven faults
            "KillNine",
            "DiskFull",
            "PermissionDenied",
            "LockContention",
            "CorruptFrame",
            "CrashTiming",
            "NetworkUnknown",
            // the types
            "pub enum StorageFaultKind",
            "pub enum StorageFaultRecovery",
            "pub enum StorageFaultExit",
            "pub enum StorageFaultMatrixStatus",
            "pub struct StorageFaultCase",
            "pub struct StorageFaultMatrix",
            "pub fn admit_fault_restart",
            "pub fn storage_fault_scope",
            "STORAGE_FAULT_MATRIX_SCHEMA",
            "STORAGE_FAULT_CASE_SCHEMA",
            "STORAGE_FAULT_VERSION",
            "MAX_STORAGE_FAULT_CASES",
            // the three rejections the card names
            "storage_fault_completion_claimed",
            "storage_fault_duplicate_effect",
            "storage_fault_recovery_beyond_authority",
            // the delivery column
            "expected_class",
            "StorageFaultExit::for_fault",
            "settles_reconciling_class",
            "storage_fault_limitation_required",
            // every other refusal code the case and matrix can return
            "storage_fault_class_mismatch",
            "storage_fault_crash_point_invalid",
            "storage_fault_case_unsafe",
            "storage_fault_effect_confirmation_invalid",
            "storage_fault_unknown_outcome_not_fenced",
            "storage_fault_corrupt_not_quarantined",
            "storage_fault_exit_code_invalid",
            "storage_fault_kind_duplicate",
            "storage_fault_kind_missing",
            "storage_fault_case_invalid",
            "storage_fault_matrix_header_invalid",
            "storage_fault_matrix_binding_invalid",
            "storage_fault_matrix_refused_with_reason",
            "storage_fault_matrix_unsafe_without_reason",
            "storage_fault_restart_cursor_not_established",
            "storage_fault_restart_not_fenced",
            // vocabulary that must be reused, not reinvented
            "StorageErrorClass",
            "FaultInjectionPoint",
            "AdapterKind",
        ],
    );
}

#[test]
fn pd30_the_seven_faults_appear_in_the_cards_order_and_exactly_once() {
    let source = sources()[0].1;
    // `as_str` repeats the same wire names, so a file-wide search would pass even if `ALL` were
    // rearranged. Pin the list block itself and check each variant appears exactly once *in it*.
    let start = source
        .find("pub const ALL: [Self; 7] = [")
        .expect("PD-30 must declare the full fault set");
    let end = source[start..]
        .find("];")
        .map(|offset| start + offset)
        .expect("PD-30 must close the fault list");
    let list = &source[start..end];
    let mut cursor = 0;
    for (variant, wire) in [
        ("KillNine", "\"kill_nine\""),
        ("DiskFull", "\"disk_full\""),
        ("PermissionDenied", "\"permission_denied\""),
        ("LockContention", "\"lock_contention\""),
        ("CorruptFrame", "\"corrupt_frame\""),
        ("CrashTiming", "\"crash_timing\""),
        ("NetworkUnknown", "\"network_unknown\""),
    ] {
        assert_exact_once("PD-30", list, &format!("Self::{variant},"));
        assert!(
            source.contains(wire),
            "PD-30 must name the fault on the wire as {wire}"
        );
        let at = list[cursor..]
            .find(&format!("Self::{variant},"))
            .unwrap_or_else(|| panic!("PD-30 must declare {variant} in ALL order"));
        cursor += at;
    }
    // The card's three rejections are decided in a fixed order, and the codes are unique so an
    // earlier violation always wins.
    assert_exact_once(
        "PD-30",
        source,
        "format!(\"storage_fault_completion_claimed:{}\", case.kind.as_str())",
    );
    assert_exact_once(
        "PD-30",
        source,
        "format!(\"storage_fault_duplicate_effect:{}\", case.kind.as_str())",
    );
    assert_exact_once(
        "PD-30",
        source,
        "\"storage_fault_recovery_beyond_authority:{}\",",
    );
    // The decision order in `derive` is the card's: the three rejections first, then the absences
    // of proof in the order a caller would be most misled by them. An earlier violation must always
    // win, so the reported reason is the same for the same facts.
    let mut cursor = source
        .find("fn derive(")
        .expect("PD-30 must have a single decision function");
    for code in [
        "storage_fault_completion_claimed",
        "storage_fault_duplicate_effect",
        "storage_fault_recovery_beyond_authority",
        "storage_fault_unknown_outcome_not_fenced",
        "storage_fault_corrupt_not_quarantined",
        "storage_fault_exit_code_invalid",
        "storage_fault_limitation_required",
    ] {
        let at = source[cursor..]
            .find(code)
            .unwrap_or_else(|| panic!("PD-30 decision order must reach {code} in order"));
        cursor += at + code.len();
    }
    // And the safety invariants are refused at the case level, not only at the matrix level.
    assert_exact_once(
        "PD-30",
        source,
        "if self.completion_claimed || self.duplicate_effect || self.recovery_beyond_authority {",
    );
    // A reconciling class is never answered with "nothing happened".
    assert_exact_once(
        "PD-30",
        source,
        "if self.class().requires_reconciliation() && !self.recovery.settles_reconciling_class() {",
    );
    // A corrupt frame is quarantined, never "recovered" by truncating it.
    assert_exact_once(
        "PD-30",
        source,
        "if self.class() == StorageErrorClass::Corrupt\n            && self.recovery != StorageFaultRecovery::QuarantineForOperator\n        {",
    );
    // A faulted store may not exit zero.
    assert_exact_once(
        "PD-30",
        source,
        "if self.exit_code != StorageFaultExit::for_fault(self.class(), self.effect_started) {",
    );
    assert!(source.contains("pub const fn as_i32(self) -> i32 {"));
    assert!(source.contains("Self::Clean => 0,"));
}

#[test]
fn pd30_reuses_the_existing_fault_and_storage_vocabulary() {
    let source = sources()[0].1;
    // The state classification IS `StorageErrorClass`, read off the adapter's own error rather
    // than re-typed on the case. A parallel enum would be the second vocabulary the card forbids.
    assert!(
        source.contains("use crate::adapter_conformance::AdapterKind;"),
        "PD-30 must bind a fault to a PD-31 adapter kind"
    );
    for reused in ["StorageError", "StorageErrorClass", "FaultInjectionPoint"] {
        assert!(
            source.contains(&format!("    {reused},")) || source.contains(&format!("{reused},")),
            "PD-30 must import the existing {reused}"
        );
    }
    // The class is read off the error, and crash timing is the only fault on the injection timeline.
    assert_exact_once(
        "PD-30",
        source,
        "pub fn class(&self) -> StorageErrorClass {\n        self.error.class\n    }",
    );
    assert_exact_once(
        "PD-30",
        source,
        "pub const fn expects_crash_point(self) -> bool {\n        matches!(self, Self::CrashTiming)\n    }",
    );
    assert_exact_once(
        "PD-30",
        source,
        "if self.kind.expects_crash_point() != self.crash_point.is_some() {",
    );
    // A refusal is a stable code plus a sealed digest, not a boolean.
    assert!(source.contains("fn safe_text(value: &str, field: &str)"));
    assert!(source.contains("scan_secret_sentinels(SecretScanChannel::Receipt, value)"));
    assert!(source.contains("if redact_text(value) != value {"));
}

#[test]
fn pd32_the_platform_matrix_names_every_platform_and_filesystem_with_a_disposition() {
    let source = sources()[1].1;
    assert_markers(
        "PD-32",
        source,
        &[
            // the card's platforms and filesystems
            "Linux",
            "Windows",
            "Macos",
            "OtherUnix",
            "Ext4",
            "Tmpfs",
            "NetworkFs",
            "Unknown",
            // the three semantics the card names, plus the two PD-28 already gates
            "pub enum StorageSemantic",
            "Rename",
            "Fsync",
            "AdvisoryLock",
            "PermissionGuard",
            "LinkGuard",
            // supported / degraded / unsupported
            "pub enum StoragePlatformDisposition",
            "Supported",
            "Degraded",
            "Unsupported",
            // clock and encoding
            "pub enum StorageClockShape",
            "MonotonicWall",
            "AdjustableWall",
            "AbsentWall",
            "pub enum StorageEncoding",
            "NonUtf8",
            // the types and the negotiation
            "pub struct StoragePlatformCell",
            "pub struct StoragePlatformMatrix",
            "pub enum SecurityCapability",
            "pub fn install_preflight",
            "pub fn negotiated_capabilities",
            "pub fn validate_against_capabilities",
            "pub fn admit_platform_expiry",
            "pub fn storage_platform_scope",
            "STORAGE_PLATFORM_MATRIX_SCHEMA",
            "STORAGE_PLATFORM_CELL_SCHEMA",
            "STORAGE_PLATFORM_BUILD_TARGET",
            "admits_durable_claim",
            "admits_expiry",
            "clock_can_expire",
            "certifiable_by_source_suite",
            // vocabulary that must be reused
            "StorageSecurityCapabilities",
            "StorageCapabilities",
            "ProofCeiling",
            // every refusal code
            "storage_platform_target_missing",
            "storage_platform_cell_duplicate",
            "storage_platform_ceiling_outranks_dispositions",
            "storage_platform_ceiling_not_negotiable",
            "storage_platform_limitation_required",
            "storage_platform_limitation_limit",
            "storage_platform_unknown_filesystem_supported",
            "storage_platform_other_unix_ceiling",
            "storage_platform_semantic_unsupported",
            "storage_platform_clock_not_expirable",
            "storage_platform_encoding_refused",
            "storage_platform_guard_not_enforceable",
            "storage_platform_security_platform_mismatch",
            "storage_platform_cell_digest_mismatch",
            "storage_platform_matrix_baseline_digest",
            "storage_platform_degraded_ceiling",
            "storage_platform_expiry_refused",
            "storage_platform_expiry_elapsed",
            "storage_platform_expiry_sample_invalid",
        ],
    );
}

#[test]
fn pd32_an_unsupported_semantic_is_refused_and_never_substituted() {
    let source = sources()[1].1;
    // Every target and every filesystem is declared, in the card's order, exactly once in `ALL`.
    let target_list = slice_between(source, "StoragePlatformTarget");
    let mut target_cursor = 0;
    for (variant, wire) in [
        ("Linux", "\"linux\""),
        ("Windows", "\"windows\""),
        ("Macos", "\"macos\""),
        ("OtherUnix", "\"other_unix\""),
    ] {
        assert!(
            target_list.contains(&format!("Self::{variant}")),
            "PD-32 must declare {variant} in StoragePlatformTarget::ALL"
        );
        // `rustfmt` collapses a four-element array onto one line, so the variants are asserted by
        // containment inside the owner-specific block plus an in-order walk, rather than by an
        // exact per-line match that a reformat would silently break.
        let at = target_list[target_cursor..]
            .find(&format!("Self::{variant}"))
            .unwrap_or_else(|| panic!("PD-32 must declare {variant} in ALL order"));
        target_cursor += at;
        assert!(source.contains(wire), "PD-32 must name {wire} on the wire");
    }
    let fs_list = slice_between(source, "StorageFilesystemClass");
    let mut fs_cursor = 0;
    for (variant, wire) in [
        ("Ext4", "\"ext4\""),
        ("Tmpfs", "\"tmpfs\""),
        ("NetworkFs", "\"network_fs\""),
        ("Unknown", "\"unknown\""),
    ] {
        assert!(
            fs_list.contains(&format!("Self::{variant}")),
            "PD-32 must declare {variant} in StorageFilesystemClass::ALL"
        );
        let at = fs_list[fs_cursor..]
            .find(&format!("Self::{variant}"))
            .unwrap_or_else(|| panic!("PD-32 must declare {variant} in ALL order"));
        fs_cursor += at;
        assert!(source.contains(wire), "PD-32 must name {wire} on the wire");
    }
    // The two `ALL` lines are pinned exactly, because each names only its own type's variants and
    // they are the one place where a reordering of the card's list would be visible.
    assert_exact_once(
        "PD-32",
        source,
        "pub const ALL: [Self; 4] = [Self::Linux, Self::Windows, Self::Macos, Self::OtherUnix];",
    );
    assert_exact_once(
        "PD-32",
        source,
        "pub const ALL: [Self; 4] = [Self::Ext4, Self::Tmpfs, Self::NetworkFs, Self::Unknown];",
    );
    // A semantic the cell does not support is *absent* from the negotiation, not present-and-false.
    assert_exact_once(
        "PD-32",
        source,
        ".filter(|(_, disposition)| disposition.admits_durable_claim())\n            .map(|(name, _)| name)\n            .collect();",
    );
    // A degraded semantic is not a supported one: only `Supported` admits a durable claim.
    assert_exact_once(
        "PD-32",
        source,
        "pub const fn admits_durable_claim(self) -> bool {\n        matches!(self, Self::Supported)\n    }",
    );
    // The preflight names the primitive and the platform, so the refusal is actionable.
    assert_exact_once(
        "PD-32",
        source,
        "\"storage_platform_semantic_unsupported:{}:{}\",\n                cell.target.as_str(),\n                semantic.as_str()",
    );
    // A cell whose clock cannot expire may not claim a durable ceiling even fully supported.
    assert_exact_once(
        "PD-32",
        source,
        "if self.proof_ceiling == ProofCeiling::LocalBehavior\n            && !(self.all_supported() && self.clock.admits_expiry())\n        {",
    );
    // And a cell may never claim a ceiling the negotiation cannot certify at all.
    assert_exact_once(
        "PD-32",
        source,
        "if self.proof_ceiling > ProofCeiling::LocalBehavior {",
    );
    // An unrecognised Unix inherits nothing from the Linux row.
    assert_exact_once(
        "PD-32",
        source,
        "if self.target == StoragePlatformTarget::OtherUnix\n            && self.proof_ceiling != ProofCeiling::Source\n        {",
    );
    // An unidentified filesystem is never credited with an atomic rename.
    assert_exact_once(
        "PD-32",
        source,
        "if self.filesystem == StorageFilesystemClass::Unknown\n            && self.rename != StoragePlatformDisposition::Unsupported\n        {",
    );
}

#[test]
fn pd32_a_rollback_clock_refuses_every_expiry() {
    let source = sources()[1].1;
    // 时钟回退破坏 TTL/retention 均拒绝. The gate is a single function and the clock check comes
    // before the trust check, so an un-expirable platform is refused on the platform, not the
    // caller's clock sample.
    assert_exact_once(
        "PD-32",
        source,
        "pub fn admit_platform_expiry(\n    cell: &StoragePlatformCell,\n    clock_trusted: bool,\n    wall_now_unix_ms: u64,\n    expires_at_unix_ms: u64,\n) -> Result<u64, String> {",
    );
    assert_exact_once(
        "PD-32",
        source,
        "if !cell.clock_can_expire() {\n        return Err(format!(\n            \"storage_platform_expiry_refused:{}\",\n            cell.clock.as_str()\n        ));\n    }",
    );
    assert_exact_once(
        "PD-32",
        source,
        "if !clock_trusted {\n        return Err(\"storage_platform_expiry_refused:untrusted_clock\".to_owned());\n    }",
    );
    // Only a monotonic wall clock may decide an expiry.
    assert_exact_once(
        "PD-32",
        source,
        "pub const fn admits_expiry(self) -> bool {\n        matches!(self, Self::MonotonicWall)\n    }",
    );
    // The same rule governs the pre-install check, so a root cannot be placed where a TTL is
    // undecidable.
    assert_exact_once(
        "PD-32",
        source,
        "if !cell.clock_can_expire() {\n            return Err(format!(\n                \"storage_platform_clock_not_expirable:{}\",\n                cell.target.as_str()\n            ));\n        }",
    );
    // A non-UTF-8 path is refused, never transliterated into a different path.
    assert_exact_once(
        "PD-32",
        source,
        "if cell.encoding != StorageEncoding::Utf8 {\n            return Err(format!(\n                \"storage_platform_encoding_refused:{}\",\n                cell.target.as_str()\n            ));\n        }",
    );
}

#[test]
fn pd32_the_cell_is_cross_checked_against_the_pd28_capability_records() {
    let source = sources()[1].1;
    // The two vocabularies cannot drift: a cell may not claim a guard the PD-28 record disallows,
    // and the fsync bit is the one `StorageCapabilities` already refuses durable commits without.
    assert_exact_once(
        "PD-32",
        source,
        "pub fn validate_against_capabilities(\n        &self,\n        cell: &StoragePlatformCell,\n        security: &StorageSecurityCapabilities,\n        storage: &StorageCapabilities,\n    ) -> Result<(), String> {",
    );
    assert_exact_once(
        "PD-32",
        source,
        "if security.platform != cell.target.security_platform() {",
    );
    assert_exact_once(
        "PD-32",
        source,
        "if cell.admits(StorageSemantic::Fsync) && !storage.fsync {\n            return Err(\"storage_platform_guard_not_enforceable:fsync\".to_owned());\n        }",
    );
    // `AdvisoryLock` has no PD-28 guard record, and neither does `Fsync`; both say so rather than
    // borrowing a guard record they do not have.
    assert_exact_once(
        "PD-32",
        source,
        "            Self::Rename => Some(SecurityCapability::AtomicReplace),\n            Self::Fsync => None,\n            Self::AdvisoryLock => None,",
    );
}

#[test]
fn pd34_the_budgets_are_declared_with_their_measurement_method_and_never_invented() {
    let source = sources()[2].1;
    assert_markers(
        "PD-34",
        source,
        &[
            // the card's subjects
            "EventFrame",
            "Artifact",
            "Index",
            "BackupPrune",
            "WriterQueue",
            // where a number comes from, and how it would be measured
            "pub enum BudgetOrigin",
            "pub enum BudgetMeasurement",
            "CapacityEnvelope",
            "WriterQueuePolicy",
            "PersistenceBudget",
            "DeclaredByReview",
            "SustainedAppend",
            "RebuildUnderRead",
            "ArtifactStageCommit",
            "MaintenanceUnderLoad",
            "QueueContention",
            // the report
            "pub struct DeclaredBudget",
            "pub struct CapacityRefusalObservation",
            "pub struct StorageDegradationReport",
            "pub enum CapacityBackpressure",
            "pub enum StorageBudgetOutcome",
            "pub enum StorageDegradationStatus",
            "STORAGE_BUDGET_SCHEMA",
            "STORAGE_DEGRADATION_SCHEMA",
            "STORAGE_DEGRADATION_VERSION",
            "MAX_MAINTENANCE_SHARE_BPS",
            // the DEP-17 vocabulary that must be reused
            "PersistenceCapacityBudget",
            "BenchmarkSummary",
            "p95",
            "p99",
            // every refusal code
            "capacity_subject_not_bounded",
            "capacity_budget_measurement_missing",
            "capacity_budget_origin_missing",
            "capacity_budget_receipt_missing",
            "capacity_backpressure_dropped_facts",
            "capacity_maintenance_budget_exceeded",
            "capacity_reader_blocked_writer",
            "capacity_budget_duplicate",
            "capacity_budget_invalid",
            "capacity_budget_envelope_field_mismatch",
            "capacity_budget_envelope_bound_mismatch",
            "capacity_budget_digest_mismatch",
            "capacity_budget_measurement_mismatch",
            "capacity_observation_subject_undeclared",
            "capacity_refusal_observation_invalid",
            "capacity_refusal_observation_digest_mismatch",
            "storage_degradation_report_header_invalid",
            "storage_degradation_report_binding_invalid",
            "storage_degradation_maintenance_share_invalid",
            "storage_degradation_summary_invalid",
            "storage_degradation_refused_with_reason",
            "storage_degradation_exceeded_without_reason",
            "storage_degradation_measured_subject_unknown",
            "storage_degradation_baseline_digest",
        ],
    );
}

#[test]
fn pd34_a_budget_reads_the_existing_envelope_rather_than_declaring_a_second_bound() {
    let source = sources()[2].1;
    // The card's first rejection: an unbounded frame/batch/queue. A budget's bound is the envelope's
    // bound for that subject, so a second set of numbers cannot appear.
    // The five subjects are declared in the card's order, pinned to their own block.
    let subject_list = slice_between(source, "StorageCapacitySubject");
    let mut cursor = 0;
    for variant in [
        "EventFrame",
        "Artifact",
        "Index",
        "BackupPrune",
        "WriterQueue",
    ] {
        assert_exact_once("PD-34", subject_list, &format!("Self::{variant},"));
        let at = subject_list[cursor..]
            .find(&format!("Self::{variant},"))
            .unwrap_or_else(|| panic!("PD-34 must declare {variant} in ALL order"));
        cursor += at;
    }
    // The writer queue is included because PD-27's `WriterQueuePolicy` already bounds it; a queue
    // with no subject would be invisible to the report.
    assert!(source.contains("bounded by the PD-27 `WriterQueuePolicy` caps"));
    assert!(source.contains("Self::WriterQueue => \"max_batch_events\","));
    assert_exact_once(
        "PD-34",
        source,
        "pub const fn envelope_field(self) -> &'static str {",
    );
    assert_exact_once(
        "PD-34",
        source,
        "pub fn envelope_bound(self, envelope: &CapacityEnvelope) -> u64 {",
    );
    assert_exact_once(
        "PD-34",
        source,
        "if self.envelope_field != self.subject.envelope_field() {\n            return Err(\"capacity_budget_envelope_field_mismatch\".to_owned());\n        }",
    );
    assert_exact_once(
        "PD-34",
        source,
        "if self.envelope_bound != self.subject.envelope_bound(envelope) {\n            return Err(\"capacity_budget_envelope_bound_mismatch\".to_owned());\n        }",
    );
    assert_exact_once("PD-34", source, "if budget.envelope_bound == 0 {");
    // The measurement method is always declared, and it is fixed per subject so two subjects
    // cannot be measured by two different means without the difference showing in the report.
    assert_exact_once(
        "PD-34",
        source,
        "if self.measurement != BudgetMeasurement::for_subject(self.subject) {\n            return Err(\"capacity_budget_measurement_mismatch\".to_owned());\n        }",
    );
    // A claimed pass with no receipt is refused: the claim and the evidence are separate fields.
    assert_exact_once(
        "PD-34",
        source,
        "pub fn is_measured(&self) -> bool {\n        self.measurement_receipt_digest.is_some()\n    }",
    );
    assert_exact_once("PD-34", source, "\"capacity_budget_receipt_missing\",");
}

#[test]
fn pd34_maintenance_and_a_long_reader_are_both_refused() {
    let source = sources()[2].1;
    // maintenance 挤占用户命令: the share is basis points and may not exceed 100%.
    assert_exact_once(
        "PD-34",
        source,
        "pub const MAX_MAINTENANCE_SHARE_BPS: u16 = 10_000;",
    );
    assert_exact_once(
        "PD-34",
        source,
        ".find(|budget| maintenance_share_bps > budget.max_maintenance_share_bps)",
    );
    // 长 reader 阻塞 writer: a reader that held the writer is named on its own.
    assert_exact_once(
        "PD-34",
        source,
        "if let Some(observation) = observations\n        .iter()\n        .find(|observation| observation.reader_held_writer)\n    {",
    );
    // And the facts-preserving rule, which is what separates a degradation from a loss.
    assert_exact_once(
        "PD-34",
        source,
        "pub const fn preserves_accepted_facts(self) -> bool {",
    );
    assert_exact_once(
        "PD-34",
        source,
        "if !self.committed_facts_retained {\n            return Err(\"capacity_backpressure_dropped_facts\".to_owned());\n        }",
    );
    // A capacity refusal may not be filed under a PD-30 reconciling class.
    assert_exact_once(
        "PD-34",
        source,
        "|| (self.rejected > 0\n                && !matches!(\n                    self.error_class,\n                    StorageErrorClass::Unavailable | StorageErrorClass::Conflict\n                ))",
    );
}

#[test]
fn pd34_the_decision_order_is_fixed_and_its_codes_are_unique() {
    let source = sources()[2].1;
    // The order is the card's: unbounded, declaration rules, dropped facts, maintenance share,
    // reader-held writer. Each is pinned so a reordering is visible in the diff.
    let order = [
        "capacity_subject_not_bounded",
        "capacity_budget_measurement_missing",
        "capacity_budget_origin_missing",
        "capacity_backpressure_dropped_facts",
        "capacity_observation_subject_undeclared",
        "storage_degradation_summary_invalid",
        "capacity_maintenance_budget_exceeded",
        "capacity_reader_blocked_writer",
        "capacity_budget_receipt_missing",
    ];
    let mut cursor = source
        .find("fn derive(")
        .expect("PD-34 must have a single decision function");
    for code in order {
        let at = source[cursor..]
            .find(code)
            .unwrap_or_else(|| panic!("PD-34 decision order must reach {code} in order"));
        cursor += at + code.len();
    }
    // A budget's declared digest seals the numbers; a report's digest seals the decision.
    assert_exact_once(
        "PD-34",
        source,
        "if self.report_digest != self.digest() {\n            return Err(\"storage_degradation_report_digest_mismatch\".to_owned());\n        }",
    );
    // Nothing in the report is measured unless a receipt says so, and `evaluate` reports the
    // measured subjects from the receipts rather than from a claim.
    assert_exact_once(
        "PD-34",
        source,
        "pub fn anything_measured(&self) -> bool {\n        !self.summaries.is_empty() || !self.measured_subjects.is_empty()\n    }",
    );
}

#[test]
fn none_of_the_three_performs_an_effect_or_reads_the_platform() {
    for (step, source) in sources() {
        // No process, no filesystem, no clock, no network. A matrix that killed a process, filled
        // a disk, statfs'd a mount or timed an append would be claiming runtime evidence from a
        // source contract. Each forbidden string is written with a leading `::` so a marker *name*
        // like `uses_storage_fsync` cannot satisfy or trip the assertion.
        for forbidden in [
            "::fs::",
            "::process::",
            "::net::",
            "::time::",
            "::thread::",
            "::Instant",
            "::SystemTime",
            "::PathBuf",
            "::File",
            "::OpenOptions",
            "::Command",
            "::libc",
            "rusqlite",
            "sqlx",
            "CapabilityBroker",
            "execute_capability",
            "std::fs",
            "std::process",
            "std::net",
            "std::thread",
            "std::path",
        ] {
            assert!(
                !source.contains(forbidden),
                "{step} crossed the effect boundary: {forbidden}"
            );
        }
        // A real syscall is written `libc::fsync(`, `::fsync(`, `sync_all(` or `fs::rename(`,
        // and every forbidden prefix above already catches those. This list catches the bare
        // spellings with no namespace at all, checked over *code lines only* — the module
        // docstrings name `fsync`/`flock`/`rename` to say these modules never call them, and a
        // comment is not a call. The `pub const fn uses_storage_fsync(` *name* is excluded by
        // requiring a non-identifier boundary before the `(`.
        for call in ["fsync(", "rename(", "flock(", "statfs(", "sync_all("] {
            for line in code_lines(source) {
                let Some(at) = line.find(call) else {
                    continue;
                };
                let boundary_is_identifier = line[..at]
                    .chars()
                    .next_back()
                    .is_some_and(|character| character.is_alphanumeric() || character == '_');
                assert!(
                    boundary_is_identifier,
                    "{step} crossed the effect boundary: a bare `{call}` call in `{line}`"
                );
            }
        }
    }
}

#[test]
fn the_baselines_state_plainly_what_none_of_this_proves() {
    for baseline in [
        include_str!("../../docs/roadmap/pd30-storage-fault-matrix-baseline.md"),
        include_str!("../../docs/roadmap/pd32-storage-platform-matrix-baseline.md"),
        include_str!("../../docs/roadmap/pd34-capacity-budget-baseline.md"),
    ] {
        for marker in ["proof", "does NOT prove", "source", "Linux", "partial"] {
            assert!(
                baseline.contains(marker),
                "PD-30/32/34 baseline marker missing: {marker}"
            );
        }
    }
}

/// The source with doc comments and line comments stripped.
///
/// The three modules name `fsync`, `flock` and `rename` in their prose to say they never call
/// them. Checking the effect boundary against the raw text would trip on the explanation, so the
/// call-shape checks run over code only.
fn code_lines(source: &str) -> Vec<&str> {
    let mut kept: Vec<&str> = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        kept.push(line);
    }
    kept
}

/// Slice the `ALL` list that belongs to `owner`.
///
/// `StoragePlatformTarget` and `StorageFilesystemClass` both declare `pub const ALL: [Self; 4]`, so
/// a plain `find` would return the target list for both and the filesystem assertions would be
/// satisfied by the wrong block. The slice runs from the owner's own `impl` block to its next
/// `impl`, so each assertion is pinned to its own type and the search never leaves that window.
fn slice_between<'a>(source: &'a str, owner: &str) -> &'a str {
    let start = source
        .find(&format!("impl {owner} {{"))
        .unwrap_or_else(|| panic!("{owner} must have an impl block"));
    let rest = &source[start + owner.len() + 6..];
    let end = rest
        .find("];")
        .map(|offset| start + owner.len() + 6 + offset)
        .unwrap_or_else(|| panic!("{owner} must close its ALL list"));
    &source[start..end]
}

//! PD-31 shared adapter conformance suite and capability matrix.
//!
//! One suite, every adapter. The checks live here rather than in a single adapter's test target so
//! that event ordering, capability honesty and refusal codes cannot drift between the Memory,
//! JSONL, future durable-file and future SQLite adapters. An adapter is expected to publish an
//! [`AdapterConformance`] declaration and to pass [`run_event_store_conformance`] unchanged.
//!
//! # What this is and is not
//!
//! This is a **source contract over declarations and logical outcomes**. It does not open a file,
//! fsync a page, restart a process, drive a power-loss, or contact a database engine. Declaring
//! [`AdapterKind::DurableFile`] or [`AdapterKind::Sqlite`] is a *reservation*; no such adapter
//! ships in this checkout, and a reserved kind may not claim [`ProofCeiling::Durable`] or
//! [`ProofCeiling::Physical`] without an adapter that actually implements it.
//!
//! The central rule is asymmetric on purpose: an adapter that declares a capability `false` must
//! *refuse* when the capability is exercised, and an adapter that declares it `true` must honour
//! it. Both directions fail closed, so a stub cannot buy conformance by returning `Ok`.

use crate::{EventStorePort, PortError};
use kiana_domain::{
    json_digest, AggregateVersion, CommitOutcome, EventStoreCapabilities, RequestId, RuntimeEvent,
    TransitionBatch,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

/// Suite version. Bump only when a new check is added; every adapter is then re-run against it.
pub const ADAPTER_CONFORMANCE_SCHEMA: &str = "kiana.pd31-adapter-conformance.v1";
pub const ADAPTER_DECLARATION_SCHEMA: &str = "kiana.pd31-adapter-declaration.v1";
pub const ADAPTER_CONFORMANCE_REPORT_SCHEMA: &str = "kiana.pd31-adapter-conformance-report.v1";
pub const ADAPTER_CAPABILITY_MATRIX_SCHEMA: &str = "kiana.pd31-adapter-capability-matrix.v1";

/// Upper bound on conformance rows in one report. A report is an evidence record, not a log.
pub const MAX_CONFORMANCE_ROWS: usize = 64;
/// Upper bound on adapters in one capability matrix.
pub const MAX_MATRIX_ADAPTERS: usize = 8;
/// Upper bound on the strings an adapter may use to name an unsupported method.
pub const MAX_UNSUPPORTED_CODES: usize = 32;

fn required(value: &str, field: &'static str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\r', '\n']) {
        Err(field.to_owned())
    } else {
        Ok(())
    }
}

fn bounded_codes(values: &[String], field: &'static str) -> Result<(), String> {
    if values.len() > MAX_UNSUPPORTED_CODES {
        return Err(field.to_owned());
    }
    for value in values {
        required(value, field, 256)?;
    }
    Ok(())
}

/// The store families this suite names. `DurableFile` and `Sqlite` are reserved rows for adapters
/// that do not exist in this checkout; a reserved row may not be presented as implemented.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind {
    Memory,
    Jsonl,
    DurableFile,
    Sqlite,
}

impl AdapterKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Jsonl => "jsonl",
            Self::DurableFile => "durable_file",
            Self::Sqlite => "sqlite",
        }
    }

    /// A kind with no adapter in this checkout. A reserved kind is a roadmap slot, not evidence.
    pub fn is_reserved(&self) -> bool {
        matches!(self, Self::DurableFile | Self::Sqlite)
    }
}

/// The strongest claim an adapter row may carry. The ladder is ordered and a row may never skip a
/// rung: an adapter proves `Durable` by also satisfying `LocalBehavior`.
///
/// `Source` means the row is a declaration plus in-process outcomes. `LocalBehavior` means the
/// adapter's own process produced the outcome it declared. `Durable` and `Physical` are claims a
/// suite that only observes declarations and logical results cannot establish on its own.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofCeiling {
    Source,
    LocalBehavior,
    Durable,
    Physical,
}

impl ProofCeiling {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::LocalBehavior => "local_behavior",
            Self::Durable => "durable",
            Self::Physical => "physical",
        }
    }

    /// This suite observes declarations and logical outcomes only, so it cannot certify the top of
    /// the ladder. A row claiming more than `LocalBehavior` is refused by `validate`.
    pub fn certifiable_by_source_suite(&self) -> bool {
        *self <= Self::LocalBehavior
    }
}

/// The checks every adapter runs. A row for each must be present in a passing report, so a suite
/// that quietly stops checking ordering is visible as a missing check rather than a green run.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConformanceCheck {
    /// The declared capabilities match the capabilities the adapter actually reports.
    DeclarationMatchesAdapter,
    /// `supports_atomic_transitions()` agrees with `capabilities().atomic_transitions`.
    AtomicFlagAgreesWithCapabilities,
    /// The declared `durable_commits` matches the adapter's own capability record.
    DurableFlagAgreesWithCapabilities,
    /// A memory adapter never declares `durable_commits`.
    MemoryIsNotDurable,
    /// A reserved kind never claims a ceiling above `Source`.
    ReservedKindIsNotImplemented,
    /// `commit_transition` returns `Committed` for a fresh batch.
    FreshBatchCommits,
    /// The same command and digest replay instead of writing twice.
    SameCommandReplays,
    /// A stale expected version is a `Conflict` that changes nothing.
    StaleVersionConflicts,
    /// A conflicting command is not confirmed by `read_command`.
    ConflictedCommandHasNoReceipt,
    /// Cursor paging returns committed events in commit order with a non-wrapping cursor.
    CursorPagePreservesCommitOrder,
    /// `read_stream` agrees with the committed order of the same events.
    StreamReadAgreesWithCommitOrder,
    /// An unsupported declared capability refuses with a stable code instead of succeeding.
    UnsupportedCapabilityRefuses,
    /// The refusal code is one the adapter declared, so the two adapters cannot drift.
    RefusalCodeIsDeclared,
    /// An adapter whose `read_all` is unsupported reports it rather than returning empty.
    UnsupportedReadAllIsNotEmptySuccess,
    /// The declared proof ceiling is one this suite can certify.
    ProofCeilingIsCertifiable,
}

impl ConformanceCheck {
    /// Stable machine-readable id. These strings are asserted by the source guard and must not be
    /// renamed without bumping [`ADAPTER_CONFORMANCE_SCHEMA`].
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DeclarationMatchesAdapter => "declaration_matches_adapter",
            Self::AtomicFlagAgreesWithCapabilities => "atomic_flag_agrees_with_capabilities",
            Self::DurableFlagAgreesWithCapabilities => "durable_flag_agrees_with_capabilities",
            Self::MemoryIsNotDurable => "memory_is_not_durable",
            Self::ReservedKindIsNotImplemented => "reserved_kind_is_not_implemented",
            Self::FreshBatchCommits => "fresh_batch_commits",
            Self::SameCommandReplays => "same_command_replays",
            Self::StaleVersionConflicts => "stale_version_conflicts",
            Self::ConflictedCommandHasNoReceipt => "conflicted_command_has_no_receipt",
            Self::CursorPagePreservesCommitOrder => "cursor_page_preserves_commit_order",
            Self::StreamReadAgreesWithCommitOrder => "stream_read_agrees_with_commit_order",
            Self::UnsupportedCapabilityRefuses => "unsupported_capability_refuses",
            Self::RefusalCodeIsDeclared => "refusal_code_is_declared",
            Self::UnsupportedReadAllIsNotEmptySuccess => {
                "unsupported_read_all_is_not_empty_success"
            }
            Self::ProofCeilingIsCertifiable => "proof_ceiling_is_certifiable",
        }
    }

    /// The full ordered check list. Order is the suite's decision order and is part of the
    /// contract: a report's rows are sorted by it, so two adapters cannot reorder the evidence.
    pub fn all() -> Vec<Self> {
        vec![
            Self::DeclarationMatchesAdapter,
            Self::AtomicFlagAgreesWithCapabilities,
            Self::DurableFlagAgreesWithCapabilities,
            Self::MemoryIsNotDurable,
            Self::ReservedKindIsNotImplemented,
            Self::FreshBatchCommits,
            Self::SameCommandReplays,
            Self::StaleVersionConflicts,
            Self::ConflictedCommandHasNoReceipt,
            Self::CursorPagePreservesCommitOrder,
            Self::StreamReadAgreesWithCommitOrder,
            Self::UnsupportedCapabilityRefuses,
            Self::RefusalCodeIsDeclared,
            Self::UnsupportedReadAllIsNotEmptySuccess,
            Self::ProofCeilingIsCertifiable,
        ]
    }
}

/// What the suite observed for one check.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConformanceResult {
    Passed,
    /// The check did not hold. `detail` carries the observed fact, never an adapter payload.
    Refused(String),
}

/// One row of the suite result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceRow {
    pub check: ConformanceCheck,
    pub result: ConformanceResult,
    /// The stable code the adapter returned, when the row concerns a refusal. Empty otherwise.
    pub observed_code: String,
}

impl ConformanceRow {
    pub fn passed(check: ConformanceCheck) -> Self {
        Self {
            check,
            result: ConformanceResult::Passed,
            observed_code: String::new(),
        }
    }

    pub fn refused(check: ConformanceCheck, detail: impl Into<String>) -> Self {
        Self {
            check,
            result: ConformanceResult::Refused(detail.into()),
            observed_code: String::new(),
        }
    }

    pub fn refused_code(check: ConformanceCheck, code: impl Into<String>) -> Self {
        let code = code.into();
        Self {
            check,
            result: ConformanceResult::Refused(code.clone()),
            observed_code: code,
        }
    }

    pub fn is_passed(&self) -> bool {
        self.result == ConformanceResult::Passed
    }
}

/// What an adapter publishes about itself before the suite runs.
///
/// The declaration is the thing under test. A suite that only inspected adapter behaviour could
/// not tell a stub from a store, so the declaration is compared against the adapter's own reported
/// capabilities and both must agree.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterConformance {
    pub schema: String,
    pub adapter: AdapterKind,
    /// The capabilities the adapter claims. Must equal `EventStorePort::capabilities()`.
    pub capabilities: EventStoreCapabilities,
    /// The strongest claim this row carries.
    pub proof_ceiling: ProofCeiling,
    /// Stable codes this adapter returns when a capability it does not declare is exercised. The
    /// suite refuses a refusal whose code is not listed here, so two adapters cannot drift apart
    /// into ad-hoc strings.
    pub unsupported_codes: Vec<String>,
    /// A capability the suite deliberately probes for refusal. It must be `false` in
    /// `capabilities`; a declared-true capability is not probed.
    pub probe_capability: UnsupportedProbe,
    /// Free-text limitations. Never a claim of durability; the baseline doc carries the full text.
    pub limitations: Vec<String>,
    pub declaration_digest: String,
}

/// The single capability the suite probes for refusal. It is a projection, not a process
/// operation: a Memory or JSONL event store has no projection writer, and the default
/// [`ProjectionStorePort`] refuses. A future adapter that gains one declares it and names a
/// different probe.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedProbe {
    /// Call `EventStorePort::close()` and require a refusal on a non-closeable adapter.
    EventStoreClose,
    /// Call `ProjectionStorePort::apply_events` through a port the adapter does not implement and
    /// require the stable `projection_store_unsupported` refusal.
    ProjectionApply,
    /// Call `EventStorePort::append_expected` with `Some(version)` and require the stable
    /// `event_store_expected_version_unsupported` refusal from a non-CAS adapter.
    ExpectedVersion,
    /// Call `EventStorePort::append_idempotent` and require the stable
    /// `event_store_idempotency_unsupported` refusal from a non-idempotent adapter.
    IdempotentAppend,
}

impl UnsupportedProbe {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EventStoreClose => "event_store_close",
            Self::ProjectionApply => "projection_apply",
            Self::ExpectedVersion => "expected_version",
            Self::IdempotentAppend => "idempotent_append",
        }
    }
}

impl AdapterConformance {
    pub fn new(
        adapter: AdapterKind,
        capabilities: EventStoreCapabilities,
        proof_ceiling: ProofCeiling,
        probe_capability: UnsupportedProbe,
        unsupported_codes: Vec<String>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut declaration = Self {
            schema: ADAPTER_DECLARATION_SCHEMA.to_owned(),
            adapter,
            capabilities,
            proof_ceiling,
            unsupported_codes,
            probe_capability,
            limitations,
            declaration_digest: String::new(),
        };
        declaration.declaration_digest = declaration.digest();
        declaration.validate()?;
        Ok(declaration)
    }

    /// The refusal code the suite expects for the declared probe when the probe is unsupported.
    pub fn expected_probe_code(&self) -> &'static str {
        match self.probe_capability {
            UnsupportedProbe::EventStoreClose => "event_store_close_unsupported",
            UnsupportedProbe::ProjectionApply => "projection_store_unsupported",
            UnsupportedProbe::ExpectedVersion => "event_store_expected_version_unsupported",
            UnsupportedProbe::IdempotentAppend => "event_store_idempotency_unsupported",
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ADAPTER_DECLARATION_SCHEMA {
            return Err("adapter_declaration_schema_invalid".to_owned());
        }
        required(
            self.adapter.as_str(),
            "adapter_declaration_kind_invalid",
            32,
        )?;
        required(
            self.proof_ceiling.as_str(),
            "adapter_declaration_proof_ceiling_invalid",
            32,
        )?;
        required(
            self.probe_capability.as_str(),
            "adapter_declaration_probe_invalid",
            32,
        )?;
        bounded_codes(
            &self.unsupported_codes,
            "adapter_declaration_unsupported_codes_invalid",
        )?;
        if self.unsupported_codes.is_empty() {
            return Err("adapter_declaration_unsupported_codes_empty".to_owned());
        }
        if !self
            .unsupported_codes
            .iter()
            .any(|code| code == self.expected_probe_code())
        {
            return Err("adapter_declaration_probe_code_not_declared".to_owned());
        }
        if self
            .unsupported_codes
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != self.unsupported_codes.len()
        {
            return Err("adapter_declaration_unsupported_code_duplicate".to_owned());
        }
        if self.limitations.len() > MAX_UNSUPPORTED_CODES {
            return Err("adapter_declaration_limitations_invalid".to_owned());
        }
        for limitation in &self.limitations {
            required(limitation, "adapter_declaration_limitations_invalid", 512)?;
        }
        if self.adapter == AdapterKind::Memory && self.capabilities.durable_commits {
            return Err("adapter_declaration_memory_cannot_be_durable".to_owned());
        }
        if self.adapter.is_reserved() && self.proof_ceiling != ProofCeiling::Source {
            return Err("adapter_declaration_proof_ceiling_not_certifiable".to_owned());
        }
        if !self.proof_ceiling.certifiable_by_source_suite() {
            return Err("adapter_declaration_proof_ceiling_not_certifiable".to_owned());
        }
        if self.adapter.is_reserved() && self.proof_ceiling != ProofCeiling::Source {
            return Err("adapter_declaration_reserved_kind_not_implemented".to_owned());
        }
        if !self.probe_is_declared_unsupported() {
            return Err("adapter_declaration_probe_capability_supported".to_owned());
        }
        validate_digest(&self.declaration_digest, "adapter_declaration_digest")?;
        if self.declaration_digest != self.digest() {
            return Err("adapter_declaration_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// The suite only probes a capability the declaration says is absent, so a `true` capability
    /// can never be used to manufacture an expected refusal.
    fn probe_is_declared_unsupported(&self) -> bool {
        match self.probe_capability {
            // `close` has no capability bit; it is probed only when the adapter does not declare
            // a durable writer, since a close acknowledgement implies a flush boundary.
            UnsupportedProbe::EventStoreClose => !self.capabilities.durable_commits,
            UnsupportedProbe::ProjectionApply => true,
            UnsupportedProbe::ExpectedVersion => true,
            UnsupportedProbe::IdempotentAppend => true,
        }
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "adapter": self.adapter,
            "capabilities": self.capabilities,
            "proof_ceiling": self.proof_ceiling,
            "probe_capability": self.probe_capability,
            "unsupported_codes": self.unsupported_codes,
            "limitations": self.limitations,
        }))
    }
}

/// The suite result for one adapter.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterConformanceReport {
    pub schema: String,
    pub declaration_digest: String,
    pub adapter: AdapterKind,
    pub proof_ceiling: ProofCeiling,
    pub rows: Vec<ConformanceRow>,
    pub report_digest: String,
}

impl AdapterConformanceReport {
    pub fn new(declaration: &AdapterConformance, rows: Vec<ConformanceRow>) -> Self {
        let mut report = Self {
            schema: ADAPTER_CONFORMANCE_REPORT_SCHEMA.to_owned(),
            declaration_digest: declaration.declaration_digest.clone(),
            adapter: declaration.adapter,
            proof_ceiling: declaration.proof_ceiling,
            rows,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report
    }

    pub fn passed(&self) -> bool {
        self.rows.iter().all(ConformanceRow::is_passed)
    }

    /// The first refusal in suite order, or `None` when the adapter passed every check.
    pub fn first_refusal(&self) -> Option<&ConformanceRow> {
        self.rows.iter().find(|row| !row.is_passed())
    }

    /// The refusal codes the adapter returned, in suite order. Two adapters that pass produce
    /// identical code sequences for the same probe, which is the anti-drift property.
    pub fn refusal_codes(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|row| !row.observed_code.is_empty())
            .map(|row| row.observed_code.clone())
            .collect()
    }

    /// Structural validation of the report itself. This is separate from `passed()`: a report can
    /// be well formed and still record a refusal.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ADAPTER_CONFORMANCE_REPORT_SCHEMA
            || self.rows.is_empty()
            || self.rows.len() > MAX_CONFORMANCE_ROWS
        {
            return Err("adapter_conformance_report_header_invalid".to_owned());
        }
        required(
            self.adapter.as_str(),
            "adapter_conformance_report_kind_invalid",
            32,
        )?;
        required(
            self.proof_ceiling.as_str(),
            "adapter_conformance_report_proof_ceiling_invalid",
            32,
        )?;
        validate_digest(
            &self.declaration_digest,
            "adapter_conformance_report_declaration_digest",
        )?;
        validate_digest(&self.report_digest, "adapter_conformance_report_digest")?;
        if !self.proof_ceiling.certifiable_by_source_suite() {
            return Err("adapter_conformance_report_proof_ceiling_not_certifiable".to_owned());
        }
        let expected = ConformanceCheck::all();
        if self.rows.len() != expected.len() {
            return Err("adapter_conformance_report_check_set_incomplete".to_owned());
        }
        for (row, check) in self.rows.iter().zip(expected.iter()) {
            if row.check != *check {
                return Err("adapter_conformance_report_check_order_invalid".to_owned());
            }
            if let ConformanceResult::Refused(detail) = &row.result {
                required(detail, "adapter_conformance_report_detail_invalid", 512)?;
            }
            if !row.observed_code.is_empty() {
                required(
                    &row.observed_code,
                    "adapter_conformance_report_code_invalid",
                    256,
                )?;
            }
        }
        if self.report_digest != self.digest() {
            return Err("adapter_conformance_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "declaration_digest": self.declaration_digest,
            "adapter": self.adapter,
            "proof_ceiling": self.proof_ceiling,
            "rows": self.rows,
        }))
    }
}

/// The capability matrix: one row per adapter, so the durable/local_behavior/physical boundary is
/// stated in one place instead of being re-derived per adapter in prose.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterCapabilityMatrix {
    pub schema: String,
    pub declarations: Vec<AdapterConformance>,
    pub matrix_digest: String,
}

impl AdapterCapabilityMatrix {
    pub fn new(declarations: Vec<AdapterConformance>) -> Result<Self, String> {
        let mut matrix = Self {
            schema: ADAPTER_CAPABILITY_MATRIX_SCHEMA.to_owned(),
            declarations,
            matrix_digest: String::new(),
        };
        matrix.matrix_digest = matrix.digest();
        matrix.validate()?;
        Ok(matrix)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ADAPTER_CAPABILITY_MATRIX_SCHEMA
            || self.declarations.is_empty()
            || self.declarations.len() > MAX_MATRIX_ADAPTERS
        {
            return Err("adapter_capability_matrix_header_invalid".to_owned());
        }
        let mut seen = std::collections::BTreeSet::new();
        for declaration in &self.declarations {
            declaration.validate()?;
            if !seen.insert(declaration.adapter) {
                return Err("adapter_capability_matrix_adapter_duplicate".to_owned());
            }
        }
        // The memory row must exist and must not be the one that claims durability.
        let memory = self
            .declarations
            .iter()
            .find(|declaration| declaration.adapter == AdapterKind::Memory)
            .ok_or_else(|| "adapter_capability_matrix_memory_row_missing".to_owned())?;
        if memory.capabilities.durable_commits {
            return Err("adapter_capability_matrix_memory_marked_durable".to_owned());
        }
        validate_digest(&self.matrix_digest, "adapter_capability_matrix_digest")?;
        if self.matrix_digest != self.digest() {
            return Err("adapter_capability_matrix_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// True when no row claims a ceiling the source suite cannot certify.
    pub fn boundaries_are_honest(&self) -> bool {
        self.declarations
            .iter()
            .all(|declaration| declaration.proof_ceiling.certifiable_by_source_suite())
    }

    pub fn durable_adapters(&self) -> Vec<AdapterKind> {
        self.declarations
            .iter()
            .filter(|declaration| declaration.capabilities.durable_commits)
            .map(|declaration| declaration.adapter)
            .collect()
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "declarations": self.declarations,
        }))
    }
}

fn validate_digest(value: &str, field: &'static str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field.to_owned());
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field.to_owned())
    }
}

/// Build a batch whose command digest is stable per `(command_id, digest_char, aggregate)`.
pub fn conformance_batch(
    command_id: RequestId,
    aggregate_id: &str,
    digest_char: char,
    idempotency_key: &str,
) -> TransitionBatch {
    let event = RuntimeEvent::new(
        command_id,
        1,
        "run.started",
        json!({ "run_id": aggregate_id }),
    )
    .expect("conformance event is well formed")
    .with_stream_metadata("run", aggregate_id, 1)
    .with_idempotency_key(idempotency_key);
    TransitionBatch {
        command_id,
        command_digest: std::iter::repeat_n(digest_char, 64).collect(),
        expected_versions: vec![AggregateVersion::new("run", aggregate_id, 0)],
        events: vec![event],
    }
}

/// The outcome an adapter produced for a probe, reduced to the stable code the suite compares.
fn port_code(error: &PortError) -> String {
    match error {
        PortError::Unavailable(code) | PortError::Conflict(code) | PortError::Failed(code) => {
            code.clone()
        }
    }
}

/// Run the one shared suite against an adapter and its declaration.
///
/// The suite is deliberately ordered: declaration honesty first, so a lying declaration is
/// reported as such rather than being discovered halfway through the behavioural checks. Behaviour
/// checks run in the order a caller would hit them — commit, replay, conflict, receipt, cursor,
/// stream — and the refusal probes run last, because a probe on a lying declaration is meaningless.
///
/// The `probe` argument is the port the suite calls to exercise the declared unsupported
/// capability. It is passed in rather than constructed here so this module keeps no dependency on
/// a concrete adapter or port implementation, and so a future SQLite adapter can supply its own.
pub async fn run_event_store_conformance<F, Fut>(
    store: &dyn EventStorePort,
    declaration: &AdapterConformance,
    probe: &F,
) -> AdapterConformanceReport
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<String, PortError>>,
{
    let mut rows: BTreeMap<ConformanceCheck, ConformanceRow> = BTreeMap::new();
    let observed = store.capabilities();
    let mut push = |check: ConformanceCheck, row: ConformanceRow| {
        rows.insert(check, row);
    };

    // 1. The declaration must match what the adapter reports, in both directions.
    push(
        ConformanceCheck::DeclarationMatchesAdapter,
        if observed == declaration.capabilities {
            ConformanceRow::passed(ConformanceCheck::DeclarationMatchesAdapter)
        } else {
            ConformanceRow::refused_code(
                ConformanceCheck::DeclarationMatchesAdapter,
                "adapter_declaration_capabilities_mismatch",
            )
        },
    );
    push(
        ConformanceCheck::AtomicFlagAgreesWithCapabilities,
        if observed.atomic_transitions == store.supports_atomic_transitions() {
            ConformanceRow::passed(ConformanceCheck::AtomicFlagAgreesWithCapabilities)
        } else {
            ConformanceRow::refused(
                ConformanceCheck::AtomicFlagAgreesWithCapabilities,
                "adapter_atomic_flag_disagrees_with_capabilities",
            )
        },
    );
    push(
        ConformanceCheck::DurableFlagAgreesWithCapabilities,
        if observed.durable_commits == declaration.capabilities.durable_commits {
            ConformanceRow::passed(ConformanceCheck::DurableFlagAgreesWithCapabilities)
        } else {
            ConformanceRow::refused(
                ConformanceCheck::DurableFlagAgreesWithCapabilities,
                "adapter_durable_flag_disagrees_with_capabilities",
            )
        },
    );
    push(
        ConformanceCheck::MemoryIsNotDurable,
        if declaration.adapter != AdapterKind::Memory || !declaration.capabilities.durable_commits {
            ConformanceRow::passed(ConformanceCheck::MemoryIsNotDurable)
        } else {
            ConformanceRow::refused_code(
                ConformanceCheck::MemoryIsNotDurable,
                "adapter_declaration_memory_cannot_be_durable",
            )
        },
    );
    push(
        ConformanceCheck::ReservedKindIsNotImplemented,
        if !declaration.adapter.is_reserved() || declaration.proof_ceiling == ProofCeiling::Source {
            ConformanceRow::passed(ConformanceCheck::ReservedKindIsNotImplemented)
        } else {
            ConformanceRow::refused_code(
                ConformanceCheck::ReservedKindIsNotImplemented,
                "adapter_declaration_reserved_kind_not_implemented",
            )
        },
    );

    // 2. Behaviour. These are only meaningful when the adapter declared atomic transitions, so a
    //    non-CAS adapter reports the refusal rather than a fabricated pass.
    let aggregate = format!("pd31-{}", declaration.adapter.as_str());
    let command_id = RequestId::new();
    let fresh = conformance_batch(command_id, &aggregate, 'a', &format!("pd31:{}", aggregate));
    let commit = store.commit_transition(fresh.clone()).await;
    push(
        ConformanceCheck::FreshBatchCommits,
        match &commit {
            Ok(CommitOutcome::Committed { receipt }) => {
                if receipt.validate_against(&fresh).is_ok() {
                    ConformanceRow::passed(ConformanceCheck::FreshBatchCommits)
                } else {
                    ConformanceRow::refused_code(
                        ConformanceCheck::FreshBatchCommits,
                        "adapter_command_receipt_invalid",
                    )
                }
            }
            Ok(other) => ConformanceRow::refused_code(
                ConformanceCheck::FreshBatchCommits,
                format!("adapter_fresh_commit_returned_{}", outcome_name(other)),
            ),
            Err(error) => ConformanceRow::refused_code(
                ConformanceCheck::FreshBatchCommits,
                format!("adapter_fresh_commit_error_{}", port_code(error)),
            ),
        },
    );

    let replay = store.commit_transition(fresh.clone()).await;
    push(
        ConformanceCheck::SameCommandReplays,
        match replay {
            Ok(CommitOutcome::Replayed { original }) => {
                if original.command_id == fresh.command_id
                    && original.command_digest == fresh.command_digest
                {
                    ConformanceRow::passed(ConformanceCheck::SameCommandReplays)
                } else {
                    ConformanceRow::refused_code(
                        ConformanceCheck::SameCommandReplays,
                        "adapter_replay_receipt_identity_mismatch",
                    )
                }
            }
            Ok(other) => ConformanceRow::refused_code(
                ConformanceCheck::SameCommandReplays,
                format!("adapter_replay_returned_{}", outcome_name(&other)),
            ),
            Err(error) => ConformanceRow::refused_code(
                ConformanceCheck::SameCommandReplays,
                format!("adapter_replay_error_{}", port_code(&error)),
            ),
        },
    );

    let stale_command = RequestId::new();
    let stale = store
        .commit_transition(conformance_batch(
            stale_command,
            &aggregate,
            'b',
            &format!("pd31:{aggregate}:stale"),
        ))
        .await;
    push(
        ConformanceCheck::StaleVersionConflicts,
        match &stale {
            Ok(CommitOutcome::Conflict { .. }) => {
                ConformanceRow::passed(ConformanceCheck::StaleVersionConflicts)
            }
            Ok(other) => ConformanceRow::refused_code(
                ConformanceCheck::StaleVersionConflicts,
                format!("adapter_stale_commit_returned_{}", outcome_name(other)),
            ),
            Err(error) => ConformanceRow::refused_code(
                ConformanceCheck::StaleVersionConflicts,
                format!("adapter_stale_commit_error_{}", port_code(error)),
            ),
        },
    );

    let stale_receipt = store.read_command(&stale_command).await;
    push(
        ConformanceCheck::ConflictedCommandHasNoReceipt,
        match stale_receipt {
            Ok(None) => ConformanceRow::passed(ConformanceCheck::ConflictedCommandHasNoReceipt),
            Ok(Some(_)) => ConformanceRow::refused_code(
                ConformanceCheck::ConflictedCommandHasNoReceipt,
                "adapter_conflicted_command_has_receipt",
            ),
            Err(error) => ConformanceRow::refused_code(
                ConformanceCheck::ConflictedCommandHasNoReceipt,
                format!("adapter_read_command_error_{}", port_code(&error)),
            ),
        },
    );

    // 3. Ordering. A page must return what was committed, in commit order, and its cursor must
    //    never wrap backwards. An adapter that sorts by anything other than commit order fails.
    let page = store.read_from(0, 8).await;
    push(
        ConformanceCheck::CursorPagePreservesCommitOrder,
        match &page {
            Ok(page) => {
                let monotonic = page
                    .events
                    .iter()
                    .map(|event| event.sequence)
                    .collect::<Vec<_>>();
                let ordered = monotonic.windows(2).all(|pair| pair[0] < pair[1]);
                let cursor_at_least_events = page.cursor as usize >= page.events.len();
                if !page.events.is_empty() && ordered && cursor_at_least_events {
                    ConformanceRow::passed(ConformanceCheck::CursorPagePreservesCommitOrder)
                } else {
                    ConformanceRow::refused_code(
                        ConformanceCheck::CursorPagePreservesCommitOrder,
                        "adapter_cursor_page_order_invalid",
                    )
                }
            }
            Err(error) => ConformanceRow::refused_code(
                ConformanceCheck::CursorPagePreservesCommitOrder,
                format!("adapter_read_from_error_{}", port_code(error)),
            ),
        },
    );

    let stream = store.read_stream("run", &aggregate).await;
    push(
        ConformanceCheck::StreamReadAgreesWithCommitOrder,
        match &stream {
            Ok(events) => {
                let sequences = events
                    .iter()
                    .map(|event| event.sequence)
                    .collect::<Vec<_>>();
                let ordered = sequences.windows(2).all(|pair| pair[0] < pair[1]);
                if ordered && events.len() == 1 {
                    ConformanceRow::passed(ConformanceCheck::StreamReadAgreesWithCommitOrder)
                } else {
                    ConformanceRow::refused_code(
                        ConformanceCheck::StreamReadAgreesWithCommitOrder,
                        "adapter_stream_order_invalid",
                    )
                }
            }
            Err(error) => ConformanceRow::refused_code(
                ConformanceCheck::StreamReadAgreesWithCommitOrder,
                format!("adapter_read_stream_error_{}", port_code(error)),
            ),
        },
    );

    // 4. Refusal. An adapter that declares a capability absent must refuse the call with a code it
    //    declared. Returning `Ok`, or returning a code the declaration did not list, is a refusal.
    let probed = probe().await;
    let (refuses, observed_code) = match probed {
        Err(error) => (true, port_code(&error)),
        Ok(code) => (false, code),
    };
    push(
        ConformanceCheck::UnsupportedCapabilityRefuses,
        if refuses {
            ConformanceRow::passed(ConformanceCheck::UnsupportedCapabilityRefuses)
        } else {
            ConformanceRow::refused_code(
                ConformanceCheck::UnsupportedCapabilityRefuses,
                "adapter_unsupported_capability_succeeded",
            )
        },
    );
    push(
        ConformanceCheck::RefusalCodeIsDeclared,
        if !refuses {
            ConformanceRow::refused_code(
                ConformanceCheck::RefusalCodeIsDeclared,
                "adapter_unsupported_capability_succeeded",
            )
        } else if declaration
            .unsupported_codes
            .iter()
            .any(|code| code == &observed_code)
        {
            ConformanceRow::passed(ConformanceCheck::RefusalCodeIsDeclared)
        } else {
            ConformanceRow::refused_code(
                ConformanceCheck::RefusalCodeIsDeclared,
                format!("adapter_refusal_code_undeclared_{observed_code}"),
            )
        },
    );

    // 5. An unsupported read is reported, never an empty success. An empty vector is the shape a
    //    caller would mistake for "no events", which is how a failed read becomes a clean receipt.
    let read_all = store.read_all().await;
    push(
        ConformanceCheck::UnsupportedReadAllIsNotEmptySuccess,
        match &read_all {
            Ok(events) if events.is_empty() => ConformanceRow::refused_code(
                ConformanceCheck::UnsupportedReadAllIsNotEmptySuccess,
                "adapter_read_all_empty_success",
            ),
            Ok(_) => ConformanceRow::passed(ConformanceCheck::UnsupportedReadAllIsNotEmptySuccess),
            Err(error) => {
                if port_code(error) == "event_store_read_all_unsupported" {
                    ConformanceRow::passed(ConformanceCheck::UnsupportedReadAllIsNotEmptySuccess)
                } else {
                    ConformanceRow::refused_code(
                        ConformanceCheck::UnsupportedReadAllIsNotEmptySuccess,
                        format!("adapter_read_all_error_{}", port_code(error)),
                    )
                }
            }
        },
    );

    push(
        ConformanceCheck::ProofCeilingIsCertifiable,
        if declaration.proof_ceiling.certifiable_by_source_suite() {
            ConformanceRow::passed(ConformanceCheck::ProofCeilingIsCertifiable)
        } else {
            ConformanceRow::refused_code(
                ConformanceCheck::ProofCeilingIsCertifiable,
                "adapter_declaration_proof_ceiling_not_certifiable",
            )
        },
    );

    // The report carries one row per check in suite order. Missing rows are impossible here, which
    // is what lets a later reader trust that a green report covered the whole suite.
    let ordered = ConformanceCheck::all()
        .into_iter()
        .map(|check| {
            rows.remove(&check)
                .unwrap_or_else(|| ConformanceRow::refused_code(check, "adapter_check_not_run"))
        })
        .collect();
    AdapterConformanceReport::new(declaration, ordered)
}

fn outcome_name(outcome: &CommitOutcome) -> &'static str {
    match outcome {
        CommitOutcome::Committed { .. } => "committed",
        CommitOutcome::Replayed { .. } => "replayed",
        CommitOutcome::Conflict { .. } => "conflict",
        CommitOutcome::Unknown { .. } => "unknown",
    }
}

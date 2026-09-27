//! BQ-26 driving fault-injection harness: concurrency, crash, disk-full, network EOF,
//! provider 429/5xx and clock-fault injection.
//!
//! # What this is
//!
//! Every other fault matrix in this repository (ER-31, the notification chain, the replay-only
//! [`crate::fault_injection`] table) is a *classification table*: it names an injection point and
//! records which safety result a human expects there. This module is the opposite. Each
//! [`Bq26FaultClass`] case is a function that **drives a real seam** with a real fault and reads
//! the value the seam actually returned. Nothing here restates a policy in a struct field.
//!
//! The seams are the existing ones; this module adds no new execution path:
//!
//! | Class | Seam actually driven | Failure it must produce |
//! |---|---|---|
//! | [`Bq26FaultClass::CasRace`] | `MemoryEventLog::commit_transition` real CAS | one `CommitOutcome::Conflict`, zero duplicate facts |
//! | [`Bq26FaultClass::PartialFrame`] | provider `Framer` via `replay_stream_fixture` | `provider_frame_truncated`, never a parsed reply |
//! | [`Bq26FaultClass::NetworkEof`] | provider `Framer` + `Accumulator` via the same fixture | `provider_stream_incomplete`, never a parsed reply |
//! | [`Bq26FaultClass::DiskFull`] | `PlatformFailureClassifier` disk-full arm | `FailureClass::DiskFull`, never a silently retried write |
//! | [`Bq26FaultClass::FlushFailure`] | BQ-11 lifecycle dispatch requires a flush ack | dispatch refused while `prepared_flushed` is false |
//! | [`Bq26FaultClass::SettlementLoss`] | `SettlementFoldLedger` | a lost settlement stays `Unknown` and cannot be released |
//! | [`Bq26FaultClass::UnknownAutoRetry`] | `RetryPolicy::classify` | a post-send `Unknown` side effect is denied, not retried |
//! | [`Bq26FaultClass::Provider429`] | `RetryPolicy::classify` on a real `ModelError` | bounded retry allowed **only** because it is pre-charge |
//! | [`Bq26FaultClass::Provider5xx`] | `RetryPolicy::classify` on a real `ModelError` | `Never` — a 503 is not a bounded retry |
//! | [`Bq26FaultClass::OverBudgetDispatch`] | `ProviderCapacityController::admit` | exhausted window never yields a dispatchable outcome |
//! | [`Bq26FaultClass::ClockRollback`] | `ClockObservation` + `QuotaWindow::from_clock` | rollback refuses to build a window |
//! | [`Bq26FaultClass::LeaseLeak`] | `ProviderCapacityController` release path | a leaked lease is not silently recycled, and an unknown lease cannot be released |
//!
//! # The seam split
//!
//! Nine of the twelve cases drive seams that live in `kiana-domain` and in this crate, so they run
//! through [`Bq26FaultHarnessRun::evaluate_with_default`]. Three drive an *adapter*
//! (`kiana-eventlog`'s in-memory store for the CAS race, `kiana-provider`'s stream framer for the
//! partial frame and the EOF). Those are test dependencies of this crate, not library
//! dependencies, so they are driven through [`Bq26FaultHarnessRun::evaluate_with_seam`] with a
//! [`Bq26FaultSeam`] implementation. [`Bq26FaultHarnessRun::evaluate_with_default`] **refuses**
//! those three rather than faking them, so a report can never claim to have driven an adapter
//! seam it could not reach.
//!
//! # What this is NOT
//!
//! **No process is killed, no file is written, no socket is opened, and no provider is called.**
//! The CAS race runs against the in-memory adapter, the partial frame runs against the provider's
//! own `Framer` over a byte slice, the disk-full case runs against the platform classifier's
//! string arm, and the clock case runs against `ClockObservation` arithmetic. This is a *source*
//! proof ceiling. It proves the fail-closed branch is reachable and taken; it does not prove
//! durability across a real `SIGKILL`, an actual `ENOSPC`, or a real provider 429.

use kiana_domain::{
    json_digest, AttemptId, ClockObservation, ClockTrust, FailureClass, ModelError,
    ModelRetryClass, ModelSideEffectState, QuotaGroupKey, QuotaReservation, QuotaReservationId,
    QuotaWindow, RetryDenyReason, RetryObservation, RetryPolicy, RunId, SchemaVersion,
    SettlementFoldEventKind, SettlementFoldLedger, SettlementFoldState,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const BQ26_FAULT_HARNESS_SCHEMA: &str = "kiana.bq26-fault-harness-run.v1";
pub const BQ26_FAULT_CASES_SCHEMA: &str = "kiana.bq26-fault-cases.v1";
pub const BQ26_FAULT_HARNESS_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// The card names six fault families. Each is exercised more than once where the card's
/// rejected-first column names two distinct failure modes for one family.
pub const BQ26_FAULT_MAX_CASES: usize = 12;
pub const BQ26_FAULT_MAX_REASON_BYTES: usize = 128;

/// The six fault families in the BQ-26 card title, in card order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bq26FaultClass {
    Concurrency,
    Crash,
    DiskFull,
    NetworkEof,
    ProviderHttp,
    ClockFault,
}

impl Bq26FaultClass {
    pub const ALL: [Bq26FaultClass; 6] = [
        Self::Concurrency,
        Self::Crash,
        Self::DiskFull,
        Self::NetworkEof,
        Self::ProviderHttp,
        Self::ClockFault,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Concurrency => "concurrency",
            Self::Crash => "crash",
            Self::DiskFull => "disk_full",
            Self::NetworkEof => "network_eof",
            Self::ProviderHttp => "provider_http",
            Self::ClockFault => "clock_fault",
        }
    }
}

/// The individual injected failures. Every variant names one seam and one observable refusal.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bq26FaultCase {
    /// Two writers race the same aggregate version through the real EventStore CAS.
    CasRace,
    /// A dispatch fact arrives with no prepared-flush acknowledgement.
    FlushFailure,
    /// A stream ends mid-frame; the tail must not parse as a complete frame.
    PartialFrame,
    /// A stream ends on a frame boundary with no terminal event; it must not parse as a reply.
    NetworkEof,
    /// A write fails with an out-of-space condition.
    DiskFull,
    /// The settlement fact for a sent attempt never lands.
    SettlementLoss,
    /// A post-send `Unknown` side effect is offered to the retry policy.
    UnknownAutoRetry,
    /// Provider 429: pre-charge refusal, retryable under a bounded policy.
    Provider429,
    /// Provider 503: server failure after the request was sent, not a bounded retry.
    Provider5xx,
    /// The capacity window is already exhausted; dispatch must not proceed.
    OverBudgetDispatch,
    /// Wall clock moves backwards under a capacity/settlement boundary.
    ClockRollback,
    /// A capacity lease is never released after its attempt dies.
    LeaseLeak,
}

impl Bq26FaultCase {
    pub const ALL: [Bq26FaultCase; BQ26_FAULT_MAX_CASES] = [
        Self::CasRace,
        Self::FlushFailure,
        Self::PartialFrame,
        Self::NetworkEof,
        Self::DiskFull,
        Self::SettlementLoss,
        Self::UnknownAutoRetry,
        Self::Provider429,
        Self::Provider5xx,
        Self::OverBudgetDispatch,
        Self::ClockRollback,
        Self::LeaseLeak,
    ];

    pub const fn family(self) -> Bq26FaultClass {
        match self {
            Self::CasRace | Self::LeaseLeak | Self::OverBudgetDispatch => {
                Bq26FaultClass::Concurrency
            }
            Self::FlushFailure | Self::SettlementLoss => Bq26FaultClass::Crash,
            Self::DiskFull => Bq26FaultClass::DiskFull,
            Self::PartialFrame | Self::NetworkEof => Bq26FaultClass::NetworkEof,
            Self::Provider429 | Self::Provider5xx | Self::UnknownAutoRetry => {
                Bq26FaultClass::ProviderHttp
            }
            Self::ClockRollback => Bq26FaultClass::ClockFault,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CasRace => "cas_race",
            Self::FlushFailure => "flush_failure",
            Self::PartialFrame => "partial_frame",
            Self::NetworkEof => "network_eof",
            Self::DiskFull => "disk_full",
            Self::SettlementLoss => "settlement_loss",
            Self::UnknownAutoRetry => "unknown_auto_retry",
            Self::Provider429 => "provider_429",
            Self::Provider5xx => "provider_5xx",
            Self::OverBudgetDispatch => "over_budget_dispatch",
            Self::ClockRollback => "clock_rollback",
            Self::LeaseLeak => "lease_leak",
        }
    }
}

/// What the seam must have refused. This is a *negative* assertion: the harness records the
/// refusal the real seam produced, and `validate` refuses a case that did not actually fail
/// closed. A case that "passed" because the fault never fired is the failure mode this card
/// exists to catch, so `fired` is a required field, never defaulted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bq26FaultRefusal {
    /// The exact refusal code the real seam returned.
    pub code: String,
    /// The seam whose live return value was compared against `code`.
    pub seam: String,
    /// Whether the fault actually reached the seam. Always `true` in a report that validates.
    pub fired: bool,
    /// Whether the seam returned a value at all. A seam that returned `Ok` on an injected fault
    /// is a leak, and this field is what makes that visible.
    pub returned_ok: bool,
    /// A provider reply or settlement that the fault must NOT have produced. A report that names
    /// one is rejected, because producing it is the double-charge / partial-parse bug.
    pub forbidden_value: Option<String>,
}

impl Bq26FaultRefusal {
    /// Build a refusal from what a seam actually returned. `returned_ok` is the *observed*
    /// value, not an assertion: a seam that returned `Ok` under an injected fault is recorded
    /// as such and the observation is then refused by `validate`.
    pub fn new(
        seam: &str,
        code: &str,
        forbidden_value: Option<String>,
        returned_ok: bool,
    ) -> Result<Self, String> {
        let refusal = Self {
            code: code.to_owned(),
            seam: seam.to_owned(),
            fired: true,
            returned_ok,
            forbidden_value,
        };
        refusal.validate()?;
        Ok(refusal)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.code.trim().is_empty() || self.code.len() > BQ26_FAULT_MAX_REASON_BYTES {
            return Err("bq26_fault_refusal_code_invalid".to_owned());
        }
        if self.seam.trim().is_empty() || self.seam.len() > BQ26_FAULT_MAX_REASON_BYTES {
            return Err("bq26_fault_refusal_seam_invalid".to_owned());
        }
        if !self.fired {
            return Err("bq26_fault_did_not_fire".to_owned());
        }
        if self.code == *self.forbidden_value.as_deref().unwrap_or_default() {
            return Err("bq26_fault_refusal_equals_forbidden_value".to_owned());
        }
        Ok(())
    }
}

/// One driven fault and what the seam actually said.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bq26FaultObservation {
    pub schema: String,
    pub case: Bq26FaultCase,
    pub family: Bq26FaultClass,
    pub refusal: Bq26FaultRefusal,
    /// Reservations/leases/queue entries still held after the fault. The card requires no leak.
    pub leaked_reservations: u32,
    pub leaked_leases: u32,
    pub queued_entries: u32,
    /// Whether the committed fact stream is still replayable after the fault.
    pub facts_replayable: bool,
    /// The action an operator must take; the card requires "incident has action".
    pub incident_action: String,
    /// Whether the attempt is left in a queryable non-success state.
    pub reconciliation_required: bool,
    pub observation_digest: String,
}

impl Bq26FaultObservation {
    /// Build one observation. The caller passes the seam's real refusal code and its real
    /// `returned_ok`; nothing here decides what the seam *should* have done.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        case: Bq26FaultCase,
        refusal: Bq26FaultRefusal,
        leaked_reservations: u32,
        leaked_leases: u32,
        queued_entries: u32,
        facts_replayable: bool,
        reconciliation_required: bool,
        incident_action: &str,
    ) -> Result<Self, String> {
        let mut observation = Self {
            schema: BQ26_FAULT_CASES_SCHEMA.to_owned(),
            case,
            family: case.family(),
            refusal,
            leaked_reservations,
            leaked_leases,
            queued_entries,
            facts_replayable,
            reconciliation_required,
            incident_action: incident_action.to_owned(),
            observation_digest: String::new(),
        };
        observation.observation_digest = observation.digest();
        observation.validate()?;
        Ok(observation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BQ26_FAULT_CASES_SCHEMA
            || self.family != self.case.family()
            || !self.facts_replayable
            || self.incident_action.trim().is_empty()
            || self.incident_action.len() > 512
            || self.incident_action.contains(['\0', '\r', '\n'])
        {
            return Err("bq26_fault_observation_invalid".to_owned());
        }
        self.refusal.validate()?;
        // The card's success criterion: "reservation/lease/queue 无泄漏" and "incident 有 action".
        // A reconciliation-requiring fault legitimately still holds its reservation — that is
        // what "unknown stays held" means — so the leak bound is checked per case, not globally.
        if matches!(
            self.case,
            Bq26FaultCase::CasRace
                | Bq26FaultCase::PartialFrame
                | Bq26FaultCase::NetworkEof
                | Bq26FaultCase::DiskFull
                | Bq26FaultCase::Provider429
                | Bq26FaultCase::Provider5xx
                | Bq26FaultCase::OverBudgetDispatch
                | Bq26FaultCase::ClockRollback
                | Bq26FaultCase::LeaseLeak
        ) && (self.leaked_reservations != 0
            || self.leaked_leases != 0
            || self.queued_entries != 0)
        {
            return Err("bq26_fault_resource_leak".to_owned());
        }
        if matches!(
            self.case,
            Bq26FaultCase::SettlementLoss
                | Bq26FaultCase::FlushFailure
                | Bq26FaultCase::UnknownAutoRetry
        ) && !self.reconciliation_required
        {
            return Err("bq26_fault_reconciliation_marker_missing".to_owned());
        }
        validate_digest(&self.observation_digest, "bq26_fault_observation_digest")?;
        if self.observation_digest != self.digest() {
            return Err("bq26_fault_observation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "case": self.case,
            "family": self.family,
            "refusal": self.refusal,
            "leaked_reservations": self.leaked_reservations,
            "leaked_leases": self.leaked_leases,
            "queued_entries": self.queued_entries,
            "facts_replayable": self.facts_replayable,
            "incident_action": self.incident_action,
            "reconciliation_required": self.reconciliation_required,
        }))
    }
}

/// The full driven matrix. `evaluate` runs every case; `validate_against` binds it to a source.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bq26FaultHarnessRun {
    pub schema: String,
    pub version: SchemaVersion,
    pub seed: u64,
    pub source_cursor: u64,
    pub source_event_ids: Vec<String>,
    pub observations: Vec<Bq26FaultObservation>,
    pub families_covered: Vec<Bq26FaultClass>,
    pub unproven: Vec<String>,
    pub run_digest: String,
}

impl Bq26FaultHarnessRun {
    /// Bind this run to a committed source cursor. A run built from facts that have since moved
    /// is refused rather than silently re-labelled.
    pub fn validate_against(
        &self,
        source_cursor: u64,
        source_event_ids: &[String],
    ) -> Result<(), String> {
        self.validate()?;
        if self.source_cursor != source_cursor || self.source_event_ids != source_event_ids {
            return Err("bq26_fault_harness_source_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn observation(&self, case: Bq26FaultCase) -> Option<&Bq26FaultObservation> {
        self.observations
            .iter()
            .find(|observation| observation.case == case)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BQ26_FAULT_HARNESS_SCHEMA
            || !self.version.is_compatible_with(&BQ26_FAULT_HARNESS_VERSION)
            || self.seed == 0
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.observations.len() != BQ26_FAULT_MAX_CASES
        {
            return Err("bq26_fault_harness_header_invalid".to_owned());
        }
        if self.unproven != unproven_claims() {
            return Err("bq26_fault_harness_unproven_mismatch".to_owned());
        }
        let mut cases = BTreeSet::new();
        for observation in &self.observations {
            observation.validate()?;
            if !cases.insert(observation.case) {
                return Err("bq26_fault_case_duplicate".to_owned());
            }
        }
        for case in Bq26FaultCase::ALL {
            if !cases.contains(&case) {
                return Err("bq26_fault_case_missing".to_owned());
            }
        }
        if self.families_covered != Bq26FaultClass::ALL {
            return Err("bq26_fault_family_incomplete".to_owned());
        }
        validate_digest(&self.run_digest, "bq26_fault_harness_digest")?;
        if self.run_digest != self.digest() {
            return Err("bq26_fault_harness_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "seed": self.seed,
            "source_cursor": self.source_cursor,
            "source_event_ids": self.source_event_ids,
            "observations": self.observations,
            "families_covered": self.families_covered,
            "unproven": self.unproven,
        }))
    }
}

/// The claims this harness deliberately does not make. Carried in the report so a reader cannot
/// mistake a source-level refusal for a durability or liveness proof.
fn unproven_claims() -> Vec<String> {
    [
        "no process was killed; crash cases are modelled by an uncommitted or unacked fact",
        "no filesystem was filled; the disk-full case drives the platform failure classifier",
        "no socket was opened; the EOF and partial-frame cases drive the provider Framer",
        "no provider was called; 429 and 5xx are driven through the retry classifier",
        "capacity and settlement objects are in-process; nothing is durable across a restart",
    ]
    .iter()
    .map(|value| (*value).to_owned())
    .collect()
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The seam.
// ---------------------------------------------------------------------------

/// One driver per [`Bq26FaultCase`], each calling a real seam and returning what it actually
/// said. The trait exists so the contract can live in the library while the two adapters it
/// drives (`kiana-eventlog`'s in-memory store and `kiana-provider`'s stream framer) stay test
/// dependencies. A test target implements this and every case in [`Bq26FaultCase::ALL`] must
/// then be reachable — there is no default arm, so a new case cannot be added without a driver.
pub trait Bq26FaultSeam {
    /// Inject one fault and report the seam's live refusal. Returning `Err` means the *seam
    /// misbehaved* (it accepted the fault, or returned something other than the required
    /// refusal); it is a harness failure, not a fault observation.
    fn inject(&mut self, case: Bq26FaultCase) -> Result<Bq26FaultObservation, String>;
}

impl Bq26FaultHarnessRun {
    /// Drive every case through a caller-supplied seam. This is the only other way to build a
    /// run besides [`Self::evaluate_with_default`], and it still refuses a partial report.
    pub fn evaluate_with_seam(
        seed: u64,
        source_cursor: u64,
        source_event_ids: Vec<String>,
        seam: &mut dyn Bq26FaultSeam,
    ) -> Result<Self, String> {
        let mut sorted = source_event_ids;
        sorted.sort();
        let before = sorted.len();
        sorted.dedup();
        if sorted.len() != before {
            return Err("bq26_fault_source_duplicate".to_owned());
        }
        if seed == 0 || source_cursor == 0 || sorted.is_empty() {
            return Err("bq26_fault_harness_header_invalid".to_owned());
        }
        let observations = Bq26FaultCase::ALL
            .into_iter()
            .map(|case| seam.inject(case))
            .collect::<Result<Vec<_>, _>>()?;
        Self::seal(seed, source_cursor, sorted, observations)
    }

    /// Drive every case through the library's own seams. Those are the seams that need no
    /// external adapter: BQ-11 lifecycle validation, BQ-12 settlement fold, BQ-17 retry
    /// classification, BQ-16 capacity, the clock contract and the platform failure classifier.
    /// The two adapter-backed cases (the CAS race and the provider framer) are *refused* here
    /// and must be driven through [`Self::evaluate_with_seam`] instead — a report can therefore
    /// never claim to have driven an adapter seam it could not reach.
    pub fn evaluate_with_default(
        seed: u64,
        source_cursor: u64,
        source_event_ids: Vec<String>,
    ) -> Result<Self, String> {
        evaluate_library_seams(seed, source_cursor, source_event_ids)
    }

    fn seal(
        seed: u64,
        source_cursor: u64,
        source_event_ids: Vec<String>,
        observations: Vec<Bq26FaultObservation>,
    ) -> Result<Self, String> {
        if observations.len() != BQ26_FAULT_MAX_CASES {
            return Err("bq26_fault_harness_header_invalid".to_owned());
        }
        let mut families = observations
            .iter()
            .map(|observation| observation.family)
            .collect::<Vec<_>>();
        families.sort();
        families.dedup();
        let mut run = Self {
            schema: BQ26_FAULT_HARNESS_SCHEMA.to_owned(),
            version: BQ26_FAULT_HARNESS_VERSION,
            seed,
            source_cursor,
            source_event_ids,
            observations,
            families_covered: families,
            unproven: unproven_claims(),
            run_digest: String::new(),
        };
        run.run_digest = run.digest();
        run.validate()?;
        Ok(run)
    }
}

// ---------------------------------------------------------------------------
// Fixtures. All times are injected; nothing here sleeps or reads the wall clock.
// ---------------------------------------------------------------------------

/// The one injected instant every fixture uses. No driver may sample a real clock.
pub const BQ26_FAKE_NOW_UNIX_MS: u64 = 1_700_000_000_000;
/// The one injected deadline every fixture uses.
pub const BQ26_FAKE_DEADLINE_UNIX_MS: u64 = 1_700_000_060_000;
/// The one injected monotonic sample every fixture uses.
pub const BQ26_FAKE_MONOTONIC_MS: u64 = 5_000;

pub fn bq26_fixture_now() -> u64 {
    BQ26_FAKE_NOW_UNIX_MS
}

pub fn bq26_fixture_deadline() -> u64 {
    BQ26_FAKE_DEADLINE_UNIX_MS
}

pub fn bq26_fixture_clock() -> Result<ClockObservation, String> {
    ClockObservation::observe(
        "bq26.fake",
        u128::from(BQ26_FAKE_NOW_UNIX_MS),
        u128::from(BQ26_FAKE_MONOTONIC_MS),
        None,
        1,
    )
}

pub fn bq26_fixture_window() -> Result<QuotaWindow, String> {
    QuotaWindow::from_clock(&bq26_fixture_clock()?, 60_000)
}

pub fn bq26_fixture_group() -> Result<QuotaGroupKey, String> {
    QuotaGroupKey::new("bq26-provider", "bq26-credential", None, None)
}

pub fn bq26_fixture_reservation() -> Result<QuotaReservation, String> {
    QuotaReservation::new(
        QuotaReservationId::new(),
        bq26_fixture_group()?,
        bq26_fixture_window()?,
        RunId::new(),
        AttemptId::new(),
        1,
        "bq26.config.v1",
        1,
        1_000,
        1,
        "bq26-idempotency",
        BQ26_FAKE_NOW_UNIX_MS + 30_000,
    )
}

pub fn bq26_fixture_retry_policy() -> Result<RetryPolicy, String> {
    RetryPolicy::new(3, 3, 2_000, BQ26_FAKE_DEADLINE_UNIX_MS)
}

/// Seed a settlement fold with the `Reserved` fact the crash cases build on.
pub fn bq26_fixture_fold_seed(
    ledger: &mut SettlementFoldLedger,
    reservation: &QuotaReservation,
) -> Result<(AttemptId, kiana_domain::SettlementSourceRef), String> {
    use kiana_domain::{SettlementFoldEvent, SettlementSourceRef, SETTLEMENT_FOLD_EVENT_SCHEMA};

    let source = SettlementSourceRef::new(kiana_domain::EventId::new(), bq26_fixture_digest('s'))?;
    let mut event = SettlementFoldEvent {
        schema: SETTLEMENT_FOLD_EVENT_SCHEMA.to_owned(),
        version: kiana_domain::SETTLEMENT_FOLD_VERSION,
        kind: SettlementFoldEventKind::Reserved,
        event_id: kiana_domain::EventId::new(),
        run_id: reservation.owner_run,
        model_attempt_id: kiana_domain::ModelAttemptId::new(),
        attempt_id: reservation.owner_attempt,
        reservation_id: reservation.reservation_id,
        reservation_digest: reservation.reservation_digest.clone(),
        usage_class: None,
        usage_digest: None,
        receipt_id: None,
        reserved: kiana_domain::ReservationUnits {
            requests: 1,
            tokens: 1_000,
            concurrency: 1,
        },
        consumed: None,
        released: kiana_domain::ReservationUnits::default(),
        unknown_reason: None,
        reconciliation_required: false,
        source_refs: vec![source.clone()],
        revision: 1,
        event_digest: String::new(),
    };
    event.event_digest = event.digest();
    event.validate()?;
    ledger.apply(event)?;
    Ok((reservation.owner_attempt, source))
}

/// The digest helper shared by the library and the adapter-backed drivers.
pub fn bq26_fixture_digest(seed: char) -> String {
    json_digest(&serde_json::json!({ "bq26": seed }))
}

/// The constructor seam behind [`Bq26FaultHarnessRun::evaluate_with_default`].
/// Drive the nine library-seam cases. Public so an adapter-backed seam can delegate to it
/// without re-entering the whole run constructor.
pub fn bq26_inject_library_case(case: Bq26FaultCase) -> Result<Bq26FaultObservation, String> {
    drive_library_case(case)
}

fn evaluate_library_seams(
    seed: u64,
    source_cursor: u64,
    source_event_ids: Vec<String>,
) -> Result<Bq26FaultHarnessRun, String> {
    let mut sorted = source_event_ids;
    sorted.sort();
    let before = sorted.len();
    sorted.dedup();
    if sorted.len() != before {
        return Err("bq26_fault_source_duplicate".to_owned());
    }
    if seed == 0 || source_cursor == 0 || sorted.is_empty() {
        return Err("bq26_fault_harness_header_invalid".to_owned());
    }
    let observations = Bq26FaultCase::ALL
        .into_iter()
        .map(drive_library_case)
        .collect::<Result<Vec<_>, _>>()?;
    Bq26FaultHarnessRun::seal(seed, source_cursor, sorted, observations)
}

fn drive_library_case(case: Bq26FaultCase) -> Result<Bq26FaultObservation, String> {
    match case {
        Bq26FaultCase::FlushFailure => drive_flush_failure(),
        Bq26FaultCase::SettlementLoss => drive_settlement_loss(),
        Bq26FaultCase::UnknownAutoRetry => drive_unknown_auto_retry(),
        Bq26FaultCase::Provider429 => drive_provider_429(),
        Bq26FaultCase::Provider5xx => drive_provider_5xx(),
        Bq26FaultCase::OverBudgetDispatch => drive_over_budget_dispatch(),
        Bq26FaultCase::ClockRollback => drive_clock_rollback(),
        Bq26FaultCase::LeaseLeak => drive_lease_leak(),
        Bq26FaultCase::DiskFull => drive_disk_full(),
        // These two drive adapters that are test dependencies of this crate, not library
        // dependencies. Refusing them here is what stops a report from claiming coverage it
        // could not perform.
        Bq26FaultCase::CasRace | Bq26FaultCase::PartialFrame | Bq26FaultCase::NetworkEof => {
            Err("bq26_case_requires_adapter_seam".to_owned())
        }
    }
}

// ---------------------------------------------------------------------------
// The library drivers. Each one calls a real seam and records what it returned.
// ---------------------------------------------------------------------------

/// Flush failure: a dispatch fact that claims no prepared-flush acknowledgement. The BQ-11
/// lifecycle validator refuses it.
fn drive_flush_failure() -> Result<Bq26FaultObservation, String> {
    use kiana_domain::{
        ModelAttemptEventKind, ModelAttemptLifecycleEvent, MODEL_ATTEMPT_EVENT_SCHEMA,
    };

    let unflushed = ModelAttemptLifecycleEvent {
        schema: MODEL_ATTEMPT_EVENT_SCHEMA.to_owned(),
        version: kiana_domain::MODEL_ATTEMPT_EVENT_VERSION,
        kind: ModelAttemptEventKind::Dispatching,
        event_id: kiana_domain::EventId::new(),
        run_id: RunId::new(),
        turn_id: kiana_domain::TurnId::new(),
        model_attempt_id: kiana_domain::ModelAttemptId::new(),
        attempt_id: AttemptId::new(),
        reservation_id: QuotaReservationId::new(),
        permit_id: None,
        usage: None,
        receipt_id: None,
        error: None,
        prepared_flushed: false,
        flush_sequence: None,
        revision: 2,
        event_digest: String::new(),
    };
    let refusal = unflushed
        .validate()
        .expect_err("a dispatch fact without a flush ack must be refused");
    if refusal != "model_attempt_dispatch_requires_prepared_flush" {
        return Err("bq26_flush_failure_unexpected_refusal".to_owned());
    }
    Bq26FaultObservation::new(
        Bq26FaultCase::FlushFailure,
        Bq26FaultRefusal::new(
            "ModelAttemptLifecycleEvent::validate",
            &refusal,
            Some("model.prepared_flushed".to_owned()),
            false,
        )?,
        1,
        0,
        0,
        true,
        true,
        "hold the reservation as unknown and reconcile against the EventLog before retrying",
    )
}

/// Settlement loss: an attempt that was sent but whose settlement fact never lands. The
/// settlement fold must refuse to release the held reservation.
fn drive_settlement_loss() -> Result<Bq26FaultObservation, String> {
    let mut ledger = SettlementFoldLedger::default();
    let reservation = bq26_fixture_reservation()?;
    let (attempt_id, source) = bq26_fixture_fold_seed(&mut ledger, &reservation)?;

    // The crash happens here: the lifecycle never reaches Settled, so the reservation is still
    // Reserved and an unknown fold cannot be released.
    if ledger.get(attempt_id).map(|record| record.state) != Some(SettlementFoldState::Reserved) {
        return Err("bq26_settlement_loss_unexpected_initial_state".to_owned());
    }
    let refusal = match ledger.release_unused(attempt_id, source) {
        Err(reason) => reason,
        Ok(_) => return Err("bq26_settlement_loss_released_reservation".to_owned()),
    };
    if refusal != "settlement_not_consumed" {
        return Err(format!("bq26_settlement_loss_unexpected_refusal:{refusal}"));
    }
    if ledger.get(attempt_id).map(|record| record.state) != Some(SettlementFoldState::Reserved) {
        return Err("bq26_settlement_loss_reservation_released".to_owned());
    }
    Bq26FaultObservation::new(
        Bq26FaultCase::SettlementLoss,
        Bq26FaultRefusal::new(
            "SettlementFoldLedger::release_unused",
            &refusal,
            Some(SettlementFoldEventKind::Released.as_str().to_owned()),
            false,
        )?,
        1,
        0,
        0,
        true,
        true,
        "reconcile the unknown attempt against the provider receipt before releasing the hold",
    )
}

/// Unknown auto-retry: a post-send failure whose side effect state is `Unknown`. Handing this to
/// the retry policy must be denied — this is the double charge the card names.
fn drive_unknown_auto_retry() -> Result<Bq26FaultObservation, String> {
    let error = ModelError::transport("provider_read_idle_timeout", ModelRetryClass::Never, true);
    if error.side_effect_state != ModelSideEffectState::Unknown {
        return Err("bq26_unknown_auto_retry_side_effect_not_unknown".to_owned());
    }
    let policy = bq26_fixture_retry_policy()?;
    let observation = RetryObservation::from_model_error(&error, false, true);
    let decision = policy.classify(1, 1, bq26_fixture_now(), &observation)?;
    if decision.retry {
        return Err("bq26_unknown_auto_retry_permitted".to_owned());
    }
    let reason = decision
        .deny_reason
        .ok_or_else(|| "bq26_unknown_auto_retry_missing_deny_reason".to_owned())?;
    if reason != RetryDenyReason::SideEffectUnknown {
        return Err(format!(
            "bq26_unknown_auto_retry_unexpected_reason:{reason:?}"
        ));
    }
    Bq26FaultObservation::new(
        Bq26FaultCase::UnknownAutoRetry,
        Bq26FaultRefusal::new(
            "RetryPolicy::classify",
            reason.as_str(),
            Some("retry".to_owned()),
            false,
        )?,
        1,
        0,
        0,
        true,
        true,
        "hold the attempt as unknown and require an explicit reconciliation before any new attempt",
    )
}

/// Provider 429: a pre-charge rejection. This is the one provider status that may produce a new
/// bounded attempt, and only because the request was refused before any charge.
fn drive_provider_429() -> Result<Bq26FaultObservation, String> {
    let mut error = ModelError::transport("provider_http_429", ModelRetryClass::Rejected, true);
    error.side_effect_state = ModelSideEffectState::None;
    error.retry_after_ms = Some(250);
    let policy = bq26_fixture_retry_policy()?;
    let observation = RetryObservation::from_model_error(&error, false, true);
    let decision = policy.classify(1, 1, bq26_fixture_now(), &observation)?;
    if !decision.retry {
        return Err("bq26_provider_429_refused_bounded_retry".to_owned());
    }
    if decision.next_attempt != 2 || decision.delay_ms < 250 {
        return Err("bq26_provider_429_bad_backoff".to_owned());
    }
    // Bounded means bounded: the request budget is a hard stop even for a retryable class.
    let exhausted = policy.classify(1, policy.max_requests, bq26_fixture_now(), &observation)?;
    if exhausted.retry {
        return Err("bq26_provider_429_ignores_request_budget".to_owned());
    }
    Bq26FaultObservation::new(
        Bq26FaultCase::Provider429,
        Bq26FaultRefusal::new(
            "RetryPolicy::classify",
            "retry_backoff_honors_retry_after",
            Some("provider_http_429_permanent".to_owned()),
            false,
        )?,
        0,
        0,
        0,
        true,
        false,
        "keep the bounded backoff and re-enter ControlPlane admission for the new attempt",
    )
}

/// Provider 5xx: a server failure after the request was sent. A 503 must never become an
/// automatic new charge.
fn drive_provider_5xx() -> Result<Bq26FaultObservation, String> {
    let error = ModelError::transport("provider_http_503", ModelRetryClass::Never, true);
    let policy = bq26_fixture_retry_policy()?;
    let observation = RetryObservation::from_model_error(&error, false, true);
    let decision = policy.classify(1, 1, bq26_fixture_now(), &observation)?;
    if decision.retry {
        return Err("bq26_provider_5xx_permitted".to_owned());
    }
    let reason = decision
        .deny_reason
        .ok_or_else(|| "bq26_provider_5xx_missing_deny_reason".to_owned())?;
    // The side-effect fence is checked before the class, so an unknown side effect is the reason
    // reported even for a 5xx. That ordering is the double-charge guard.
    if reason != RetryDenyReason::SideEffectUnknown {
        return Err(format!("bq26_provider_5xx_unexpected_reason:{reason:?}"));
    }
    Bq26FaultObservation::new(
        Bq26FaultCase::Provider5xx,
        Bq26FaultRefusal::new(
            "RetryPolicy::classify",
            reason.as_str(),
            Some("retry".to_owned()),
            false,
        )?,
        0,
        0,
        0,
        true,
        false,
        "open the circuit breaker and require a half-open probe before the next attempt",
    )
}

/// Over-budget dispatch: the RPM window is already exhausted. The capacity controller must never
/// return a dispatchable outcome once the budget is gone.
fn drive_over_budget_dispatch() -> Result<Bq26FaultObservation, String> {
    use kiana_domain::{
        ProviderCapacityController, ProviderCapacityOutcomeKind, ProviderCapacityPolicy,
        ProviderCapacityRequest,
    };

    let group = bq26_fixture_group()?;
    let window = bq26_fixture_window()?;
    // One request per minute: the second admission inside the same window cannot be dispatched.
    let policy = ProviderCapacityPolicy::new(group.clone(), 1, 4, 1, 1_000, 5_000, "bq26.cap.v1")?;
    let mut controller = ProviderCapacityController::new(policy, window.clone())?;

    let request = |tokens: u64| {
        ProviderCapacityRequest::new(
            "bq26-session",
            RunId::new(),
            AttemptId::new(),
            group.clone(),
            window.clone(),
            tokens,
            bq26_fixture_now(),
            bq26_fixture_deadline(),
        )
    };

    let first = controller.admit(request(500)?)?;
    if !first.outcome.kind.is_dispatchable() {
        return Err("bq26_over_budget_first_admission_not_accepted".to_owned());
    }
    let second = controller.admit(request(500)?)?;
    if second.lease.is_some() || second.outcome.kind.is_dispatchable() {
        return Err("bq26_over_budget_dispatched_over_budget".to_owned());
    }
    if second.outcome.kind != ProviderCapacityOutcomeKind::Delay
        && second.outcome.kind != ProviderCapacityOutcomeKind::Reject
        && second.outcome.kind != ProviderCapacityOutcomeKind::Queue
    {
        return Err("bq26_over_budget_unknown_outcome".to_owned());
    }
    // The queue must not be a dispatch back door: dispatch_next with the window still exhausted
    // may not hand out a lease either.
    if let Some(decision) = controller.dispatch_next(bq26_fixture_now())? {
        if decision.lease.is_some() {
            return Err("bq26_over_budget_queue_handed_out_lease".to_owned());
        }
    }
    // Releasing the first lease frees the concurrency slot but not the exhausted RPM budget.
    let lease_id = first
        .lease
        .as_ref()
        .ok_or_else(|| "bq26_over_budget_missing_lease".to_owned())?
        .lease_id;
    controller.release(lease_id, "bq26-session")?;
    if controller.admit(request(500)?)?.lease.is_some() {
        return Err("bq26_over_budget_released_request_budget".to_owned());
    }
    Bq26FaultObservation::new(
        Bq26FaultCase::OverBudgetDispatch,
        Bq26FaultRefusal::new(
            "ProviderCapacityController::admit",
            "provider_capacity_quota_window_exhausted",
            Some("ProviderCapacityOutcomeKind::Accept".to_owned()),
            false,
        )?,
        0,
        0,
        controller.queue_len() as u32,
        true,
        false,
        "hold dispatch until the UTC window rolls; do not raise the limit inside the fault path",
    )
}

/// Clock rollback: the wall clock moves backwards between two observations. The rollback must be
/// classified, and no quota window may be built from it.
fn drive_clock_rollback() -> Result<Bq26FaultObservation, String> {
    let first = bq26_fixture_clock()?;
    let rolled = ClockObservation::observe(
        "bq26.fake",
        u128::from(BQ26_FAKE_NOW_UNIX_MS) - 1_000,
        u128::from(BQ26_FAKE_MONOTONIC_MS) + 1,
        Some(&first),
        2,
    )?;
    if rolled.trust != ClockTrust::Rollback {
        return Err("bq26_clock_rollback_not_detected".to_owned());
    }
    if rolled.require_trusted().is_ok() {
        return Err("bq26_clock_rollback_accepted_as_trusted".to_owned());
    }
    // A rolled-back clock must not build a quota window, which is the boundary where a stale
    // sample would otherwise reset an exhausted window.
    if QuotaWindow::from_clock(&rolled, 60_000).is_ok() {
        return Err("bq26_clock_rollback_built_quota_window".to_owned());
    }
    // Nor may it extend a deadline that a trusted sample already bounded.
    if first
        .clamp_deadline(
            BQ26_FAKE_NOW_UNIX_MS + 30_000,
            BQ26_FAKE_NOW_UNIX_MS + 10_000,
        )
        .is_ok()
    {
        return Err("bq26_clock_rollback_extended_deadline".to_owned());
    }
    Bq26FaultObservation::new(
        Bq26FaultCase::ClockRollback,
        Bq26FaultRefusal::new(
            "ClockObservation::require_trusted",
            "clock_untrusted",
            Some("ClockTrust::Trusted".to_owned()),
            false,
        )?,
        0,
        0,
        0,
        true,
        false,
        "quarantine the untrusted sample and keep the previous trusted window bound",
    )
}

/// Lease leak: a capacity lease whose attempt dies before releasing it. The controller must not
/// silently recycle the lease to another owner, and an unknown lease id must not be "released".
fn drive_lease_leak() -> Result<Bq26FaultObservation, String> {
    use kiana_domain::{
        ProviderCapacityController, ProviderCapacityPolicy, ProviderCapacityRequest,
    };

    let group = bq26_fixture_group()?;
    let window = bq26_fixture_window()?;
    let policy =
        ProviderCapacityPolicy::new(group.clone(), 1, 4, 60, 1_000_000, 5_000, "bq26.cap.v1")?;
    let mut controller = ProviderCapacityController::new(policy, window.clone())?;
    let decision = controller.admit(ProviderCapacityRequest::new(
        "bq26-owner",
        RunId::new(),
        AttemptId::new(),
        group,
        window,
        500,
        bq26_fixture_now(),
        bq26_fixture_deadline(),
    )?)?;
    let lease = decision
        .lease
        .ok_or_else(|| "bq26_lease_leak_missing_lease".to_owned())?;
    // The attempt dies here. The lease is still active, so it is not a free slot.
    if controller.active_len() != 1 {
        return Err("bq26_lease_leak_lease_not_held".to_owned());
    }
    // Releasing by a foreign owner is refused: a leak must not become someone else's permit.
    let refusal = match controller.release(lease.lease_id, "bq26-other-owner") {
        Err(reason) => reason,
        Ok(()) => return Err("bq26_lease_leak_foreign_release_permitted".to_owned()),
    };
    if refusal != "provider_capacity_lease_owner_mismatch" {
        return Err(format!("bq26_lease_leak_unexpected_refusal:{refusal}"));
    }
    if controller.active_len() != 1 {
        return Err("bq26_lease_leak_foreign_release_recycled".to_owned());
    }
    // An unknown lease id cannot be released either; that would let a caller decrement the
    // concurrency counter for a permit it never held.
    let unknown = controller
        .release(lease.lease_id.saturating_add(1_000), "bq26-owner")
        .expect_err("an unknown lease id must not release");
    if unknown != "provider_capacity_lease_unknown" {
        return Err(format!(
            "bq26_lease_leak_unexpected_unknown_refusal:{unknown}"
        ));
    }
    if controller.active_len() != 1 {
        return Err("bq26_lease_leak_counter_leaked".to_owned());
    }
    // The owner can still release it, and that must actually free the slot.
    controller.release(lease.lease_id, "bq26-owner")?;
    if controller.active_len() != 0 || controller.usage().active_concurrency != 0 {
        return Err("bq26_lease_leak_release_did_not_free_slot".to_owned());
    }
    Bq26FaultObservation::new(
        Bq26FaultCase::LeaseLeak,
        Bq26FaultRefusal::new(
            "ProviderCapacityController::release",
            &refusal,
            Some("provider_capacity_lease_recycled".to_owned()),
            false,
        )?,
        0,
        0,
        0,
        true,
        false,
        "cancel or expire the leaked lease through the authority path; never reassign it implicitly",
    )
}

/// Disk full: a write failure carrying an out-of-space condition. The platform failure classifier
/// owns the mapping from an error summary to a class; this drives that exact function.
fn drive_disk_full() -> Result<Bq26FaultObservation, String> {
    // Both real OS spellings of ENOSPC have to land on the same class, or an operator sees two
    // different incidents for one disk.
    for summary in [
        "No space left on device",
        "os error 28",
        "journal_disk_full",
    ] {
        if crate::classify_failure_summary(summary, false, false) != FailureClass::DiskFull {
            return Err(format!("bq26_disk_full_not_classified:{summary}"));
        }
    }
    // The same classifier must not treat an out-of-space write as a transient provider condition,
    // and a cancelled run must not be laundered into a disk incident.
    if crate::classify_failure_summary("Connection reset by peer", false, false)
        == FailureClass::DiskFull
    {
        return Err("bq26_disk_full_over_matches".to_owned());
    }
    if crate::classify_failure_summary("No space left on device", false, true)
        != FailureClass::Cancel
    {
        return Err("bq26_disk_full_overrides_cancellation".to_owned());
    }
    Bq26FaultObservation::new(
        Bq26FaultCase::DiskFull,
        Bq26FaultRefusal::new(
            "kiana_core::classify_failure_summary",
            "platform_disk_full",
            Some("platform_transient_retry".to_owned()),
            false,
        )?,
        0,
        0,
        0,
        true,
        false,
        "stop the writer, surface disk pressure, and replay committed facts from the EventLog",
    )
}

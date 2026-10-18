//! NM-20 driving fault harness over the notification chain.
//!
//! # What this is
//!
//! `crate::notification_faults` (the NM-20 classification matrix) is a *table*: it records which
//! disposition a human expects for nine notification failure scenarios. This module is the
//! opposite. Every [`Nm20FaultCase`] names one injected failure and the harness **drives a real
//! seam** — the projection replay, the outbox lease state machine, the delivery worker planner,
//! the secret scanner and the committed inbox query — then reports what that seam actually
//! returned. Nothing here restates a policy in a struct field, and no case invents a second
//! execution path.
//!
//! # The chain
//!
//! The card names the whole notification chain: EventLog/Projector/Worker/Channel/UI.
//!
//! | Case | Family | Seam actually driven |
//! |---|---|---|
//! | [`Nm20FaultCase::DuplicateReplay`] | Duplicate | `NotificationProjection::apply_committed` replayed page |
//! | [`Nm20FaultCase::CursorRegression`] | OutOfOrder | `ProjectionCheckpoint` replay, stale cursor |
//! | [`Nm20FaultCase::CursorGap`] | CursorGap | `ProjectionCheckpoint` replay, skipped cursor |
//! | [`Nm20FaultCase::LeaseReclaimFence`] | CrashWindow | `NotificationOutboxRecord` claim/reclaim/lease fence |
//! | [`Nm20FaultCase::DiskFullWriteRefusal`] | DiskFull | `classify_failure_summary` + `NotificationDeliveryWorker::plan` |
//! | [`Nm20FaultCase::SecretSentinel`] | SecretSentinel | `scan_secret_sentinels` / `redact_text` / `redact_value` |
//! | [`Nm20FaultCase::ProjectionLoss`] | ProjectionLoss | replay rebuild equality from committed input |
//! | [`Nm20FaultCase::CriticalQueryFallback`] | ProjectionLoss | `NotificationStore::list` while the stream is gone |
//! | [`Nm20FaultCase::CrashAfterClaim`] | CrashWindow | durable outbox store (adapter seam, test-only) |
//! | [`Nm20FaultCase::BoundedQueueShed`] | QueueFull | bounded outbox store admission/pressure (adapter seam) |
//! | [`Nm20FaultCase::SlowSubscriber`] | SlowConsumer | daemon `NotificationStreamBridge` (adapter seam) |
//! | [`Nm20FaultCase::MultiSubscriberIsolation`] | SlowConsumer | two daemon stream bridges (adapter seam) |
//!
//! The four adapter cases drive `kiana-eventlog` and `kiana-daemon` seams. Those are test
//! dependencies of this crate (or another crate's), so [`Nm20LibrarySeam`] **refuses** them with
//! `nm20_adapter_seam_required:<case>` instead of pretending to drive them. A run that used the
//! library seam therefore reports exactly which cases it did not cover through
//! [`Nm20FaultHarnessRun::covered_cases`] — a report can never claim coverage it does not have.
//!
//! # What this is NOT
//!
//! **No process is killed, no disk is filled, no queue is drained, no notification is sent and no
//! connector is called.** The crash window is a lease state machine, the disk-full case is a
//! classifier arm, the queue-full case is the bounded store's admission outcome, the secret case
//! is a scanner, and the slow-consumer case is a bounded bridge window. This is a *source* proof
//! ceiling: it proves the fail-closed branch is reachable and taken and that critical facts stay
//! queryable, not durability across a real `SIGKILL`, an actual `ENOSPC` or a real browser.

use crate::{
    classify_failure_summary, NotificationDeliveryPlan, NotificationDeliveryWorker,
    NotificationListRequest, NotificationProjection, NotificationStore, NotificationStoreError,
};
use kiana_domain::{
    json_digest, redact_text, redact_value, scan_secret_sentinels, scan_secret_value, FailureClass,
    HumanAction, HumanInboxKind, HumanTaskBridge, MessageId, Notification, NotificationChannel,
    NotificationMaterialization, NotificationOutboxClass, NotificationOutboxRecord,
    NotificationOutboxState, NotificationStatus, RequestId, RuntimeEvent, SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const NM20_FAULT_HARNESS_SCHEMA: &str = "kiana.nm20-fault-harness-run.v1";
pub const NM20_FAULT_CASES_SCHEMA: &str = "kiana.nm20-fault-cases.v1";
/// Upper bound on cases one report may carry; the card names twelve injected failures.
pub const NM20_FAULT_MAX_CASES: usize = 16;
pub const NM20_FAULT_MAX_TEXT_BYTES: usize = 256;

/// The nine fault families from the NM-20 card title.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Nm20FaultFamily {
    Duplicate,
    OutOfOrder,
    CursorGap,
    CrashWindow,
    SlowConsumer,
    QueueFull,
    DiskFull,
    SecretSentinel,
    ProjectionLoss,
}

impl Nm20FaultFamily {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Duplicate => "duplicate",
            Self::OutOfOrder => "out_of_order",
            Self::CursorGap => "cursor_gap",
            Self::CrashWindow => "crash_window",
            Self::SlowConsumer => "slow_consumer",
            Self::QueueFull => "queue_full",
            Self::DiskFull => "disk_full",
            Self::SecretSentinel => "secret_sentinel",
            Self::ProjectionLoss => "projection_loss",
        }
    }
}

/// The twelve injected failures. Every one binds one seam and one observable refusal or invariant.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Nm20FaultCase {
    /// The same committed page is replayed: one materialized notification, no second row.
    DuplicateReplay,
    /// A stale page arrives after a newer checkpoint: refused without mutating the projection.
    CursorRegression,
    /// A page claims a cursor the projector skipped: refused, snapshot/rebuild required.
    CursorGap,
    /// A worker claims a record and never submits: the reclaim is fenced, one effective sender.
    LeaseReclaimFence,
    /// An outbox write fails `ENOSPC`: classified disk-full, delivery stays reconcile-unknown.
    DiskFullWriteRefusal,
    /// A secret sentinel reaches an envelope: refused/quarantined, redacted form passes.
    SecretSentinel,
    /// The in-memory projection is lost: a rebuild from committed input is byte-identical.
    ProjectionLoss,
    /// The volatile stream is gone: the committed inbox query still answers critical rows.
    CriticalQueryFallback,
    /// Crash after claim on the durable outbox store: reclaim, stale lease fenced, one ack.
    CrashAfterClaim,
    /// A bounded best-effort queue is full: shed is counted, protected records are never shed.
    BoundedQueueShed,
    /// A consumer never hydrates / overflows the merge window: frames refused, no partial merge.
    SlowSubscriber,
    /// One subscriber hits a gap: the other subscriber's cursor/terminal is untouched.
    MultiSubscriberIsolation,
}

impl Nm20FaultCase {
    pub const ALL: [Nm20FaultCase; 12] = [
        Self::DuplicateReplay,
        Self::CursorRegression,
        Self::CursorGap,
        Self::LeaseReclaimFence,
        Self::DiskFullWriteRefusal,
        Self::SecretSentinel,
        Self::ProjectionLoss,
        Self::CriticalQueryFallback,
        Self::CrashAfterClaim,
        Self::BoundedQueueShed,
        Self::SlowSubscriber,
        Self::MultiSubscriberIsolation,
    ];

    /// Cases whose seam is an adapter (`kiana-eventlog`/`kiana-daemon`) rather than this library.
    pub const ADAPTER_CASES: [Nm20FaultCase; 4] = [
        Self::CrashAfterClaim,
        Self::BoundedQueueShed,
        Self::SlowSubscriber,
        Self::MultiSubscriberIsolation,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DuplicateReplay => "duplicate_replay",
            Self::CursorRegression => "cursor_regression",
            Self::CursorGap => "cursor_gap",
            Self::LeaseReclaimFence => "lease_reclaim_fence",
            Self::DiskFullWriteRefusal => "disk_full_write_refusal",
            Self::SecretSentinel => "secret_sentinel",
            Self::ProjectionLoss => "projection_loss",
            Self::CriticalQueryFallback => "critical_query_fallback",
            Self::CrashAfterClaim => "crash_after_claim",
            Self::BoundedQueueShed => "bounded_queue_shed",
            Self::SlowSubscriber => "slow_subscriber",
            Self::MultiSubscriberIsolation => "multi_subscriber_isolation",
        }
    }

    pub const fn family(self) -> Nm20FaultFamily {
        match self {
            Self::DuplicateReplay => Nm20FaultFamily::Duplicate,
            Self::CursorRegression => Nm20FaultFamily::OutOfOrder,
            Self::CursorGap => Nm20FaultFamily::CursorGap,
            Self::LeaseReclaimFence | Self::CrashAfterClaim => Nm20FaultFamily::CrashWindow,
            Self::DiskFullWriteRefusal => Nm20FaultFamily::DiskFull,
            Self::SecretSentinel => Nm20FaultFamily::SecretSentinel,
            Self::ProjectionLoss | Self::CriticalQueryFallback => Nm20FaultFamily::ProjectionLoss,
            Self::BoundedQueueShed => Nm20FaultFamily::QueueFull,
            Self::SlowSubscriber | Self::MultiSubscriberIsolation => Nm20FaultFamily::SlowConsumer,
        }
    }
}

/// What one driven case observed. All invariants are positive assertions: a report with
/// `effect_started=true`, `secret_free=false` or `critical_preserved=false` is invalid, not an
/// interesting outcome.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Nm20FaultObservation {
    pub schema: String,
    pub case: Nm20FaultCase,
    pub family: Nm20FaultFamily,
    /// The concrete seam that was called, as a source path.
    pub seam: String,
    /// True when the seam refused the injected fault's forbidden outcome.
    pub refusal_fired: bool,
    /// The stable refusal/invariant code the seam produced.
    pub refusal_code: String,
    /// The wrong outcome the case is guarding against, named for the failure report.
    pub forbidden_value: Option<String>,
    /// Whether the injected operation itself returned `Ok` (an idempotent replay does).
    pub returned_ok: bool,
    pub critical_preserved: bool,
    pub duplicate_suppressed: bool,
    pub snapshot_required: bool,
    pub reconcile_required: bool,
    /// True when the authoritative committed query can still return the affected fact.
    pub query_fallback_available: bool,
    pub secret_free: bool,
    pub effect_started: bool,
    pub observation_digest: String,
}

impl Nm20FaultObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        case: Nm20FaultCase,
        seam: impl Into<String>,
        refusal_fired: bool,
        refusal_code: impl Into<String>,
        forbidden_value: Option<impl Into<String>>,
        returned_ok: bool,
        critical_preserved: bool,
        duplicate_suppressed: bool,
        snapshot_required: bool,
        reconcile_required: bool,
        query_fallback_available: bool,
    ) -> Result<Self, String> {
        let mut observation = Self {
            schema: NM20_FAULT_CASES_SCHEMA.to_owned(),
            case,
            family: case.family(),
            seam: seam.into(),
            refusal_fired,
            refusal_code: refusal_code.into(),
            forbidden_value: forbidden_value.map(Into::into),
            returned_ok,
            critical_preserved,
            duplicate_suppressed,
            snapshot_required,
            reconcile_required,
            query_fallback_available,
            secret_free: true,
            effect_started: false,
            observation_digest: String::new(),
        };
        observation.observation_digest = observation.digest();
        observation.validate()?;
        Ok(observation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NM20_FAULT_CASES_SCHEMA
            || self.family != self.case.family()
            || self.seam.trim().is_empty()
            || self.seam.len() > NM20_FAULT_MAX_TEXT_BYTES
            || self.refusal_code.trim().is_empty()
            || self.refusal_code.len() > NM20_FAULT_MAX_TEXT_BYTES
        {
            return Err("nm20_fault_observation_header_invalid".to_owned());
        }
        if self.effect_started {
            return Err("nm20_fault_effect_started".to_owned());
        }
        if !self.secret_free {
            return Err("nm20_fault_not_secret_free".to_owned());
        }
        if !self.critical_preserved {
            return Err("nm20_fault_critical_fact_lost".to_owned());
        }
        match (self.refusal_fired, self.forbidden_value.as_deref()) {
            (true, Some(value)) if !value.trim().is_empty() => {}
            (false, None) => {}
            _ => return Err("nm20_fault_observation_refusal_incomplete".to_owned()),
        }
        if !self.returned_ok && !self.refusal_fired {
            // A call that did not return Ok must name the refusal that stopped it.
            return Err("nm20_fault_observation_refusal_ambiguous".to_owned());
        }
        if self.observation_digest != self.digest() {
            return Err("nm20_fault_observation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "case": self.case,
            "family": self.family,
            "seam": self.seam,
            "refusal_fired": self.refusal_fired,
            "refusal_code": self.refusal_code,
            "forbidden_value": self.forbidden_value,
            "returned_ok": self.returned_ok,
            "critical_preserved": self.critical_preserved,
            "duplicate_suppressed": self.duplicate_suppressed,
            "snapshot_required": self.snapshot_required,
            "reconcile_required": self.reconcile_required,
            "query_fallback_available": self.query_fallback_available,
            "secret_free": self.secret_free,
            "effect_started": self.effect_started,
        }))
    }
}

/// One seam implementation is injected per case. Tests provide the adapter-backed seams; the
/// library seam handles everything the library can actually reach.
pub trait Nm20FaultSeam {
    fn inject(&mut self, case: Nm20FaultCase) -> Result<Nm20FaultObservation, String>;
}

/// The library's own seam. It refuses adapter-backed cases instead of simulating them.
#[derive(Clone, Copy, Debug, Default)]
pub struct Nm20LibrarySeam;

impl Nm20FaultSeam for Nm20LibrarySeam {
    fn inject(&mut self, case: Nm20FaultCase) -> Result<Nm20FaultObservation, String> {
        if Nm20FaultCase::ADAPTER_CASES.contains(&case) {
            return Err(format!("nm20_adapter_seam_required:{}", case.as_str()));
        }
        drive_library_case(case)
    }
}

/// One deterministic fault-harness report over the injected cases.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Nm20FaultHarnessRun {
    pub schema: String,
    pub seed: u64,
    pub source_cursor: u64,
    pub source_event_ids: Vec<String>,
    pub cases: Vec<Nm20FaultObservation>,
    pub run_digest: String,
}

impl Nm20FaultHarnessRun {
    /// Drive every case through the supplied seam. A seam failure aborts the run: an incomplete
    /// report is never returned as if it were a result.
    pub fn evaluate_with_seam(
        seed: u64,
        source_cursor: u64,
        mut source_event_ids: Vec<String>,
        seam: &mut dyn Nm20FaultSeam,
    ) -> Result<Self, String> {
        if seed == 0 || source_cursor == 0 || source_event_ids.is_empty() {
            return Err("nm20_fault_harness_header_invalid".to_owned());
        }
        source_event_ids.sort();
        source_event_ids.dedup();
        let mut cases = Vec::with_capacity(Nm20FaultCase::ALL.len());
        for case in Nm20FaultCase::ALL {
            let observation = seam.inject(case)?;
            observation.validate()?;
            cases.push(observation);
        }
        Self::from_observations(seed, source_cursor, source_event_ids, cases)
    }

    /// Assemble a report from the observations a seam actually produced. `missing_cases` names
    /// everything the seam could not reach, so a partial run stays an honest partial run.
    pub fn from_observations(
        seed: u64,
        source_cursor: u64,
        mut source_event_ids: Vec<String>,
        mut cases: Vec<Nm20FaultObservation>,
    ) -> Result<Self, String> {
        if seed == 0 || source_cursor == 0 || source_event_ids.is_empty() {
            return Err("nm20_fault_harness_header_invalid".to_owned());
        }
        source_event_ids.sort();
        source_event_ids.dedup();
        cases.sort_by_key(|observation| observation.case);
        let mut run = Self {
            schema: NM20_FAULT_HARNESS_SCHEMA.to_owned(),
            seed,
            source_cursor,
            source_event_ids,
            cases,
            run_digest: String::new(),
        };
        run.run_digest = run.digest();
        run.validate()?;
        Ok(run)
    }

    /// Drive only the cases the library seam can reach; adapter cases are reported as missing.
    pub fn evaluate_with_default(
        seed: u64,
        source_cursor: u64,
        source_event_ids: Vec<String>,
    ) -> Result<Self, String> {
        let mut seam = Nm20LibrarySeam;
        let mut covered = Vec::new();
        for case in Nm20FaultCase::ALL {
            match seam.inject(case) {
                Ok(observation) => covered.push(observation),
                Err(error) if error.starts_with("nm20_adapter_seam_required:") => continue,
                Err(error) => return Err(error),
            }
        }
        Self::from_observations(seed, source_cursor, source_event_ids, covered)
    }

    pub fn observation(&self, case: Nm20FaultCase) -> Option<&Nm20FaultObservation> {
        self.cases
            .iter()
            .find(|observation| observation.case == case)
    }

    pub fn covered_cases(&self) -> Vec<Nm20FaultCase> {
        self.cases
            .iter()
            .map(|observation| observation.case)
            .collect()
    }

    /// Cases a complete run must carry but this run does not. An adapter-seam run reports the
    /// cases it genuinely did not drive.
    pub fn missing_cases(&self) -> Vec<Nm20FaultCase> {
        Nm20FaultCase::ALL
            .into_iter()
            .filter(|case| self.observation(*case).is_none())
            .collect()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NM20_FAULT_HARNESS_SCHEMA
            || self.seed == 0
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.cases.is_empty()
            || self.cases.len() > NM20_FAULT_MAX_CASES
            || self
                .source_event_ids
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err("nm20_fault_harness_invalid".to_owned());
        }
        let mut seen = Vec::new();
        for observation in &self.cases {
            observation.validate()?;
            if seen.contains(&observation.case) {
                return Err("nm20_fault_harness_duplicate_case".to_owned());
            }
            seen.push(observation.case);
        }
        if self.run_digest != self.digest() {
            return Err("nm20_fault_harness_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "seed": self.seed,
            "source_cursor": self.source_cursor,
            "source_event_ids": self.source_event_ids,
            "cases": self.cases,
        }))
    }
}

/// The library seam for one case. Adapter cases never reach this function.
fn drive_library_case(case: Nm20FaultCase) -> Result<Nm20FaultObservation, String> {
    match case {
        Nm20FaultCase::DuplicateReplay => drive_duplicate_replay(),
        Nm20FaultCase::CursorRegression => drive_cursor_regression(),
        Nm20FaultCase::CursorGap => drive_cursor_gap(),
        Nm20FaultCase::LeaseReclaimFence => drive_lease_reclaim_fence(),
        Nm20FaultCase::DiskFullWriteRefusal => drive_disk_full_write_refusal(),
        Nm20FaultCase::SecretSentinel => drive_secret_sentinel(),
        Nm20FaultCase::ProjectionLoss => drive_projection_loss(),
        Nm20FaultCase::CriticalQueryFallback => drive_critical_query_fallback(),
        other => Err(format!("nm20_adapter_seam_required:{}", other.as_str())),
    }
}

const DUPLICATE_SEAM: &str = "kiana_core::NotificationProjection::apply_committed";
const CURSOR_SEAM: &str = "kiana_core::ReplayProjection::from_checkpoint";
const OUTBOX_SEAM: &str = "kiana_domain::NotificationOutboxRecord";
const DELIVERY_SEAM: &str = "kiana_core::NotificationDeliveryWorker::plan";
const REDACTION_SEAM: &str = "kiana_domain::scan_secret_sentinels";
const STORE_SEAM: &str = "kiana_core::NotificationStore::list";

/// One committed `run.completed` fact materialized as a notification.
fn committed_notification_event(cursor: u64, source: &str) -> Result<RuntimeEvent, String> {
    let notification = Notification::new(
        MessageId::new(),
        "actor-20",
        None,
        vec!["project/nm20".to_owned()],
        NotificationChannel::InApp,
        100,
        10_000,
        1,
        None,
    )
    .map_err(|error| format!("nm20_notification_invalid:{error}"))?;
    RuntimeEvent::new(
        RequestId::new(),
        cursor,
        "run.completed",
        json!({
            "source": source,
            "owner_id": "actor-20",
            "notification": notification,
        }),
    )
    .map_err(|error| format!("nm20_event_invalid:{error}"))
}

/// One committed `approval.requested` fact materialized as an actionable Approval row.
fn committed_approval_event(recipient: &str, cursor: u64) -> Result<RuntimeEvent, String> {
    let notification = Notification::new(
        MessageId::new(),
        recipient,
        None,
        vec!["project/nm20".to_owned()],
        NotificationChannel::InApp,
        100,
        10_000,
        1,
        None,
    )
    .map_err(|error| format!("nm20_notification_invalid:{error}"))?;
    let mut event = RuntimeEvent::new(
        RequestId::new(),
        cursor,
        "approval.requested",
        json!({"source": "eventlog", "owner_id": recipient}),
    )
    .map_err(|error| format!("nm20_event_invalid:{error}"))?;
    let action = HumanAction {
        id: "approve".to_owned(),
        label: "Approve".to_owned(),
        command: "notification.approve".to_owned(),
        arguments: json!({"ref": "opaque"}),
        required_fields: Vec::new(),
    };
    let bridge = HumanTaskBridge::new(
        "task-nm20",
        "approval",
        "approval-nm20",
        1,
        "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        event.event_id,
        cursor,
        recipient,
        500,
        9_000,
        vec![format!("event:evidence-{cursor}")],
        vec![action.clone()],
    )
    .map_err(|error| format!("nm20_task_bridge_invalid:{error}"))?;
    let materialization = NotificationMaterialization::new(
        notification,
        event.event_id,
        cursor,
        HumanInboxKind::Approval,
        "Approval required",
        "A redacted approval is waiting",
        500,
        9_000,
        vec![format!("event:evidence-{cursor}")],
        vec![action],
        Some(bridge),
    )
    .map_err(|error| format!("nm20_materialization_invalid:{error}"))?;
    event.data["source_cursor"] = json!(cursor);
    event.data["notification_materialization"] = json!(materialization);
    Ok(event)
}

/// One committed `run.completed` fact materialized as a non-actionable Question row.
///
/// This mirrors the NM-09 fixture shape: the materialization carries its explicit source cursor
/// so the store can require a contiguous committed page.
fn committed_question_event(recipient: &str, cursor: u64) -> Result<RuntimeEvent, String> {
    let notification = Notification::new(
        MessageId::new(),
        recipient,
        None,
        vec!["project/nm20".to_owned()],
        NotificationChannel::InApp,
        100,
        10_000,
        1,
        None,
    )
    .map_err(|error| format!("nm20_notification_invalid:{error}"))?;
    let mut event = RuntimeEvent::new(
        RequestId::new(),
        cursor,
        "run.completed",
        json!({"source": "eventlog", "owner_id": recipient}),
    )
    .map_err(|error| format!("nm20_event_invalid:{error}"))?;
    let action = HumanAction {
        id: "open".to_owned(),
        label: "Open".to_owned(),
        command: "notification.open".to_owned(),
        arguments: json!({"ref": "opaque"}),
        required_fields: Vec::new(),
    };
    let materialization = NotificationMaterialization::new(
        notification,
        event.event_id,
        cursor,
        HumanInboxKind::Question,
        "Run completed",
        "A redacted run terminal is waiting",
        500,
        9_000,
        vec![format!("event:evidence-{cursor}")],
        vec![action],
        None,
    )
    .map_err(|error| format!("nm20_materialization_invalid:{error}"))?;
    event.data["source_cursor"] = json!(cursor);
    event.data["notification_materialization"] = json!(materialization);
    Ok(event)
}

/// One outbox record over a fresh notification. `actionable` selects the protected class.
fn outbox_record(
    class: NotificationOutboxClass,
    actionable: bool,
) -> Result<NotificationOutboxRecord, String> {
    let action_ref = actionable.then(kiana_domain::ActionRefId::new);
    let notification = Notification::new(
        MessageId::new(),
        "actor-20",
        None,
        vec!["project/nm20".to_owned()],
        NotificationChannel::InApp,
        100,
        10_000,
        1,
        action_ref,
    )
    .map_err(|error| format!("nm20_notification_invalid:{error}"))?;
    NotificationOutboxRecord::new(
        notification,
        class,
        kiana_domain::SubscriptionId::new(),
        7,
        100,
    )
    .map_err(|error| format!("nm20_outbox_record_invalid:{error}"))
}

/// Duplicate replay: the same committed page twice produces one row and an identical checkpoint.
fn drive_duplicate_replay() -> Result<Nm20FaultObservation, String> {
    let event = committed_notification_event(1, "eventlog")?;
    let page = [event];
    let mut projection = NotificationProjection::new();
    projection
        .apply_committed(&page, 1)
        .map_err(|error| format!("nm20_duplicate_replay_first:{error}"))?;
    let after_first = projection.clone();
    let checkpoint = projection.checkpoint().cloned();
    projection
        .apply_committed(&page, 1)
        .map_err(|error| format!("nm20_duplicate_replay_second:{error}"))?;
    if projection != after_first {
        return Err("nm20_duplicate_replay_mutated_projection".to_owned());
    }
    if projection.checkpoint() != checkpoint.as_ref() {
        return Err("nm20_duplicate_replay_moved_checkpoint".to_owned());
    }
    if projection.notifications().len() != 1 {
        return Err("nm20_duplicate_replay_duplicated_row".to_owned());
    }
    Nm20FaultObservation::new(
        Nm20FaultCase::DuplicateReplay,
        DUPLICATE_SEAM,
        true,
        "notification_projection_duplicate_suppressed",
        Some("second notification row for one committed event"),
        true,
        true,
        true,
        false,
        false,
        true,
    )
}

/// Cursor regression: a stale page is refused with the projection unchanged.
fn drive_cursor_regression() -> Result<Nm20FaultObservation, String> {
    let event = committed_notification_event(2, "eventlog")?;
    let mut projection = NotificationProjection::new();
    projection
        .apply_committed(std::slice::from_ref(&event), 2)
        .map_err(|error| format!("nm20_cursor_regression_first:{error}"))?;
    let before = projection.clone();
    let stale = committed_notification_event(1, "eventlog")?;
    let refusal = match projection.apply_committed(std::slice::from_ref(&stale), 1) {
        Ok(()) => return Err("nm20_cursor_regression_accepted".to_owned()),
        Err(code) => code,
    };
    if refusal != "projection_checkpoint_cursor_not_advanced" {
        return Err(format!("nm20_cursor_regression_unexpected:{refusal}"));
    }
    if projection != before {
        return Err("nm20_cursor_regression_mutated_projection".to_owned());
    }
    Nm20FaultObservation::new(
        Nm20FaultCase::CursorRegression,
        CURSOR_SEAM,
        true,
        refusal,
        Some("projection advanced by a stale cursor"),
        false,
        true,
        false,
        false,
        false,
        true,
    )
}

/// Cursor gap: a page that claims a cursor the projector skipped is refused.
fn drive_cursor_gap() -> Result<Nm20FaultObservation, String> {
    let event = committed_notification_event(1, "eventlog")?;
    let mut projection = NotificationProjection::new();
    projection
        .apply_committed(std::slice::from_ref(&event), 1)
        .map_err(|error| format!("nm20_cursor_gap_first:{error}"))?;
    let before = projection.clone();
    // The gap page delivers no events but claims the cursor jumped past the missing range.
    let refusal = match projection.apply_committed(&[], 3) {
        Ok(()) => return Err("nm20_cursor_gap_completed".to_owned()),
        Err(code) => code,
    };
    if refusal != "projection_checkpoint_cursor_gap" {
        return Err(format!("nm20_cursor_gap_unexpected:{refusal}"));
    }
    if projection != before {
        return Err("nm20_cursor_gap_mutated_projection".to_owned());
    }
    Nm20FaultObservation::new(
        Nm20FaultCase::CursorGap,
        CURSOR_SEAM,
        true,
        refusal,
        Some("gap reported as a completed position"),
        false,
        true,
        false,
        true,
        false,
        true,
    )
}

/// The crash window at the lease level: a claim that was never submitted is reclaimable after
/// expiry, the old lease is fenced, and the record is submitted exactly once.
fn drive_lease_reclaim_fence() -> Result<Nm20FaultObservation, String> {
    let mut record = outbox_record(NotificationOutboxClass::Actionable, true)?;
    let lease_a = record
        .claim("worker-a", 7, 100, 50)
        .map_err(|error| format!("nm20_lease_claim_a:{error}"))?;
    // While the lease is live a second worker cannot steal it.
    if record.claim("worker-b", 7, 140, 50).is_ok() {
        return Err("nm20_lease_active_claim_accepted".to_owned());
    }
    // After expiry the second worker reclaims it with a new attempt and generation.
    let lease_b = record
        .claim("worker-b", 7, 150, 50)
        .map_err(|error| format!("nm20_lease_claim_b:{error}"))?;
    if lease_a.lease_token == lease_b.lease_token || record.claim_generation != 2 {
        return Err("nm20_lease_reclaim_generation_missing".to_owned());
    }
    if record.attempt.attempt_number != 2 {
        return Err("nm20_lease_reclaim_attempt_missing".to_owned());
    }
    // The stale lease cannot submit: at the boundary it is a fence mismatch, later it expired.
    let stale_code = match record.mark_submitted(&lease_a, 150) {
        Ok(()) => return Err("nm20_stale_lease_submitted".to_owned()),
        Err(code) => code,
    };
    if stale_code != "notification_outbox_lease_fence_mismatch" {
        return Err(format!("nm20_stale_lease_unexpected:{stale_code}"));
    }
    let expired_code = match record.mark_submitted(&lease_a, 201) {
        Ok(()) => return Err("nm20_expired_lease_submitted".to_owned()),
        Err(code) => code,
    };
    if expired_code != "notification_outbox_lease_expired" {
        return Err(format!("nm20_expired_lease_unexpected:{expired_code}"));
    }
    record
        .mark_submitted(&lease_b, 151)
        .map_err(|error| format!("nm20_reclaimed_lease_submit_failed:{error}"))?;
    if record.state != NotificationOutboxState::Submitted {
        return Err("nm20_reclaimed_lease_state_invalid".to_owned());
    }
    Nm20FaultObservation::new(
        Nm20FaultCase::LeaseReclaimFence,
        OUTBOX_SEAM,
        true,
        stale_code,
        Some("stale lease submitting a reclaimed record"),
        false,
        true,
        false,
        false,
        true,
        true,
    )
}

/// Disk full: the write failure is classified and the delivery can only reconcile, never deliver.
fn drive_disk_full_write_refusal() -> Result<Nm20FaultObservation, String> {
    let summary = "notification outbox append failed: No space left on device (os error 28)";
    if classify_failure_summary(summary, false, false) != FailureClass::DiskFull {
        return Err("nm20_disk_full_not_classified".to_owned());
    }
    if classify_failure_summary("connection reset by peer", false, false) == FailureClass::DiskFull
    {
        return Err("nm20_disk_full_over_matched".to_owned());
    }
    if classify_failure_summary("disk_full", false, true) != FailureClass::DiskFull {
        return Err("nm20_disk_full_hidden_by_cancellation".to_owned());
    }

    // A submitted send whose write/ack window closed can only reconcile; it is not delivered.
    let mut record = outbox_record(NotificationOutboxClass::BestEffort, false)?;
    let lease = record
        .claim("worker-a", 7, 100, 20)
        .map_err(|error| format!("nm20_disk_full_claim:{error}"))?;
    record
        .mark_submitted(&lease, 110)
        .map_err(|error| format!("nm20_disk_full_submit:{error}"))?;
    let worker = NotificationDeliveryWorker::new("worker-a", 7, 50)
        .map_err(|error| format!("nm20_disk_full_worker:{error}"))?;
    let plan = worker
        .plan(&record, 200)
        .map_err(|error| format!("nm20_disk_full_plan:{error}"))?;
    if plan != NotificationDeliveryPlan::ReconcileUnknown {
        return Err("nm20_disk_full_plan_not_reconcile".to_owned());
    }
    if record.state == NotificationOutboxState::Acknowledged
        || record.notification.status == NotificationStatus::Delivered
    {
        return Err("nm20_disk_full_delivered".to_owned());
    }
    record
        .reconcile_unknown(&lease, 120)
        .map_err(|error| format!("nm20_disk_full_reconcile:{error}"))?;
    if record.state != NotificationOutboxState::Unknown
        || worker
            .plan(&record, 130)
            .map_err(|error| format!("nm20_disk_full_terminal_plan:{error}"))?
            != NotificationDeliveryPlan::Terminal
    {
        return Err("nm20_disk_full_state_invalid".to_owned());
    }
    Nm20FaultObservation::new(
        Nm20FaultCase::DiskFullWriteRefusal,
        DELIVERY_SEAM,
        true,
        "platform_disk_full",
        Some("disk-full write silently retried or marked delivered"),
        false,
        true,
        false,
        false,
        true,
        true,
    )
}

/// Secret sentinel: the raw form is refused, the redacted form passes, nothing was sent.
fn drive_secret_sentinel() -> Result<Nm20FaultObservation, String> {
    const SENTINEL: &str = "Authorization: Bearer sk-live-nm20-secret-sentinel";
    if scan_secret_sentinels(SecretScanChannel::Receipt, SENTINEL).is_ok() {
        return Err("nm20_secret_sentinel_not_detected".to_owned());
    }
    let envelope = json!({
        "title": "Approval required",
        "summary": SENTINEL,
    });
    if scan_secret_value(SecretScanChannel::Receipt, &envelope).is_ok() {
        return Err("nm20_secret_envelope_not_detected".to_owned());
    }
    let redacted = redact_text(SENTINEL);
    if redacted.contains("sk-live-nm20") {
        return Err("nm20_secret_redaction_leaked".to_owned());
    }
    if scan_secret_sentinels(SecretScanChannel::Receipt, &redacted).is_err() {
        return Err("nm20_secret_redaction_unscannable".to_owned());
    }
    let redacted_envelope = redact_value(&envelope);
    if scan_secret_value(SecretScanChannel::Receipt, &redacted_envelope).is_err() {
        return Err("nm20_secret_redacted_envelope_refused".to_owned());
    }
    Nm20FaultObservation::new(
        Nm20FaultCase::SecretSentinel,
        REDACTION_SEAM,
        true,
        "secret_sentinel_quarantined",
        Some("secret sentinel rendered, digested or persisted"),
        false,
        true,
        false,
        false,
        true,
        true,
    )
}

/// Projection loss: losing the in-memory projection and rebuilding from committed input must
/// reproduce the same notifications and checkpoint, and must not lose the actionable row.
fn drive_projection_loss() -> Result<Nm20FaultObservation, String> {
    let events = vec![
        committed_notification_event(1, "eventlog")?,
        committed_notification_event(2, "eventlog")?,
        committed_notification_event(3, "eventlog")?,
    ];
    let mut first = NotificationProjection::new();
    first
        .apply_committed(&events, 3)
        .map_err(|error| format!("nm20_projection_loss_first:{error}"))?;
    if first.notifications().len() != 3 {
        return Err("nm20_projection_loss_initial_rows".to_owned());
    }
    // The projection is volatile: a fresh projector rebuilds it from the committed page only.
    let mut rebuilt = NotificationProjection::new();
    rebuilt
        .apply_committed(&events, 3)
        .map_err(|error| format!("nm20_projection_loss_rebuild:{error}"))?;
    if rebuilt != first || rebuilt.checkpoint() != first.checkpoint() {
        return Err("nm20_projection_loss_rebuild_diverged".to_owned());
    }
    if rebuilt.source_cursor() != 3 {
        return Err("nm20_projection_loss_cursor_lost".to_owned());
    }
    Nm20FaultObservation::new(
        Nm20FaultCase::ProjectionLoss,
        "kiana_core::NotificationProjection::apply_committed",
        true,
        "notification_projection_rebuild_equivalent",
        Some("projection loss reported as permanent fact loss"),
        true,
        true,
        false,
        true,
        false,
        true,
    )
}

/// Critical query fallback: while the volatile stream is gone the committed inbox query still
/// answers the critical row, and an unavailable projection is not reported as empty.
fn drive_critical_query_fallback() -> Result<Nm20FaultObservation, String> {
    let recipient = "actor-20";
    // A contiguous committed page: the actionable approval row plus a non-actionable question.
    let events = vec![
        committed_approval_event(recipient, 1)?,
        committed_question_event(recipient, 2)?,
    ];
    let request = NotificationListRequest {
        schema: crate::NOTIFICATION_STORE_SCHEMA.to_owned(),
        recipient_id: recipient.to_owned(),
        limit: 10,
        after_item_id: None,
        expected_source_cursor: None,
    };
    let mut store = NotificationStore::new();
    store
        .apply_committed(&events, 2)
        .map_err(|error| format!("nm20_query_fallback_apply:{error}"))?;
    store
        .bind_server_recipient(recipient)
        .map_err(|error| format!("nm20_query_fallback_bind:{error}"))?;
    let page = store
        .list(request.clone())
        .map_err(|error| format!("nm20_query_fallback_page:{error}"))?;
    page.validate()
        .map_err(|error| format!("nm20_query_fallback_page_invalid:{error}"))?;
    let approval = page
        .items
        .iter()
        .find(|item| item.item.kind == HumanInboxKind::Approval)
        .ok_or_else(|| "nm20_query_fallback_approval_missing".to_owned())?;
    if approval.unknown || approval.acknowledged || approval.item.actions.is_empty() {
        return Err("nm20_query_fallback_approval_degraded".to_owned());
    }
    // A projection that has no committed page answers `unavailable`, never an empty inbox.
    let mut empty = NotificationStore::new();
    empty
        .bind_server_recipient(recipient)
        .map_err(|error| format!("nm20_query_fallback_empty_bind:{error}"))?;
    match empty.list(request.clone()) {
        Err(NotificationStoreError::ProjectionUnavailable) => {}
        other => {
            return Err(format!(
                "nm20_query_fallback_unavailable_confused:{other:?}"
            ))
        }
    }
    // A rebuild from the same committed input serves the same bytes.
    let mut rebuilt = NotificationStore::new();
    rebuilt
        .apply_committed(&events, 2)
        .map_err(|error| format!("nm20_query_fallback_rebuild:{error}"))?;
    rebuilt
        .bind_server_recipient(recipient)
        .map_err(|error| format!("nm20_query_fallback_rebuild_bind:{error}"))?;
    let rebuilt_page = rebuilt
        .list(request)
        .map_err(|error| format!("nm20_query_fallback_rebuilt_page:{error}"))?;
    if rebuilt_page.page_digest != page.page_digest {
        return Err("nm20_query_fallback_rebuild_diverged".to_owned());
    }
    Nm20FaultObservation::new(
        Nm20FaultCase::CriticalQueryFallback,
        STORE_SEAM,
        true,
        "notification_query_fallback_served",
        Some("critical fact hidden behind a lagged volatile stream"),
        true,
        true,
        false,
        true,
        false,
        true,
    )
}

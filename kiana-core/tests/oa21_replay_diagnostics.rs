//! OA-21 fixtures: read-only replay diagnostics and audit/receipt/health consistency.
//!
//! Deny-first: duplicate sources, unknown audit schemas, swallowed expectations and metric/trace
//! write-back are refused before the success path compares deterministic projectors against an
//! explicit consistency baseline.

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::{
    diagnose_replay, diagnose_replay_with_baseline, ControlPlane, ReplayDiagnosticsError,
    ReplayExpectation,
};
use kiana_domain::{
    ApprovalChallenge, ApprovalId, CapabilityRequest, EventId, InvocationId, PendingApproval,
    ReplayConsistencyBaseline, ReplayDivergenceKind, RequestContext, RequestId, RunId,
    RuntimeEvent, TraceStatus,
};
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, EventStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const REPLAY_CONSISTENCY_FIXTURE: &str = include_str!("fixtures/oa21-replay-consistency.json");

fn digest(byte: char) -> String {
    format!(
        "sha256:{}",
        std::iter::repeat(byte).take(64).collect::<String>()
    )
}

fn event(
    run_id: RunId,
    request_id: RequestId,
    sequence: u64,
    kind: &str,
    data: serde_json::Value,
) -> RuntimeEvent {
    RuntimeEvent::new(request_id, sequence, kind, data)
        .unwrap()
        .with_stream_metadata("run", run_id.to_string(), sequence)
}

fn unknown_run() -> (RunId, RequestId, Vec<RuntimeEvent>) {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let digest = digest('a');
    let events = vec![
        event(
            run_id,
            request_id,
            1,
            "run.authorized",
            json!({"run_id":run_id,"session_id":"session-1","actor_id":"local-user","project_root":"/repo","authority_epoch":1,"data_epoch":1,"source_cursor":1}),
        ),
        event(
            run_id,
            request_id,
            2,
            "run.capability_requested",
            json!({"run_id":run_id,"capability_request_id":request_id,"capability":"process","operation":"shell.exec","risk":"local_write","arguments":{"command":"raw-secret"},"action_digest":digest,"attempt":1,"authority_epoch":1,"data_epoch":1,"source_cursor":2}),
        ),
        event(
            run_id,
            request_id,
            3,
            "capability.decision",
            json!({"run_id":run_id,"capability_request_id":request_id,"attempt":1,"action_digest":digest,"gate":{"decision":"allowed"},"authority_epoch":1,"data_epoch":1,"source_cursor":3}),
        ),
        event(
            run_id,
            request_id,
            4,
            "invocation.dispatching",
            json!({"run_id":run_id,"capability_request_id":request_id,"attempt":1,"source_cursor":4}),
        ),
        event(
            run_id,
            request_id,
            5,
            "invocation.executing",
            json!({"run_id":run_id,"capability_request_id":request_id,"attempt":1,"effect_started":true,"source_cursor":5}),
        ),
        event(
            run_id,
            request_id,
            6,
            "execution.result_committed",
            json!({"run_id":run_id,"capability_request_id":request_id,"attempt":1,"effect_known":false,"error":"result_unknown:provider_timeout","result":{"success":false,"output":{"raw":"raw-secret"}},"source_cursor":6}),
        ),
        event(
            run_id,
            request_id,
            7,
            "audit.forged",
            json!({"run_id":run_id,"authority_epoch":1,"data_epoch":1,"actor_ref":"model","source_cursor":7}),
        ),
    ];
    (run_id, request_id, events)
}

/// A run whose deterministic audit, receipt, health and invocation projectors can all agree.
fn consistent_run() -> (RunId, RequestId, Vec<RuntimeEvent>) {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let action_digest = digest('a');
    let events = vec![
        event(
            run_id,
            request_id,
            1,
            "run.authorized",
            json!({"run_id":run_id,"session_id":"session-1","actor_id":"local-user","project_root":"/repo","authority_epoch":1,"data_epoch":1,"source_cursor":1}),
        ),
        event(
            run_id,
            request_id,
            2,
            "run.capability_requested",
            json!({"run_id":run_id,"capability_request_id":request_id,"capability":"process","operation":"shell.exec","risk":"local_write","arguments":{"command":"raw-secret"},"action_digest":action_digest,"attempt":1,"authority_epoch":1,"data_epoch":1,"source_cursor":2}),
        ),
        event(
            run_id,
            request_id,
            3,
            "capability.decision",
            json!({"run_id":run_id,"capability_request_id":request_id,"attempt":1,"action_digest":action_digest,"gate":{"decision":"allowed"},"authority_epoch":1,"data_epoch":1,"source_cursor":3}),
        ),
        event(
            run_id,
            request_id,
            4,
            "invocation.dispatching",
            json!({"run_id":run_id,"capability_request_id":request_id,"attempt":1,"source_cursor":4}),
        ),
        event(
            run_id,
            request_id,
            5,
            "invocation.executing",
            json!({"run_id":run_id,"capability_request_id":request_id,"attempt":1,"effect_started":true,"source_cursor":5}),
        ),
        event(
            run_id,
            request_id,
            6,
            "execution.result_committed",
            json!({"run_id":run_id,"capability_request_id":request_id,"attempt":1,"effect_known":true,"result":{"success":true,"output":{"summary":"ok"}},"source_cursor":6}),
        ),
    ];
    (run_id, request_id, events)
}

#[test]
fn replay_diagnostics_are_deterministic_safe_and_unknown_effect_is_locatable() {
    let (run_id, request_id, events) = unknown_run();
    let first = diagnose_replay(&events, Some(run_id), &[]).unwrap();
    let second = diagnose_replay(&events, Some(run_id), &[]).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.status, TraceStatus::Unknown);
    assert!(!first.diagnostics.is_empty());
    assert!(first
        .diagnostics
        .iter()
        .any(|diagnostic| { diagnostic.divergence == ReplayDivergenceKind::UnknownEffect }));
    assert!(first.diagnostics.iter().all(|diagnostic| {
        diagnostic
            .error_code
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".-_".contains(character))
    }));
    let encoded = serde_json::to_string(&first).unwrap();
    assert!(!encoded.contains("raw-secret"));
    first.validate().unwrap();

    // The unknown effect stays bound to its invocation, so the first divergence is locatable by
    // `(invocation_id, attempt, input_digest, status, error_code)`.
    let unknown = first
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.divergence == ReplayDivergenceKind::UnknownEffect)
        .unwrap();
    assert_eq!(
        unknown.invocation_id,
        Some(InvocationId::from_uuid(request_id.as_uuid()))
    );
    assert_eq!(
        unknown.observed_error_code.as_deref(),
        Some("result_unknown")
    );
    let first_divergence = first.first_divergence().unwrap();
    assert_eq!(Some(first_divergence), first.diagnostics.first());
    assert!(!first.is_consistent());

    let expectation = ReplayExpectation {
        invocation_id: Some(InvocationId::from_uuid(request_id.as_uuid())),
        attempt: Some(1),
        input_digest: Some(digest('b')),
        expected_status: TraceStatus::Ok,
        expected_error_code: None,
    };
    let expected = diagnose_replay(&events[..6], Some(run_id), &[expectation]).unwrap();
    assert!(expected
        .diagnostics
        .iter()
        .any(|diagnostic| { diagnostic.divergence == ReplayDivergenceKind::StatusMismatch }));
}

#[test]
fn duplicate_source_is_rejected_before_any_projection_or_side_effect() {
    let (run_id, _request_id, mut events) = unknown_run();
    let duplicate = events[0].clone();
    events.push(duplicate);
    assert_eq!(
        diagnose_replay(&events, Some(run_id), &[]).unwrap_err(),
        ReplayDiagnosticsError::SourceDuplicate
    );
}

#[test]
fn replay_expectation_limit_is_bounded() {
    let run_id = RunId::new();
    let events = vec![event(
        run_id,
        RequestId::new(),
        1,
        "run.authorized",
        json!({"run_id":run_id,"authority_epoch":1,"data_epoch":1,"source_cursor":1}),
    )];
    let expectations = (0..257)
        .map(|_| ReplayExpectation {
            invocation_id: None,
            attempt: None,
            input_digest: None,
            expected_status: TraceStatus::Unknown,
            expected_error_code: None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        diagnose_replay(&events, None, &expectations).unwrap_err(),
        ReplayDiagnosticsError::ExpectationLimit
    );
}

#[test]
fn undeclared_run_expectations_and_unmatched_invocations_are_refused() {
    let (run_id, _request_id, events) = unknown_run();
    let stray = ReplayExpectation {
        invocation_id: None,
        attempt: None,
        input_digest: None,
        expected_status: TraceStatus::Ok,
        expected_error_code: None,
    };
    // Expectations cannot be silently dropped just because no run was supplied.
    assert_eq!(
        diagnose_replay(&events, None, std::slice::from_ref(&stray)).unwrap_err(),
        ReplayDiagnosticsError::ExpectationRunRequired
    );
    assert_eq!(
        ReplayDiagnosticsError::ExpectationRunRequired.to_string(),
        "replay_expectation_run_required"
    );

    // An expectation that matches no invocation is a divergence, not a missing line item.
    let missing_id = InvocationId::new();
    let missing = ReplayExpectation {
        invocation_id: Some(missing_id),
        attempt: Some(3),
        input_digest: Some(digest('d')),
        expected_status: TraceStatus::Ok,
        expected_error_code: Some("provider_timeout".to_owned()),
    };
    let snapshot = diagnose_replay(&events, Some(run_id), &[missing]).unwrap();
    let diagnostic = snapshot
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.error_code == "replay_expectation_invocation_missing")
        .expect("missing invocation must stay visible");
    assert_eq!(diagnostic.invocation_id, Some(missing_id));
    assert_eq!(diagnostic.attempt, Some(3));
    assert_eq!(
        diagnostic.input_digest.as_deref(),
        Some(digest('d').as_str())
    );
    assert_eq!(
        diagnostic.expected_error_code.as_deref(),
        Some("provider_timeout")
    );
    assert_eq!(diagnostic.observed_error_code, None);
    assert_eq!(diagnostic.observed_status, None);
    assert_eq!(diagnostic.expected_status, Some(TraceStatus::Ok));
    assert_eq!(diagnostic.divergence, ReplayDivergenceKind::StatusMismatch);
    assert_eq!(snapshot.first_divergence(), Some(diagnostic));

    // Diagnostics are kept in canonical locator order; a reordered snapshot is refused.
    for pair in snapshot.diagnostics.windows(2) {
        assert!(pair[0].locator_key() <= pair[1].locator_key());
    }
    let mut forged = snapshot.clone();
    forged.diagnostics.reverse();
    let error = forged.validate().unwrap_err();
    assert!(
        error == "replay_diagnostic_order_invalid" || error == "replay_diagnostic_digest_mismatch",
        "unexpected forged snapshot error: {error}"
    );
}

#[test]
fn expectation_error_codes_are_compared_without_collapsing_into_status() {
    let (run_id, request_id, events) = unknown_run();
    let invocation_id = InvocationId::from_uuid(request_id.as_uuid());
    let wrong_code = ReplayExpectation {
        invocation_id: Some(invocation_id),
        attempt: Some(1),
        input_digest: Some(digest('a')),
        expected_status: TraceStatus::Unknown,
        expected_error_code: Some("provider_timeout".to_owned()),
    };
    let snapshot = diagnose_replay(&events, Some(run_id), &[wrong_code]).unwrap();
    let mismatch = snapshot
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.error_code == "replay_error_code_mismatch")
        .expect("an error code mismatch must not be swallowed");
    assert_eq!(mismatch.invocation_id, Some(invocation_id));
    assert_eq!(mismatch.attempt, Some(1));
    assert_eq!(mismatch.input_digest.as_deref(), Some(digest('a').as_str()));
    assert_eq!(
        mismatch.expected_error_code.as_deref(),
        Some("provider_timeout")
    );
    assert_eq!(
        mismatch.observed_error_code.as_deref(),
        Some("result_unknown")
    );
    assert_eq!(mismatch.divergence, ReplayDivergenceKind::StatusMismatch);

    // The same attempt with matching status, input digest and error code adds no expectation
    // divergence; the unknown effect itself remains visible.
    let matching = ReplayExpectation {
        invocation_id: Some(invocation_id),
        attempt: Some(1),
        input_digest: Some(digest('a')),
        expected_status: TraceStatus::Unknown,
        expected_error_code: Some("result_unknown".to_owned()),
    };
    let agreed = diagnose_replay(&events[..6], Some(run_id), &[matching]).unwrap();
    assert!(!agreed.is_consistent());
    assert!(agreed
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.divergence == ReplayDivergenceKind::UnknownEffect));
}

#[test]
fn unknown_audit_schema_is_refused_instead_of_guessed() {
    let (run_id, _request_id, events) = unknown_run();
    let snapshot = diagnose_replay(&events, Some(run_id), &[]).unwrap();
    let schema = snapshot
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.divergence == ReplayDivergenceKind::SchemaConflict)
        .expect("an untrusted audit schema must be reported, not guessed");
    assert_eq!(schema.error_code, "audit_projection_reduce_failed");
    let encoded = serde_json::to_string(&snapshot).unwrap();
    assert!(!encoded.contains("audit.forged"));
    assert!(!encoded.contains("raw-secret"));
    assert!(!encoded.contains("actor_ref"));
    snapshot.validate().unwrap();
}

/// Read-only EventStore fixture: every append is counted and refused.
struct RecordingReadOnlyStore {
    events: Vec<RuntimeEvent>,
    appends: AtomicUsize,
}

impl RecordingReadOnlyStore {
    fn new(events: Vec<RuntimeEvent>) -> Self {
        Self {
            events,
            appends: AtomicUsize::new(0),
        }
    }

    fn appends(&self) -> usize {
        self.appends.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl EventStorePort for RecordingReadOnlyStore {
    async fn append(&self, _event: RuntimeEvent) -> Result<(), PortError> {
        self.appends.fetch_add(1, Ordering::SeqCst);
        Err(PortError::Failed("oa21_fixture_read_only".to_owned()))
    }

    async fn read_request(&self, request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(self
            .events
            .iter()
            .filter(|event| event.request_id == *request_id)
            .cloned()
            .collect())
    }

    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(self.events.clone())
    }
}

/// A store without read-all support must surface "unavailable", never an empty success.
struct UnsupportedReadAllStore;

#[async_trait]
impl EventStorePort for UnsupportedReadAllStore {
    async fn append(&self, _event: RuntimeEvent) -> Result<(), PortError> {
        Err(PortError::Failed("oa21_fixture_read_only".to_owned()))
    }

    async fn read_request(&self, _request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(Vec::new())
    }
}

struct UnusedApprovalStore;

#[async_trait]
impl ApprovalStorePort for UnusedApprovalStore {
    /// Replay never stages an approval; being called at all must fail closed.
    async fn stage(
        &self,
        _context: &RequestContext,
        _request: CapabilityRequest,
        _reason: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        Err(PortError::Unavailable(
            "oa21_fixture_approval_unused".to_owned(),
        ))
    }

    /// Replay never consumes an approval; being called at all must fail closed.
    async fn activate(&self, _approval_id: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable(
            "oa21_fixture_approval_unused".to_owned(),
        ))
    }

    /// Replay never consumes an approval; being called at all must fail closed.
    async fn consume(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable(
            "oa21_fixture_approval_unused".to_owned(),
        ))
    }
}

struct UnusedRunner;

#[async_trait]
impl RunnerPort for UnusedRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Unavailable(
            "oa21_fixture_runner_unused".to_owned(),
        ))
    }
}

fn replay_core(store: Arc<dyn EventStorePort>) -> ControlPlane {
    ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        store,
        Arc::new(CapabilityBroker::new()),
        Arc::new(UnusedApprovalStore),
        Arc::new(UnusedRunner),
    )
}

#[test]
fn baseline_pins_audit_receipt_and_health_projectors() {
    let (run_id, _request_id, events) = consistent_run();
    let plain = diagnose_replay(&events, Some(run_id), &[]).unwrap();
    assert!(
        plain.is_consistent(),
        "unexpected replay diagnostics: {:?}",
        plain.diagnostics
    );
    assert_eq!(plain.baseline_digest, None);
    for projection in [
        "audit",
        "receipt",
        "health",
        "metrics",
        "run",
        "invocations",
        "spans",
    ] {
        assert!(
            plain.projection_digests.contains_key(projection),
            "projection digest missing: {projection}"
        );
    }
    assert!(plain
        .limitations
        .iter()
        .any(|limitation| limitation == "replay_projection_only_read_only"));
    assert!(plain
        .limitations
        .iter()
        .any(|limitation| limitation == "replay_baseline_not_provided"));

    // Success path: an explicit baseline built from the same committed source agrees, and the
    // snapshot records which baseline the verdict belongs to.
    let baseline = ReplayConsistencyBaseline::new(
        plain.source_event_ids.clone(),
        plain.projection_digests.clone(),
        Vec::new(),
    )
    .unwrap();
    baseline.validate().unwrap();
    assert_eq!(
        baseline.expected_digest("audit"),
        plain.projection_digests.get("audit").map(String::as_str)
    );
    let agreed =
        diagnose_replay_with_baseline(&events, Some(run_id), &[], Some(&baseline)).unwrap();
    assert!(
        agreed.is_consistent(),
        "unexpected baseline diagnostics: {:?}",
        agreed.diagnostics
    );
    assert_eq!(
        agreed.baseline_digest,
        Some(baseline.baseline_digest.clone())
    );
    assert_eq!(agreed.projection_digests, plain.projection_digests);
    assert_eq!(agreed.source_event_ids, plain.source_event_ids);
    assert!(agreed
        .limitations
        .iter()
        .all(|limitation| limitation != "replay_baseline_not_provided"));
    let repeated =
        diagnose_replay_with_baseline(&events, Some(run_id), &[], Some(&baseline)).unwrap();
    assert_eq!(agreed.snapshot_digest, repeated.snapshot_digest);

    // A tampered projector digest becomes a named first divergence, never a skipped comparison.
    let fixture: serde_json::Value = serde_json::from_str(REPLAY_CONSISTENCY_FIXTURE).unwrap();
    let tamper_projection = fixture["tamper"]["projection"].as_str().unwrap();
    let mut tampered_digests = baseline.projection_digests.clone();
    tampered_digests.insert(tamper_projection.to_owned(), digest('c'));
    let tampered = ReplayConsistencyBaseline::new(
        plain.source_event_ids.clone(),
        tampered_digests,
        Vec::new(),
    )
    .unwrap();
    let diverged =
        diagnose_replay_with_baseline(&events, Some(run_id), &[], Some(&tampered)).unwrap();
    assert!(!diverged.is_consistent());
    let first = diverged.first_divergence().expect("first divergence");
    assert_eq!(first.divergence, ReplayDivergenceKind::ProjectionMismatch);
    assert_eq!(
        first.error_code,
        fixture["tamper"]["expected_error_code"].as_str().unwrap()
    );
    assert_eq!(diverged.diagnostics.first(), Some(first));
    assert_eq!(
        diverged.baseline_digest,
        Some(tampered.baseline_digest.clone())
    );

    // A baseline pinned to a foreign source is refused before any digest verdict.
    let foreign = ReplayConsistencyBaseline::new(
        vec![EventId::new()],
        baseline.projection_digests.clone(),
        Vec::new(),
    )
    .unwrap();
    let source_mismatch =
        diagnose_replay_with_baseline(&events, Some(run_id), &[], Some(&foreign)).unwrap();
    assert_eq!(
        source_mismatch
            .first_divergence()
            .map(|diagnostic| diagnostic.error_code.as_str()),
        Some("replay_baseline_source_mismatch")
    );

    // A projection the replay could not reconstruct is unavailable, not silently skipped.
    let receipt_only = ReplayConsistencyBaseline::new(
        plain.source_event_ids.clone(),
        BTreeMap::from([(
            "receipt".to_owned(),
            plain.projection_digests["receipt"].clone(),
        )]),
        Vec::new(),
    )
    .unwrap();
    let unavailable =
        diagnose_replay_with_baseline(&events, None, &[], Some(&receipt_only)).unwrap();
    assert!(unavailable
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.error_code == "replay_receipt_digest_unavailable"));

    // Domain refusals: unknown projector, empty baseline, bad digest, duplicate source.
    assert_eq!(
        ReplayConsistencyBaseline::new(plain.source_event_ids.clone(), BTreeMap::new(), Vec::new())
            .unwrap_err(),
        "replay_baseline_projection_required"
    );
    let unknown_projection = ReplayConsistencyBaseline::new(
        plain.source_event_ids.clone(),
        BTreeMap::from([("telemetry".to_owned(), digest('b'))]),
        Vec::new(),
    )
    .unwrap_err();
    assert!(unknown_projection.starts_with("replay_baseline_projection_unknown"));
    assert_eq!(
        ReplayConsistencyBaseline::new(
            plain.source_event_ids.clone(),
            BTreeMap::from([("audit".to_owned(), "not-a-digest".to_owned())]),
            Vec::new(),
        )
        .unwrap_err(),
        "replay_baseline_projection_digest_invalid"
    );
    assert_eq!(
        ReplayConsistencyBaseline::new(
            vec![plain.source_event_ids[0], plain.source_event_ids[0]],
            plain.projection_digests.clone(),
            Vec::new(),
        )
        .unwrap_err(),
        "replay_baseline_source_event_ids_duplicate"
    );
    let mut stale = baseline.clone();
    stale
        .projection_digests
        .insert("audit".to_owned(), digest('d'));
    assert_eq!(
        stale.validate().unwrap_err(),
        "replay_baseline_digest_mismatch"
    );

    // The declarative fixture keeps the same deny-first boundary.
    let required = fixture["required"].as_array().unwrap();
    let denied = fixture["denied"].as_array().unwrap();
    assert!(required.len() >= 4 && denied.len() >= 4);
    assert_eq!(
        fixture["source_mismatch_error_code"].as_str(),
        Some("replay_baseline_source_mismatch")
    );
    assert_eq!(
        fixture["missing_projection_error_code"].as_str(),
        Some("replay_receipt_digest_unavailable")
    );
    assert_eq!(
        fixture["unknown_schema_event_kind"].as_str(),
        Some("audit.forged")
    );
}

#[tokio::test]
async fn replay_bridge_compares_a_baseline_without_appending_any_fact() {
    let (run_id, _request_id, events) = consistent_run();
    let store = Arc::new(RecordingReadOnlyStore::new(events.clone()));
    let core = replay_core(store.clone());
    let plain = core.replay_diagnostics(Some(run_id), &[]).await.unwrap();
    assert!(
        plain.is_consistent(),
        "unexpected replay diagnostics: {:?}",
        plain.diagnostics
    );
    assert_eq!(plain.baseline_digest, None);

    let baseline = ReplayConsistencyBaseline::new(
        plain.source_event_ids.clone(),
        plain.projection_digests.clone(),
        Vec::new(),
    )
    .unwrap();
    let agreed = core
        .replay_diagnostics_against_baseline(Some(run_id), &[], Some(&baseline))
        .await
        .unwrap();
    assert!(
        agreed.is_consistent(),
        "unexpected baseline diagnostics: {:?}",
        agreed.diagnostics
    );
    assert_eq!(agreed.projection_digests, plain.projection_digests);
    assert_eq!(
        agreed.baseline_digest,
        Some(baseline.baseline_digest.clone())
    );
    let repeated = core
        .replay_diagnostics_against_baseline(Some(run_id), &[], Some(&baseline))
        .await
        .unwrap();
    assert_eq!(agreed.snapshot_digest, repeated.snapshot_digest);

    let mut tampered_digests = baseline.projection_digests.clone();
    tampered_digests.insert("audit".to_owned(), digest('c'));
    let tampered = ReplayConsistencyBaseline::new(
        plain.source_event_ids.clone(),
        tampered_digests,
        Vec::new(),
    )
    .unwrap();
    let diverged = core
        .replay_diagnostics_against_baseline(Some(run_id), &[], Some(&tampered))
        .await
        .unwrap();
    assert_eq!(
        diverged
            .first_divergence()
            .map(|diagnostic| diagnostic.error_code.as_str()),
        Some("replay_audit_digest_mismatch")
    );

    // Replay and baseline comparison append no fact and mutate no committed slice.
    assert_eq!(store.appends(), 0);
    assert_eq!(store.events, events);
}

#[tokio::test]
async fn replay_bridge_reports_unavailable_read_all_instead_of_an_empty_verdict() {
    let (run_id, _request_id, _events) = consistent_run();
    let core = replay_core(Arc::new(UnsupportedReadAllStore));
    let expected = "port_unavailable:replay_read_all_unsupported";
    assert_eq!(
        core.replay_diagnostics(Some(run_id), &[])
            .await
            .unwrap_err()
            .to_string(),
        expected
    );
    assert_eq!(
        core.replay_diagnostics_against_baseline(Some(run_id), &[], None)
            .await
            .unwrap_err()
            .to_string(),
        expected
    );
}

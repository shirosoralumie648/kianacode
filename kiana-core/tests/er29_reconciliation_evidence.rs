//! ER-29 deny fixtures: reconciliation evidence is a typed, digest-bound observation, not a
//! caller-authored label.
//!
//! Before this slice `failure.reconcile` accepted any string starting with `provider:` / `os:` /
//! `file:` / `human:` that merely contained the incident id. Nothing in the workspace produced
//! such a string, so the only thing the rule enforced was that an operator could type the
//! incident id back — a self-certified label, not an observation. These fixtures drive the real
//! `ControlPlane::handle_command` path and assert every refusal before the success fixture.

use async_trait::async_trait;
use kiana_core::ControlPlane;
use kiana_domain::{
    json_digest, ApprovalChallenge, ApprovalId, CapabilityRequest, CommandIntent, ExecutionStatus,
    PendingApproval, PermissionProfile, RequestContext, RequestId, RunId, RuntimeEvent,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, CapabilityBrokerPort, EventStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

struct NoBroker;

#[async_trait]
impl CapabilityBrokerPort for NoBroker {
    async fn execute(
        &self,
        _request: kiana_domain::AuthorizedCapabilityRequest,
    ) -> Result<kiana_domain::CapabilityResult, PortError> {
        Err(PortError::Failed(
            "er29_fixture_never_dispatches".to_owned(),
        ))
    }
}

struct NoRunner;

#[async_trait]
impl RunnerPort for NoRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Failed("er29_fixture_never_runs".to_owned()))
    }
}

/// Reconciliation never reaches approval staging; any call fails closed instead of passing.
struct NoApprovals;

#[async_trait]
impl ApprovalStorePort for NoApprovals {
    async fn stage(
        &self,
        _context: &RequestContext,
        _request: CapabilityRequest,
        _reason: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        Err(PortError::Failed("er29_fixture_never_stages".to_owned()))
    }

    async fn activate(&self, _approval_id: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Failed("er29_fixture_never_activates".to_owned()))
    }

    async fn consume(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Failed("er29_fixture_never_consumes".to_owned()))
    }
}

struct Fixture {
    core: ControlPlane,
    events: Arc<MemoryEventLog>,
    root: PathBuf,
    context: RequestContext,
    run_id: RunId,
}

impl Fixture {
    async fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("kiana-er29-reconcile-{stamp}"));
        fs::create_dir_all(&root).expect("project root");
        let root = fs::canonicalize(&root).expect("canonical root");
        let mut context = RequestContext::local("er29-reconcile", root.to_string_lossy());
        context.project_trusted = true;
        context.permission_profile = PermissionProfile::Balanced;
        let events = Arc::new(MemoryEventLog::new());
        let core = ControlPlane::new(
            Arc::new(DefaultPolicyEngine),
            Arc::new(DefaultGateEngine),
            events.clone(),
            Arc::new(NoBroker),
            Arc::new(NoApprovals),
            Arc::new(NoRunner),
        );
        let fixture = Self {
            core,
            events,
            root,
            context,
            run_id: RunId::new(),
        };
        fixture.append_authority().await;
        fixture.append_authorized_run().await;
        fixture
    }

    /// The protected-event commit requires a non-empty authority stream.
    async fn append_authority(&self) {
        let key = json_digest(&json!({"project_root": self.root.to_string_lossy()}));
        self.events
            .append(
                RuntimeEvent::new(RequestId::new(), 1, "authority.revised", json!({}))
                    .expect("authority event")
                    .with_stream_metadata("authority", key, 1),
            )
            .await
            .expect("authority append");
    }

    async fn append_authorized_run(&self) {
        let request_id = self.context.request_id;
        self.events
            .append(
                RuntimeEvent::new(
                    request_id,
                    1,
                    "run.authorized",
                    json!({
                        "run_id": self.run_id,
                        "actor_id": "local-user",
                        "project_root": self.root.to_string_lossy(),
                        "role_id": "builder",
                        "model_profile": "fixture",
                    }),
                )
                .expect("run.authorized event")
                .with_stream_metadata("run", self.run_id.to_string(), 1),
            )
            .await
            .expect("authorized run append");
    }

    fn run_event(&self, kind: &str, sequence: u64, data: Value) -> RuntimeEvent {
        RuntimeEvent::new(self.context.request_id, sequence, kind, data)
            .expect("run event")
            .with_stream_metadata("run", self.run_id.to_string(), sequence)
    }

    /// The incident under reconciliation: an unknown terminal fact for this run.
    async fn append_unknown_source(&self) -> RuntimeEvent {
        self.append_unknown_source_at(2).await
    }

    async fn append_unknown_source_at(&self, sequence: u64) -> RuntimeEvent {
        let event = self.run_event(
            "run.result_unknown",
            sequence,
            json!({
                "run_id": self.run_id,
                "actor_id": "local-user",
                "project_root": self.root.to_string_lossy(),
                "error": "provider response lost",
                "effect_known": false,
            }),
        );
        self.events
            .append(event.clone())
            .await
            .expect("unknown source append");
        event
    }

    /// A result-class observation on the same run. `digest` is the payload digest a caller must
    /// cite; producing it from the committed payload is what makes the ref non-forgeable.
    async fn append_observation(&self, kind: &str, sequence: u64) -> (RuntimeEvent, String) {
        let event = self.run_event(
            kind,
            sequence,
            json!({
                "run_id": self.run_id,
                "capability_request_id": RequestId::new(),
                "outcome": {"status": "observed"},
            }),
        );
        self.events
            .append(event.clone())
            .await
            .expect("observation append");
        let digest = json_digest(&event.data);
        (event, digest)
    }

    fn incident_id(source: &RuntimeEvent) -> String {
        format!("failure:{}", source.event_id)
    }

    async fn reconcile(&self, arguments: Value) -> (ExecutionStatus, Option<String>, Value) {
        let response = self
            .core
            .handle_command(
                self.context.clone(),
                CommandIntent::new("failure.reconcile", arguments),
            )
            .await
            .expect("command response");
        (response.status, response.error, response.output)
    }

    fn arguments(source: &RuntimeEvent, refs: Vec<String>, key: &str) -> Value {
        json!({
            "incident_id": Self::incident_id(source),
            "resolution": "observed_succeeded",
            "evidence_refs": refs,
            "expected_revision": 0,
            "idempotency_key": key,
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn source_ref(source: &RuntimeEvent) -> String {
    format!("event:{}", source.event_id)
}

fn observation_ref(event: &RuntimeEvent, digest: &str) -> String {
    format!("event:{}#{digest}", event.event_id)
}

#[tokio::test]
async fn caller_authored_provider_label_is_not_reconciliation_evidence() {
    let fixture = Fixture::new().await;
    let source = fixture.append_unknown_source().await;

    // The exact shape the old rule accepted: a scheme prefix plus the incident id. It is still
    // only a label; the refusal code must say the evidence could not be bound, not that it was
    // merely missing.
    for fake in [
        format!("provider:{}:fake", Fixture::incident_id(&source)),
        format!("os:{}:fake", Fixture::incident_id(&source)),
        format!("file:/tmp/{}", Fixture::incident_id(&source)),
        format!("human:{}", Fixture::incident_id(&source)),
    ] {
        let (status, error, _) = fixture
            .reconcile(Fixture::arguments(
                &source,
                vec![source_ref(&source), fake.clone()],
                "er29-fake-label",
            ))
            .await;
        assert_eq!(status, ExecutionStatus::Blocked, "accepted {fake}");
        assert_eq!(
            error.as_deref(),
            Some("failure_reconciliation_evidence_unbound"),
            "wrong refusal for {fake}"
        );
    }
}

#[tokio::test]
async fn an_event_reference_without_a_digest_or_with_a_wrong_digest_is_refused() {
    let fixture = Fixture::new().await;
    let source = fixture.append_unknown_source().await;
    let (observation, digest) = fixture.append_observation("capability.completed", 4).await;

    for reference in [
        // An id alone is a pointer anyone who can read the log could have produced.
        format!("event:{}", observation.event_id),
        // A digest that does not match the committed payload.
        format!("event:{}#sha256:{}", observation.event_id, "b".repeat(64)),
        // A source ref whose supplied digest is wrong is refused as well.
        format!("event:{}#sha256:{}", source.event_id, "c".repeat(64)),
    ] {
        let (status, error, _) = fixture
            .reconcile(Fixture::arguments(
                &source,
                vec![source_ref(&source), reference.clone()],
                "er29-digest",
            ))
            .await;
        assert_eq!(status, ExecutionStatus::Blocked, "accepted {reference}");
        assert_eq!(
            error.as_deref(),
            Some("failure_reconciliation_evidence_unbound")
        );
    }

    // Sanity: the digest this fixture computed is accepted later; the refusals above are about
    // binding, not about digest formatting being impossible to satisfy.
    assert_eq!(digest.len(), 71);
}

#[tokio::test]
async fn evidence_must_be_owned_current_and_observational() {
    let fixture = Fixture::new().await;
    // Appended before the incident: a fact that already existed when the effect was attempted
    // cannot be an observation of its outcome.
    let before = fixture.append_observation("capability.completed", 2).await;
    let source = fixture.append_unknown_source_at(3).await;

    // 1. An event from another run is not owned by this actor/project.
    let foreign_run = RunId::new();
    let foreign = RuntimeEvent::new(
        RequestId::new(),
        1,
        "capability.completed",
        json!({"run_id": foreign_run, "capability_request_id": RequestId::new(), "outcome": {}}),
    )
    .expect("foreign event")
    .with_stream_metadata("run", foreign_run.to_string(), 1);
    fixture
        .events
        .append(foreign.clone())
        .await
        .expect("append");
    let foreign_ref = format!("event:{}#{}", foreign.event_id, json_digest(&foreign.data));
    let (status, error, _) = fixture
        .reconcile(Fixture::arguments(
            &source,
            vec![source_ref(&source), foreign_ref],
            "er29-foreign",
        ))
        .await;
    assert_eq!(status, ExecutionStatus::Blocked);
    assert_eq!(
        error.as_deref(),
        Some("failure_reconciliation_evidence_unbound")
    );

    // 2. An observation that predates the incident cannot explain it.
    let before_ref = observation_ref(&before.0, &before.1);
    let (status, error, _) = fixture
        .reconcile(Fixture::arguments(
            &source,
            vec![source_ref(&source), before_ref],
            "er29-before",
        ))
        .await;
    assert_eq!(status, ExecutionStatus::Blocked);
    assert_eq!(
        error.as_deref(),
        Some("failure_reconciliation_evidence_unbound")
    );

    // 3. Another unknown fact merely restates the uncertainty; it observes nothing.
    let (unknown_again, unknown_digest) = fixture
        .append_observation("capability.result_unknown", 5)
        .await;
    let (status, error, _) = fixture
        .reconcile(Fixture::arguments(
            &source,
            vec![
                source_ref(&source),
                observation_ref(&unknown_again, &unknown_digest),
            ],
            "er29-unknown-again",
        ))
        .await;
    assert_eq!(status, ExecutionStatus::Blocked);
    assert_eq!(
        error.as_deref(),
        Some("failure_reconciliation_evidence_unbound")
    );

    // 4. Missing the source reference is a shape problem, not a binding problem: the caller has
    // to say which fact is being reconciled before any observation can be judged.
    let (observed, observed_digest) = fixture.append_observation("capability.completed", 6).await;
    let (status, error, _) = fixture
        .reconcile(Fixture::arguments(
            &source,
            vec![observation_ref(&observed, &observed_digest)],
            "er29-no-source",
        ))
        .await;
    assert_eq!(status, ExecutionStatus::Blocked);
    assert_eq!(
        error.as_deref(),
        Some("failure_reconciliation_evidence_required")
    );
}

#[tokio::test]
async fn a_digest_bound_owned_observation_reconciles_once_and_replays() {
    let fixture = Fixture::new().await;
    let source = fixture.append_unknown_source().await;
    let (observation, digest) = fixture.append_observation("capability.completed", 4).await;
    let arguments = Fixture::arguments(
        &source,
        vec![
            format!("{}#{}", source_ref(&source), json_digest(&source.data)),
            observation_ref(&observation, &digest),
        ],
        "er29-accepted",
    );

    let (status, error, output) = fixture.reconcile(arguments.clone()).await;
    assert_eq!(status, ExecutionStatus::Completed, "error: {error:?}");
    assert_eq!(output["record"]["evidence_scope"], "incident_bound");
    assert_eq!(output["record"]["evidence_binding"], "typed_event_digest");
    assert_eq!(output["record"]["runtime_outcome_unchanged"], true);
    assert_eq!(output["record"]["automatic_retry_allowed"], false);

    // Replaying the same command identity returns the committed record instead of reconciling
    // twice; the original run outcome is still untouched.
    let (status, error, replay) = fixture.reconcile(arguments).await;
    assert_eq!(status, ExecutionStatus::Completed, "error: {error:?}");
    assert_eq!(replay["replayed"], true);
    let all = fixture.events.read_all().await.expect("read events");
    assert_eq!(
        all.iter()
            .filter(|event| event.kind == "failure.reconciled")
            .count(),
        1
    );
    assert!(all
        .iter()
        .all(|event| event.kind != "run.completed" || event.event_id != source.event_id));
}

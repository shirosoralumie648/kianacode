//! ER-13 behavioral fixtures use the real atomic journal and delivery helper; no handler runs.
use super::*;
use kiana_capability_broker::CapabilityBroker;
use kiana_domain::{
    ApprovalChallenge, ApprovalId, CapabilityExecutionState, CapabilityKind, EventId,
    PendingApproval, TurnIdentity, TurnSemantics,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, RunnerPort};
use std::sync::atomic::{AtomicBool, Ordering};

struct NoApprovals;

#[async_trait::async_trait]
impl ApprovalStorePort for NoApprovals {
    async fn stage(
        &self,
        _: &RequestContext,
        _: CapabilityRequest,
        _: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        Err(PortError::Unavailable("fixture_no_approval".to_owned()))
    }
    async fn activate(&self, _: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable("fixture_no_approval".to_owned()))
    }
    async fn consume(
        &self,
        _: &RequestContext,
        _: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable("fixture_no_approval".to_owned()))
    }
}

struct CountingRunner {
    events: Arc<MemoryEventLog>,
    calls: Mutex<Vec<(RunId, CapabilityResult)>>,
    fail: bool,
}

#[async_trait::async_trait]
impl RunnerPort for CountingRunner {
    async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        let RunnerCommand::CapabilityResult { run_id, result } = command else {
            panic!("delivery must only send its committed capability result")
        };
        let claim_id = derived_request_id("result.deliver", &result.request_id.to_string());
        assert!(self.events.read_command(&claim_id).await?.is_some());
        self.calls.lock().unwrap().push((run_id, result));
        if self.fail {
            Err(PortError::Unavailable("fixture_callback_failed".to_owned()))
        } else {
            Ok(Vec::new())
        }
    }
}

fn plane(events: Arc<dyn EventStorePort>, runner: Arc<CountingRunner>) -> Arc<ControlPlane> {
    Arc::new(ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events,
        Arc::new(CapabilityBroker::new()),
        Arc::new(NoApprovals),
        runner,
    ))
}

fn scoped_prompt(run_id: RunId, version: u64, semantics: TurnSemantics) -> RuntimeEvent {
    let turn_id = TurnId::new();
    let turn =
        TurnIdentity::new("er13-fixture", run_id, turn_id, None, semantics, version).unwrap();
    RuntimeEvent::new(RequestId::new(), 1, "run.prompt",
        json!({"run_id":run_id,"turn_id":turn_id,"turn":turn,"session_id":"er13-fixture","text":"fixture"}),
    ).unwrap().with_stream_metadata("run", run_id.to_string(), version)
}

struct Fixture {
    events: Arc<MemoryEventLog>,
    runner: Arc<CountingRunner>,
    run_id: RunId,
    turn_request_id: RequestId,
    request: CapabilityRequest,
    permit: DispatchPermit,
    result: CapabilityResult,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_turn_semantics(TurnSemantics::Start).await
    }

    async fn with_turn_semantics(semantics: TurnSemantics) -> Self {
        let events = Arc::new(MemoryEventLog::new());
        let runner = Arc::new(CountingRunner {
            events: events.clone(),
            calls: Mutex::new(Vec::new()),
            fail: false,
        });
        let run_id = RunId::new();
        let turn_request_id = RequestId::new();
        let turn_id = TurnId::new();
        let identity = TurnIdentity::new(
            "er13-fixture",
            run_id,
            turn_id,
            matches!(semantics, TurnSemantics::NewTurn).then(RunId::new),
            semantics,
            if matches!(semantics, TurnSemantics::LegacyContinue) {
                2
            } else {
                1
            },
        )
        .unwrap();
        events
            .append(
                RuntimeEvent::new(
                    turn_request_id,
                    1,
                    "run.prompt",
                    json!({"run_id":run_id,"turn_id":turn_id,"turn":identity,"session_id":"er13-fixture","text":"fixture"}),
                )
                .unwrap()
                .with_stream_metadata("run", run_id.to_string(), 1),
            )
            .await
            .expect("run prompt");
        let request = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Query,
            "memory.search",
            json!({"call_id":"fixture-call","query":"fixture"}),
        );
        let mut permit = DispatchPermit {
            schema: DISPATCH_PERMIT_SCHEMA.to_owned(),
            version: DISPATCH_PERMIT_VERSION,
            execution_id: ExecutionId::new(),
            invocation_id: InvocationId::from_uuid(request.request_id.as_uuid()),
            request_id: request.request_id,
            run_id: Some(run_id),
            turn_id: Some(turn_id),
            decision_id: "fixture-authorization".to_owned(),
            approval_id: None,
            context: RequestContext::local("er13-fixture", "/fixture"),
            action_digest: kiana_domain::capability_action_digest(&request),
            project_identity: json!({"canonical_root":"/fixture"}),
            authority_versions: vec![AggregateVersion::new("run", run_id.to_string(), 1)],
            issued_at_unix_ms: 1_000,
            expires_at_unix_ms: 2_000,
            permit_digest: String::new(),
        };
        permit.permit_digest = permit.digest();
        permit.validate().expect("fixture permit");
        let result = kiana_domain::normalize_capability_result(
            request.request_id,
            CapabilityResult::success(request.request_id, json!({"answer":"committed"})),
        );
        Self {
            events,
            runner,
            run_id,
            turn_request_id,
            request,
            permit,
            result,
        }
    }

    fn core(&self) -> Arc<ControlPlane> {
        plane(self.events.clone(), self.runner.clone())
    }

    async fn prepare(&self, atomic: bool) {
        let command_id =
            derived_request_id("execution.prepare", &self.request.request_id.to_string());
        let invocation = typed_invocation_identity(
            Some(self.run_id),
            self.permit.turn_id,
            self.permit.invocation_id,
            self.permit.execution_id,
            &self.request,
            1,
        )
        .unwrap();
        let event = RuntimeEvent::new(
            command_id,
            1,
            "execution.prepared",
            json!({"run_id":self.run_id,"turn_id":self.permit.turn_id,
                "execution_id":self.permit.execution_id,"invocation_id":self.permit.invocation_id,
                "capability_request_id":self.request.request_id,"action_digest":self.permit.action_digest,
                "attempt":1,"permit":self.permit,"invocation":invocation}),
        )
        .unwrap()
        .with_stream_metadata("execution_permit", self.permit.execution_id.to_string(), 1);
        if atomic {
            assert!(!commit_confirmed(self.events.as_ref(), TransitionBatch {
                command_id,
                command_digest: json_digest(&json!({"context":self.permit.context,"run_id":self.run_id,"request":self.request})),
                expected_versions: vec![
                    AggregateVersion::new("execution_permit", self.permit.execution_id.to_string(), 0),
                    AggregateVersion::new("run", self.run_id.to_string(), 1),
                ],
                events: vec![event],
            }).await.expect("prepare commit"));
        } else {
            self.events
                .append(event)
                .await
                .expect("lookalike preparation");
        }
    }

    fn source(&self) -> RuntimeEvent {
        let receipt = CapabilityResultReceipt::from_result(
            &self.result,
            Some(self.permit.execution_id),
            Some(self.permit.invocation_id),
            1,
            true,
        )
        .unwrap();
        let invocation = typed_invocation_identity(
            Some(self.run_id),
            self.permit.turn_id,
            self.permit.invocation_id,
            self.permit.execution_id,
            &self.request,
            1,
        )
        .unwrap();
        RuntimeEvent::new(
            derived_request_id("execution.result", &self.permit.execution_id.to_string()),
            1,
            "execution.result_committed",
            committed_result_payload(
                &self.permit,
                &self.result,
                &receipt,
                invocation,
                false,
                true,
            ),
        )
        .unwrap()
        .with_stream_metadata("execution_permit", self.permit.execution_id.to_string(), 2)
    }

    async fn commit_source(&self, source: RuntimeEvent) {
        assert!(!commit_confirmed(
            self.events.as_ref(),
            TransitionBatch {
                command_id: derived_request_id(
                    "execution.result",
                    &self.permit.execution_id.to_string()
                ),
                command_digest: json_digest(&json!(self.result)),
                expected_versions: vec![AggregateVersion::new(
                    "execution_permit",
                    self.permit.execution_id.to_string(),
                    1,
                )],
                events: vec![source],
            }
        )
        .await
        .expect("result commit"));
    }

    async fn no_delivery(&self) {
        assert!(self.runner.calls.lock().unwrap().is_empty());
        assert!(self
            .events
            .read_stream("result_delivery", &self.request.request_id.to_string())
            .await
            .unwrap()
            .is_empty());
        assert!(self
            .events
            .read_command(&derived_request_id(
                "result.deliver",
                &self.request.request_id.to_string()
            ))
            .await
            .unwrap()
            .is_none());
    }
}

#[tokio::test]
async fn missing_or_append_only_execution_facts_never_claim_or_call_runner() {
    for (prepare, result) in [
        (None, false),
        (Some(true), false),
        (Some(false), true),
        (Some(true), true),
    ] {
        let fixture = Fixture::new().await;
        if let Some(atomic) = prepare {
            fixture.prepare(atomic).await;
        }
        if result {
            fixture
                .events
                .append(fixture.source())
                .await
                .expect("lookalike result");
        }
        assert!(fixture
            .core()
            .deliver_capability_result(fixture.run_id, fixture.result.clone())
            .await
            .is_err());
        fixture.no_delivery().await;
    }
}

#[tokio::test]
async fn missing_committed_source_stream_version_is_denied_without_delivery() {
    let fixture = Fixture::new().await;
    fixture.prepare(true).await;
    fixture.commit_source(fixture.source()).await;
    assert_eq!(
        fault_core(&fixture, Fault::MissingSourceVersion)
            .deliver_capability_result(fixture.run_id, fixture.result.clone())
            .await
            .unwrap_err(),
        dispatch_error("result_unknown:result_delivery_source_version_missing")
    );
    fixture.no_delivery().await;
}

#[tokio::test]
async fn contradictory_committed_payloads_or_tampered_receipts_never_deliver() {
    for drift in [
        "run",
        "turn",
        "request",
        "execution",
        "invocation",
        "attempt",
        "result",
        "outcome",
        "not_ready",
        "effect",
        "receipt_digest",
        "receipt_result",
        "uncommitted_receipt",
        "invocation_identity",
    ] {
        let fixture = Fixture::new().await;
        fixture.prepare(true).await;
        let mut source = fixture.source();
        match drift {
            "run" => source.data["run_id"] = json!(RunId::new()),
            "turn" => source.data["turn_id"] = json!(TurnId::new()),
            "request" => source.data["capability_request_id"] = json!(RequestId::new()),
            "execution" => source.data["execution_id"] = json!(ExecutionId::new()),
            "invocation" => source.data["invocation_id"] = json!(InvocationId::new()),
            "attempt" => source.data["attempt"] = json!(2),
            "result" => source.data["result"]["output"]["answer"] = json!("altered"),
            "outcome" => source.data["outcome_state"] = json!(CapabilityExecutionState::Unknown),
            "not_ready" => source.data["outcome_ready"] = json!(false),
            "effect" => source.data["effect_started"] = json!(false),
            "invocation_identity" => {
                let mut identity: kiana_domain::InvocationIdentity =
                    serde_json::from_value(source.data["invocation"].clone()).unwrap();
                identity.turn_id = TurnId::new();
                identity.identity_digest = identity.digest();
                source.data["invocation"] = json!(identity);
            }
            "receipt_digest" => {
                source.data["result_receipt"]["receipt_digest"] =
                    json!(json_digest(&json!("tampered")))
            }
            "receipt_result" => {
                let mut different = fixture.result.clone();
                different.output["answer"] = json!("altered");
                source.data["result_receipt"] = json!(CapabilityResultReceipt::from_result(
                    &different,
                    Some(fixture.permit.execution_id),
                    Some(fixture.permit.invocation_id),
                    1,
                    true,
                )
                .unwrap());
            }
            "uncommitted_receipt" => {
                let mut receipt =
                    CapabilityResultReceipt::from_json(&source.data["result_receipt"]).unwrap();
                receipt.committed = false;
                receipt.receipt_digest = receipt.digest();
                source.data["result_receipt"] = json!(receipt);
            }
            _ => unreachable!(),
        }
        fixture.commit_source(source).await;
        assert!(
            fixture
                .core()
                .deliver_capability_result(fixture.run_id, fixture.result.clone())
                .await
                .is_err(),
            "accepted {drift}"
        );
        fixture.no_delivery().await;
    }
}

#[tokio::test]
async fn changed_result_duplicate_completion_and_old_turn_cannot_advance() {
    for drift in ["result", "duplicate", "old_turn", "wrong_run"] {
        let fixture = Fixture::new().await;
        fixture.prepare(true).await;
        let source = fixture.source();
        fixture.commit_source(source.clone()).await;
        let mut result = fixture.result.clone();
        let mut run_id = fixture.run_id;
        match drift {
            "result" => result.output["answer"] = json!("uncommitted caller value"),
            "wrong_run" => {
                run_id = RunId::new();
                fixture
                    .events
                    .append(scoped_prompt(run_id, 1, TurnSemantics::Start))
                    .await
                    .unwrap();
            }
            "duplicate" => {
                let mut duplicate = source;
                duplicate.event_id = EventId::new();
                duplicate.sequence = 2;
                duplicate.stream_version = Some(3);
                fixture.events.append(duplicate).await.unwrap();
            }
            "old_turn" => {
                fixture
                    .events
                    .append(scoped_prompt(run_id, 2, TurnSemantics::LegacyContinue))
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            fixture
                .core()
                .deliver_capability_result(run_id, result)
                .await
                .is_err(),
            "accepted {drift}"
        );
        fixture.no_delivery().await;
    }
}

#[tokio::test]
async fn current_terminal_or_cancelling_run_never_claims_result() {
    for kind in [
        "run.cancelling",
        "run.cancelled",
        "run.result_unknown",
        "run.failed",
        "run.completed",
    ] {
        let fixture = Fixture::new().await;
        fixture.prepare(true).await;
        fixture.commit_source(fixture.source()).await;
        fixture
            .events
            .append(
                RuntimeEvent::new(
                    fixture.turn_request_id,
                    2,
                    kind,
                    json!({"run_id":fixture.run_id}),
                )
                .unwrap()
                .with_stream_metadata("run", fixture.run_id.to_string(), 2),
            )
            .await
            .unwrap();
        assert_eq!(
            fixture
                .core()
                .deliver_capability_result(fixture.run_id, fixture.result.clone())
                .await
                .unwrap_err(),
            dispatch_error("cancelled:result_delivery_run_inactive")
        );
        fixture.no_delivery().await;
    }
}

#[tokio::test]
async fn delivery_uses_declared_typed_turn_and_rejects_missing_or_conflicting_identity() {
    for drift in [
        "missing_id",
        "missing_identity",
        "run",
        "turn",
        "session",
        "digest",
    ] {
        let fixture = Fixture::new().await;
        fixture.prepare(true).await;
        fixture.commit_source(fixture.source()).await;
        let mut prompt = fixture
            .events
            .read_stream("run", &fixture.run_id.to_string())
            .await
            .unwrap()[0]
            .clone();
        prompt.event_id = EventId::new();
        prompt.sequence = 2;
        prompt.stream_version = Some(2);
        let mut identity: TurnIdentity =
            serde_json::from_value(prompt.data["turn"].clone()).unwrap();
        match drift {
            "missing_id" => {
                prompt.data.as_object_mut().unwrap().remove("turn_id");
            }
            "missing_identity" => {
                prompt.data.as_object_mut().unwrap().remove("turn");
            }
            "run" => {
                identity.run_id = RunId::new();
                identity.identity_digest = identity.digest();
                prompt.data["turn"] = json!(identity);
            }
            "turn" => {
                identity.turn_id = TurnId::new();
                identity.identity_digest = identity.digest();
                prompt.data["turn"] = json!(identity);
            }
            "session" => prompt.data["session_id"] = json!("other-session"),
            "digest" => {
                prompt.data["turn"]["identity_digest"] = json!(json_digest(&json!("tampered")))
            }
            _ => unreachable!(),
        }
        fixture.events.append(prompt).await.unwrap();
        assert_eq!(
            fixture
                .core()
                .deliver_capability_result(fixture.run_id, fixture.result.clone())
                .await
                .unwrap_err(),
            dispatch_error("result_unknown:result_delivery_turn_invalid"),
            "accepted {drift}"
        );
        fixture.no_delivery().await;
    }
    for semantics in [
        TurnSemantics::Start,
        TurnSemantics::NewTurn,
        TurnSemantics::LegacyContinue,
        TurnSemantics::Resume,
    ] {
        let fixture = Fixture::with_turn_semantics(semantics).await;
        assert_ne!(
            fixture.permit.turn_id.unwrap().as_uuid(),
            fixture.turn_request_id.as_uuid()
        );
        fixture.prepare(true).await;
        fixture.commit_source(fixture.source()).await;
        fixture
            .core()
            .deliver_capability_result(fixture.run_id, fixture.result.clone())
            .await
            .expect("typed turn delivery");
        assert_eq!(fixture.runner.calls.lock().unwrap().len(), 1);
    }
}

#[derive(Clone, Copy)]
enum Fault {
    Cancel,
    Terminal,
    NextTurn,
    SourceAdvance,
    UnknownClaim,
    StoredUnknownClaim,
    ReceiptEvent,
    ReceiptDigest,
    ReceiptVersion,
    ReceiptCursor,
    MissingSourceVersion,
}

struct FaultStore {
    inner: Arc<MemoryEventLog>,
    fault: Fault,
    armed: AtomicBool,
    run_id: RunId,
    execution_id: ExecutionId,
}

#[async_trait::async_trait]
impl EventStorePort for FaultStore {
    fn supports_atomic_transitions(&self) -> bool {
        true
    }

    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
        self.inner.append(event).await
    }
    async fn read_request(&self, id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_request(id).await
    }
    async fn read_stream(&self, kind: &str, id: &str) -> Result<Vec<RuntimeEvent>, PortError> {
        let mut events = self.inner.read_stream(kind, id).await?;
        if matches!(self.fault, Fault::MissingSourceVersion)
            && kind == "execution_permit"
            && id == self.execution_id.to_string()
        {
            if let Some(event) = events
                .iter_mut()
                .find(|event| event.kind == "execution.result_committed")
            {
                event.stream_version = None;
            }
        }
        Ok(events)
    }

    async fn read_command(&self, id: &RequestId) -> Result<Option<CommandReceipt>, PortError> {
        let mut receipt = self.inner.read_command(id).await?;
        if *id == derived_request_id("execution.result", &self.execution_id.to_string()) {
            if let Some(receipt) = &mut receipt {
                match self.fault {
                    Fault::ReceiptEvent => receipt.event_ids = vec![EventId::new()],
                    Fault::ReceiptDigest => receipt.command_digest = "a".repeat(64),
                    Fault::ReceiptVersion => receipt.versions[0].version += 1,
                    Fault::ReceiptCursor => receipt.first_cursor = 0,
                    _ => {}
                }
            }
        }
        Ok(receipt)
    }

    async fn commit_transition(&self, batch: TransitionBatch) -> Result<CommitOutcome, PortError> {
        if batch
            .events
            .iter()
            .any(|event| event.kind == "result.delivery_claimed")
            && !self.armed.swap(true, Ordering::SeqCst)
        {
            match self.fault {
                Fault::Cancel | Fault::Terminal | Fault::NextTurn => {
                    let event = match self.fault {
                        Fault::NextTurn => {
                            scoped_prompt(self.run_id, 2, TurnSemantics::LegacyContinue)
                        }
                        _ => RuntimeEvent::new(
                            RequestId::new(),
                            1,
                            if matches!(self.fault, Fault::Cancel) {
                                "run.cancelling"
                            } else {
                                "run.completed"
                            },
                            json!({"run_id":self.run_id}),
                        )
                        .unwrap()
                        .with_stream_metadata(
                            "run",
                            self.run_id.to_string(),
                            2,
                        ),
                    };
                    self.inner.append(event).await?;
                }
                Fault::SourceAdvance => {
                    self.inner
                        .append(
                            RuntimeEvent::new(
                                RequestId::new(),
                                1,
                                "diagnostic.source_changed",
                                json!({}),
                            )
                            .unwrap()
                            .with_stream_metadata(
                                "execution_permit",
                                self.execution_id.to_string(),
                                3,
                            ),
                        )
                        .await?;
                }
                Fault::UnknownClaim | Fault::StoredUnknownClaim => {
                    let command_id = batch.command_id;
                    if matches!(self.fault, Fault::StoredUnknownClaim) {
                        self.inner.commit_transition(batch).await?;
                    }
                    return Ok(CommitOutcome::Unknown {
                        command_id,
                        reason: "fixture_unconfirmed_claim".to_owned(),
                    });
                }
                _ => {}
            }
        }
        self.inner.commit_transition(batch).await
    }
}

fn fault_core(fixture: &Fixture, fault: Fault) -> Arc<ControlPlane> {
    plane(
        Arc::new(FaultStore {
            inner: fixture.events.clone(),
            fault,
            armed: AtomicBool::new(false),
            run_id: fixture.run_id,
            execution_id: fixture.permit.execution_id,
        }),
        fixture.runner.clone(),
    )
}

#[tokio::test]
async fn source_and_run_cas_races_or_unconfirmed_receipts_have_zero_delivery() {
    for fault in [
        Fault::Cancel,
        Fault::Terminal,
        Fault::NextTurn,
        Fault::SourceAdvance,
        Fault::UnknownClaim,
        Fault::ReceiptEvent,
        Fault::ReceiptDigest,
        Fault::ReceiptVersion,
        Fault::ReceiptCursor,
        Fault::MissingSourceVersion,
    ] {
        let fixture = Fixture::new().await;
        fixture.prepare(true).await;
        fixture.commit_source(fixture.source()).await;
        assert!(fault_core(&fixture, fault)
            .deliver_capability_result(fixture.run_id, fixture.result.clone())
            .await
            .is_err());
        fixture.no_delivery().await;
    }
}

#[tokio::test]
async fn committed_source_receipt_is_claimed_before_exact_result_reaches_runner_once() {
    for failed in [false, true] {
        let mut fixture = Fixture::new().await;
        if failed {
            let mut result =
                CapabilityResult::failure(fixture.request.request_id, "capability_failed:fixture");
            result.output["not_executed"] = json!(true);
            fixture.result =
                kiana_domain::normalize_capability_result(fixture.request.request_id, result);
        } else {
            fixture.result.output["result_receipt"] = json!({"receipt_digest":"business-output"});
        }
        fixture.prepare(true).await;
        let source = fixture.source();
        assert_eq!(source.data["effect_started"], json!(!failed));
        assert_eq!(source.data["zero_effect"], json!(failed));
        assert_eq!(source.data["effect_known"], json!(true));
        fixture.commit_source(source.clone()).await;
        let core = fixture.core();
        core.deliver_capability_result(fixture.run_id, fixture.result.clone())
            .await
            .expect("deliver");
        assert_eq!(
            *fixture.runner.calls.lock().unwrap(),
            vec![(fixture.run_id, fixture.result.clone())]
        );
        let claims = fixture
            .events
            .read_stream("result_delivery", &fixture.request.request_id.to_string())
            .await
            .unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].causation_event_id, Some(source.event_id));
        assert_eq!(
            claims[0].data["receipt_digest"],
            source.data["result_receipt"]["receipt_digest"]
        );
        assert_ne!(
            claims[0].data["receipt_digest"],
            fixture.result.output["result_receipt"]["receipt_digest"]
        );
        assert_eq!(claims[0].data["turn_id"], source.data["turn_id"]);
        let receipt = fixture
            .events
            .read_command(&derived_request_id(
                "result.deliver",
                &fixture.request.request_id.to_string(),
            ))
            .await
            .unwrap()
            .unwrap();
        assert!(receipt.versions.contains(&AggregateVersion::new(
            "execution_permit",
            fixture.permit.execution_id.to_string(),
            2
        )));
        assert!(core
            .deliver_capability_result(fixture.run_id, fixture.result.clone())
            .await
            .is_err());
        assert_eq!(fixture.runner.calls.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn concurrent_claims_advance_runner_at_most_once() {
    let fixture = Fixture::new().await;
    fixture.prepare(true).await;
    fixture.commit_source(fixture.source()).await;
    let core = fixture.core();
    let barrier = Arc::new(tokio::sync::Barrier::new(8));
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let core = core.clone();
        let barrier = barrier.clone();
        let run_id = fixture.run_id;
        let result = fixture.result.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            core.deliver_capability_result(run_id, result).await
        }));
    }
    let mut successes = 0;
    for task in tasks {
        successes += usize::from(task.await.unwrap().is_ok());
    }
    assert_eq!(successes, 1);
    assert_eq!(fixture.runner.calls.lock().unwrap().len(), 1);
    assert_eq!(
        fixture
            .events
            .read_stream("result_delivery", &fixture.request.request_id.to_string())
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn failed_callback_or_unknown_claim_ack_is_never_retried() {
    for callback_failure in [false, true] {
        let mut fixture = Fixture::new().await;
        fixture.prepare(true).await;
        fixture.commit_source(fixture.source()).await;
        let core = if callback_failure {
            fixture.runner = Arc::new(CountingRunner {
                events: fixture.events.clone(),
                calls: Mutex::new(Vec::new()),
                fail: true,
            });
            fixture.core()
        } else {
            fault_core(&fixture, Fault::StoredUnknownClaim)
        };
        let first = core
            .deliver_capability_result(fixture.run_id, fixture.result.clone())
            .await
            .unwrap_err();
        assert!(first.to_string().contains("result_unknown:"));
        let replay = core
            .deliver_capability_result(fixture.run_id, fixture.result.clone())
            .await
            .unwrap_err();
        assert!(replay
            .to_string()
            .contains("result_delivery_already_claimed"));
        assert_eq!(
            fixture.runner.calls.lock().unwrap().len(),
            usize::from(callback_failure)
        );
        assert_eq!(
            fixture
                .events
                .read_stream("result_delivery", &fixture.request.request_id.to_string())
                .await
                .unwrap()
                .len(),
            1
        );
    }
}

#[tokio::test]
async fn cancellation_after_a_winning_delivery_does_not_reopen_its_claim() {
    let fixture = Fixture::new().await;
    fixture.prepare(true).await;
    fixture.commit_source(fixture.source()).await;
    let core = fixture.core();
    core.deliver_capability_result(fixture.run_id, fixture.result.clone())
        .await
        .expect("claim wins before cancellation");
    fixture
        .events
        .append(
            RuntimeEvent::new(
                RequestId::new(),
                1,
                "run.cancelling",
                json!({"run_id":fixture.run_id}),
            )
            .unwrap()
            .with_stream_metadata("run", fixture.run_id.to_string(), 2),
        )
        .await
        .unwrap();
    assert!(core
        .deliver_capability_result(fixture.run_id, fixture.result.clone())
        .await
        .is_err());
    assert_eq!(fixture.runner.calls.lock().unwrap().len(), 1);
    assert_eq!(
        fixture
            .events
            .read_stream("result_delivery", &fixture.request.request_id.to_string())
            .await
            .unwrap()
            .len(),
        1
    );
}

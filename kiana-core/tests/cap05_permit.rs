use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_core::JournalPermitVerifier;
use kiana_domain::{
    AggregateVersion, AuthenticatedPrincipalRef, AuthorizedCapabilityRequest, CapabilityKind,
    CapabilityRequest, CapabilityResult, CommandReceipt, CommitOutcome, DispatchPermit,
    ExecutionId, ExecutionScope, InvocationId, InvocationIdentity, ProjectIdentity, RequestContext,
    RequestId, RunId, RuntimeEvent, ScopeDimension, ScopeLimit, ScopeSet, TransitionBatch, TurnId,
    DISPATCH_PERMIT_SCHEMA, DISPATCH_PERMIT_VERSION,
};
use kiana_eventlog::MemoryEventLog;
use kiana_ports::{CapabilityBrokerPort, EventStorePort, ExecutionPermitVerifierPort, PortError};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::sync::Barrier;

fn request(context: &RequestContext) -> CapabilityRequest {
    CapabilityRequest::new(
        context.request_id,
        CapabilityKind::Query,
        "context.search",
        json!({"query":"permit","call_id":"cap05-call"}),
    )
}

fn prepared_event_payload(
    permit: &DispatchPermit,
    invocation: &InvocationIdentity,
    cell_reservation: Option<serde_json::Value>,
) -> serde_json::Value {
    json!({
        "run_id":permit.run_id,
        "turn_id":permit.turn_id,
        "invocation_id":permit.invocation_id,
        "execution_id":permit.execution_id,
        "capability_request_id":permit.request_id,
        "action_digest":permit.action_digest,
        "attempt":1,
        "permit":permit,
        "cell_reservation":cell_reservation,
        "invocation":invocation,
    })
}

async fn prepared_verifier() -> (
    Arc<JournalPermitVerifier>,
    Arc<dyn EventStorePort>,
    AuthorizedCapabilityRequest,
    DispatchPermit,
) {
    prepared_verifier_with_payload_change(|_| {}).await
}

async fn prepared_verifier_with_payload_change(
    change: impl FnOnce(&mut serde_json::Value),
) -> (
    Arc<JournalPermitVerifier>,
    Arc<dyn EventStorePort>,
    AuthorizedCapabilityRequest,
    DispatchPermit,
) {
    let events: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    prepared_verifier_in(events, false, change).await
}

async fn prepared_verifier_in(
    events: Arc<dyn EventStorePort>,
    broker_scoped: bool,
    change: impl FnOnce(&mut serde_json::Value),
) -> (
    Arc<JournalPermitVerifier>,
    Arc<dyn EventStorePort>,
    AuthorizedCapabilityRequest,
    DispatchPermit,
) {
    let verifier = Arc::new(JournalPermitVerifier::new(events.clone()));
    let mut context = RequestContext::local("session-cap05", ".");
    context.project_trusted = true;
    let mut request = request(&context);
    let execution_id = ExecutionId::new();
    let invocation_id = InvocationId::from_uuid(request.request_id.as_uuid());
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    if broker_scoped {
        kiana_domain::normalize_capability_action(&mut request).unwrap();
        request.execution_scope = Some(broker_scope(&context, &request, run_id, turn_id));
    }
    let project_identity = kiana_core::project_root_identity(&context.project_root).unwrap();
    events
        .append(
            RuntimeEvent::new(
                RequestId::new(),
                1,
                "authority.snapshot",
                json!({"project_trusted":true}),
            )
            .unwrap()
            .with_stream_metadata("authority", "authority-key", 1),
        )
        .await
        .unwrap();
    let mut permit = DispatchPermit {
        schema: DISPATCH_PERMIT_SCHEMA.to_owned(),
        version: DISPATCH_PERMIT_VERSION,
        execution_id,
        invocation_id,
        request_id: request.request_id,
        run_id: Some(run_id),
        turn_id: Some(turn_id),
        decision_id: "policy:cap05".to_owned(),
        approval_id: None,
        context,
        action_digest: kiana_domain::capability_action_digest(&request),
        project_identity,
        authority_versions: vec![AggregateVersion::new("authority", "authority-key", 1)],
        issued_at_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        permit_digest: String::new(),
    };
    permit.permit_digest = permit.digest();
    let invocation = InvocationIdentity::new(
        run_id,
        turn_id,
        invocation_id,
        execution_id,
        request.arguments["call_id"].as_str().map(str::to_owned),
        1,
    )
    .unwrap();
    let prepared_command =
        kiana_domain::derived_request_id("execution.prepare", &request.request_id.to_string());
    let mut prepared_payload = prepared_event_payload(&permit, &invocation, None);
    change(&mut prepared_payload);
    events
        .append(
            RuntimeEvent::new(prepared_command, 1, "execution.prepared", prepared_payload)
                .unwrap()
                .with_stream_metadata("execution_permit", execution_id.to_string(), 1)
                .with_identity_links(Some(prepared_command), Some(request.request_id), None, None),
        )
        .await
        .unwrap();
    let authorized =
        AuthorizedCapabilityRequest::new(format!("permit:{execution_id}"), request).unwrap();
    (verifier, events, authorized, permit)
}

fn broker_scope(
    context: &RequestContext,
    request: &CapabilityRequest,
    run_id: RunId,
    turn_id: TurnId,
) -> ExecutionScope {
    let permission_scope = ScopeSet::new(
        ScopeDimension::Restricted(vec![request.operation.clone()]),
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeLimit::NotApplicable,
        ScopeLimit::NotApplicable,
    )
    .unwrap();
    let trust_revision = kiana_domain::json_digest(&json!("cap05-server-trust"));
    let project_identity = kiana_core::project_root_identity(&context.project_root).unwrap();
    let canonical_root = project_identity["canonical_root"].as_str().unwrap();
    let project = ProjectIdentity::new(
        context.project_root.as_str(),
        canonical_root,
        None,
        None,
        trust_revision.as_str(),
    )
    .unwrap();
    let mut scope = ExecutionScope {
        schema: kiana_domain::EXECUTION_SCOPE_SCHEMA.to_owned(),
        version: kiana_domain::EXECUTION_SCOPE_SCHEMA_VERSION,
        principal: AuthenticatedPrincipalRef::local(),
        project,
        session_id: context.session_id.clone(),
        run_id: Some(run_id),
        turn_id: Some(turn_id),
        cell_id: None,
        grant_refs: Vec::new(),
        budget_lease_id: None,
        work_packet_id: None,
        environment_id: "kiana-local".to_owned(),
        workspace_revision: None,
        permission_scope_digest: permission_scope.digest(),
        permission_scope,
        read_roots: vec![canonical_root.to_owned()],
        write_roots: Vec::new(),
        read_denies: Vec::new(),
        write_denies: Vec::new(),
        memory_scopes: Vec::new(),
        server_scopes: Vec::new(),
        network_policy: Vec::new(),
        authority_epoch: 1,
        trust_revision,
        data_epoch: 1,
        cancellation_epoch: 1,
        deadline_unix_ms: u64::MAX,
        fencing_token: 1,
        catalog_digest: kiana_domain::capability_action_catalog_digest(),
        action_digest: kiana_domain::capability_action_digest(request),
        scope_digest: String::new(),
    };
    scope.scope_digest = scope.digest();
    scope.validate_for_request(request).unwrap();
    scope
}

struct CountingQueryHandler {
    events: Arc<dyn EventStorePort>,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl CapabilityHandler for CountingQueryHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(request.request.operation, "context.search");
        let id = request.authorization_id.strip_prefix("permit:").unwrap();
        let records = self.events.read_stream("execution_permit", id).await?;
        assert_eq!(
            records
                .iter()
                .map(|event| event.kind.as_str())
                .collect::<Vec<_>>(),
            [
                "execution.prepared",
                "invocation.dispatching",
                "invocation.executing"
            ]
        );
        let command_id = kiana_domain::derived_request_id("permit.consume", id);
        let receipt = self
            .events
            .read_command(&command_id)
            .await?
            .expect("handler requires the confirmed atomic consumption receipt");
        assert_eq!(
            receipt.event_ids,
            records[1..]
                .iter()
                .map(|event| event.event_id)
                .collect::<Vec<_>>()
        );
        assert_eq!(records[1].stream_version, Some(2));
        assert_eq!(records[2].stream_version, Some(3));
        Ok(CapabilityResult::success(
            request.request.request_id,
            json!({"hits":[]}),
        ))
    }
}

struct UnusedHandler {
    binding_version: &'static str,
}

#[async_trait]
impl CapabilityHandler for UnusedHandler {
    fn binding_version(&self) -> &'static str {
        self.binding_version
    }

    async fn execute(&self, _: AuthorizedCapabilityRequest) -> Result<CapabilityResult, PortError> {
        Err(PortError::Failed("cap05_unexpected_handler".to_owned()))
    }
}

fn broker_with_counter(
    verifier: Arc<JournalPermitVerifier>,
    events: Arc<dyn EventStorePort>,
) -> (CapabilityBroker, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let query: Arc<dyn CapabilityHandler> = Arc::new(CountingQueryHandler {
        events,
        calls: calls.clone(),
    });
    let mut broker = CapabilityBroker::new();
    for operation in kiana_domain::ACTION_OPERATIONS {
        let descriptor = kiana_domain::capability_action_descriptor(operation).unwrap();
        let handler: Arc<dyn CapabilityHandler> = if *operation == "context.search" {
            query.clone()
        } else {
            Arc::new(UnusedHandler {
                binding_version: descriptor.binding_version,
            })
        };
        broker
            .register_static(descriptor.capability, *operation, handler)
            .unwrap();
    }
    broker.validate_catalog_bindings().unwrap();
    broker.set_permit_verifier(verifier);
    (broker, calls)
}

#[derive(Clone, Copy, Debug)]
enum CommitFault {
    None,
    Failed,
    Unknown,
    AuthorityChangedBeforeCas,
    ConcurrentCas,
}

/// Faults the adapter acknowledgement/CAS boundary, never authorization. All facts and successful
/// transitions remain in one MemoryEventLog; no synthetic verifier or consumption registry exists.
struct FaultJournal {
    inner: MemoryEventLog,
    fault: CommitFault,
    commits: AtomicUsize,
    barrier: Barrier,
}

impl FaultJournal {
    fn new(fault: CommitFault) -> Self {
        Self {
            inner: MemoryEventLog::new(),
            fault,
            commits: AtomicUsize::new(0),
            barrier: Barrier::new(2),
        }
    }
}

#[async_trait]
impl EventStorePort for FaultJournal {
    fn supports_atomic_transitions(&self) -> bool {
        self.inner.supports_atomic_transitions()
    }

    fn capabilities(&self) -> kiana_domain::EventStoreCapabilities {
        self.inner.capabilities()
    }

    async fn commit_transition(&self, batch: TransitionBatch) -> Result<CommitOutcome, PortError> {
        self.commits.fetch_add(1, Ordering::SeqCst);
        match self.fault {
            CommitFault::Failed => {
                return Err(PortError::Unavailable("cap05_commit_failed".to_owned()))
            }
            CommitFault::Unknown => {
                return Ok(CommitOutcome::Unknown {
                    command_id: batch.command_id,
                    reason: "cap05_commit_unconfirmed".to_owned(),
                })
            }
            CommitFault::ConcurrentCas => {
                self.barrier.wait().await;
            }
            CommitFault::AuthorityChangedBeforeCas => {
                assert!(batch.expected_versions.iter().any(|version| {
                    version.aggregate_type == "authority" && version.version == 1
                }));
                self.inner
                    .append_expected(
                        RuntimeEvent::new(
                            RequestId::new(),
                            1,
                            "authority.snapshot",
                            json!({"project_trusted":false}),
                        )
                        .unwrap()
                        .with_stream_metadata(
                            "authority",
                            "authority-key",
                            2,
                        ),
                        Some(1),
                    )
                    .await?;
            }
            CommitFault::None => {}
        }
        self.inner.commit_transition(batch).await
    }

    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
        self.inner.append(event).await
    }

    async fn read_request(&self, id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_request(id).await
    }

    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_all().await
    }

    async fn read_stream(&self, kind: &str, id: &str) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_stream(kind, id).await
    }

    async fn read_command(&self, id: &RequestId) -> Result<Option<CommandReceipt>, PortError> {
        self.inner.read_command(id).await
    }
}

async fn assert_prepared_only(events: &dyn EventStorePort, permit: &DispatchPermit) {
    let records = events
        .read_stream("execution_permit", &permit.execution_id.to_string())
        .await
        .unwrap();
    assert_eq!(
        records.len(),
        1,
        "rejected dispatch cannot write execution facts"
    );
    assert_eq!(records[0].kind, "execution.prepared");
    assert_eq!(records[0].stream_version, Some(1));
    let consume =
        kiana_domain::derived_request_id("permit.consume", &permit.execution_id.to_string());
    assert!(events.read_command(&consume).await.unwrap().is_none());
}

#[tokio::test]
async fn broker_unknown_authorization_has_zero_handler_calls_and_no_execution_facts() {
    let events: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    let (verifier, events, authorized, permit) = prepared_verifier_in(events, true, |_| {}).await;
    let (broker, calls) = broker_with_counter(verifier, events.clone());
    for (authorization_id, expected) in [
        ("opaque".to_owned(), "execution_permit_required"),
        ("permit:".to_owned(), "execution_permit_required"),
        (
            format!("permit:{}", ExecutionId::new()),
            "execution_permit_unavailable",
        ),
    ] {
        let unknown =
            AuthorizedCapabilityRequest::new(authorization_id, authorized.request.clone()).unwrap();
        assert!(
            matches!(broker.execute(unknown).await, Err(PortError::Failed(reason)) if reason == expected)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_prepared_only(events.as_ref(), &permit).await;
    }
}

#[tokio::test]
async fn broker_success_enters_handler_once_after_atomic_commit_and_replay_adds_no_facts() {
    let journal = Arc::new(FaultJournal::new(CommitFault::None));
    let (verifier, events, authorized, permit) =
        prepared_verifier_in(journal.clone(), true, |_| {}).await;
    let (broker, calls) = broker_with_counter(verifier, events.clone());
    let (_keep_running, cancellation) = tokio::sync::watch::channel(false);
    let result = broker
        .execute_cancellable(authorized.clone(), cancellation)
        .await
        .unwrap();
    assert!(result.success);
    assert_eq!(result.request_id, authorized.request.request_id);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(journal.commits.load(Ordering::SeqCst), 1);
    let consumed = events
        .read_stream("execution_permit", &permit.execution_id.to_string())
        .await
        .unwrap();
    assert!(
        matches!(broker.execute(authorized).await, Err(PortError::Failed(reason)) if reason == "execution_permit_already_consumed")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(journal.commits.load(Ordering::SeqCst), 1);
    assert_eq!(
        events
            .read_stream("execution_permit", &permit.execution_id.to_string())
            .await
            .unwrap(),
        consumed
    );
}

#[tokio::test]
async fn broker_concurrent_cas_contenders_reach_one_handler() {
    let journal = Arc::new(FaultJournal::new(CommitFault::ConcurrentCas));
    let (verifier, events, authorized, permit) =
        prepared_verifier_in(journal.clone(), true, |_| {}).await;
    let (broker, calls) = broker_with_counter(verifier, events.clone());
    // Both verifiers have read the prepared/authority versions before either real CAS begins.
    // The existing MemoryEventLog arbitrates the winner, not a test-side once flag.
    let (left, right) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            broker.execute(authorized.clone()),
            broker.execute(authorized.clone())
        )
    })
    .await
    .expect("both broker contenders reached their CAS barrier");
    let outcomes = [left, right];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(outcomes.iter().filter(|result| result.is_err()).count(), 1);
    assert!(outcomes.iter().any(|result| {
        matches!(result, Err(PortError::Failed(reason)) if reason == "execution_permit_already_consumed")
            || matches!(result, Err(PortError::Conflict(reason)) if reason == "control_transition_conflict")
    }));
    assert_eq!(journal.commits.load(Ordering::SeqCst), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let records = events
        .read_stream("execution_permit", &permit.execution_id.to_string())
        .await
        .unwrap();
    assert_eq!(records.len(), 3);
    assert_eq!(
        records
            .iter()
            .filter(|event| event.kind == "invocation.executing")
            .count(),
        1
    );
    let consume =
        kiana_domain::derived_request_id("permit.consume", &permit.execution_id.to_string());
    let receipt = events.read_command(&consume).await.unwrap().unwrap();
    assert_eq!(receipt.event_ids.len(), 2);
    assert!(
        matches!(broker.execute(authorized).await, Err(PortError::Failed(reason)) if reason == "execution_permit_already_consumed")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(journal.commits.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn broker_stale_authority_and_malformed_read_sets_have_zero_handlers() {
    let events: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    let (verifier, events, authorized, permit) = prepared_verifier_in(events, true, |_| {}).await;
    let (broker, calls) = broker_with_counter(verifier, events.clone());
    events
        .append(
            RuntimeEvent::new(
                RequestId::new(),
                1,
                "authority.snapshot",
                json!({"project_trusted":false}),
            )
            .unwrap()
            .with_stream_metadata("authority", "authority-key", 2),
        )
        .await
        .unwrap();
    assert!(
        matches!(broker.execute(authorized).await, Err(PortError::Failed(reason)) if reason == "old_epoch_permit_rejected")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_prepared_only(events.as_ref(), &permit).await;

    for duplicate in [false, true] {
        let events: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
        let (verifier, events, authorized, permit) =
            prepared_verifier_in(events, true, |payload| {
                let mut stored: DispatchPermit =
                    serde_json::from_value(payload["permit"].clone()).unwrap();
                if duplicate {
                    stored
                        .authority_versions
                        .push(stored.authority_versions[0].clone());
                } else {
                    stored.authority_versions.clear();
                }
                stored.permit_digest = stored.digest();
                payload["permit"] = stored.to_json().unwrap();
            })
            .await;
        let (broker, calls) = broker_with_counter(verifier, events.clone());
        assert!(
            matches!(broker.execute(authorized).await, Err(PortError::Failed(reason)) if reason == "execution_permit_invalid")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_prepared_only(events.as_ref(), &permit).await;
    }
}

#[tokio::test]
async fn broker_commit_failure_unknown_ack_and_read_set_race_have_zero_effects() {
    for fault in [
        CommitFault::Failed,
        CommitFault::Unknown,
        CommitFault::AuthorityChangedBeforeCas,
    ] {
        let journal = Arc::new(FaultJournal::new(fault));
        let (verifier, events, authorized, permit) =
            prepared_verifier_in(journal.clone(), true, |_| {}).await;
        let (broker, calls) = broker_with_counter(verifier, events.clone());
        let error = broker.execute(authorized).await.unwrap_err();
        match fault {
            CommitFault::Failed => assert!(
                matches!(error, PortError::Unavailable(reason) if reason == "cap05_commit_failed")
            ),
            CommitFault::Unknown => assert!(
                matches!(error, PortError::Failed(reason) if reason == "result_unknown:control_commit_unconfirmed")
            ),
            CommitFault::AuthorityChangedBeforeCas => assert!(
                matches!(error, PortError::Conflict(reason) if reason == "control_transition_conflict")
            ),
            _ => unreachable!("only refusal faults are exercised"),
        }
        assert_eq!(
            journal.commits.load(Ordering::SeqCst),
            1,
            "{fault:?} reached the actual commit boundary"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "{fault:?} started no handler"
        );
        assert_prepared_only(events.as_ref(), &permit).await;
        if matches!(fault, CommitFault::AuthorityChangedBeforeCas) {
            let authority = events
                .read_stream("authority", "authority-key")
                .await
                .unwrap();
            assert_eq!(
                authority.len(),
                2,
                "the authority change itself was committed"
            );
            assert_eq!(authority[1].stream_version, Some(2));
        }
    }
}

#[tokio::test]
async fn broker_cancel_before_consume_starts_no_handler_and_writes_no_execution_facts() {
    let journal = Arc::new(FaultJournal::new(CommitFault::None));
    let (verifier, events, authorized, permit) =
        prepared_verifier_in(journal.clone(), true, |_| {}).await;
    let (broker, calls) = broker_with_counter(verifier, events.clone());
    let (_cancel, cancelled) = tokio::sync::watch::channel(true);
    assert!(
        matches!(broker.execute_cancellable(authorized.clone(), cancelled).await,
        Err(PortError::Failed(reason)) if reason == "cancelled:before_broker")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(journal.commits.load(Ordering::SeqCst), 0);
    assert_prepared_only(events.as_ref(), &permit).await;
}

#[tokio::test]
async fn concurrent_dispatch_consumes_one_permit() {
    let (verifier, events, authorized, permit) = prepared_verifier().await;
    let left = verifier.verify_and_consume(&authorized);
    let right = verifier.verify_and_consume(&authorized);
    let (left, right) = tokio::join!(left, right);
    let results = [left, right];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    assert!(results.iter().any(|result| {
        matches!(result, Err(PortError::Failed(reason)) if reason == "execution_permit_already_consumed")
            || matches!(result, Err(PortError::Conflict(reason)) if reason == "control_transition_conflict")
    }));
    let records = events
        .read_stream("execution_permit", &permit.execution_id.to_string())
        .await
        .unwrap();
    assert_eq!(
        records
            .iter()
            .map(|record| record.kind.as_str())
            .collect::<Vec<_>>(),
        [
            "execution.prepared",
            "invocation.dispatching",
            "invocation.executing"
        ]
    );
    assert_eq!(
        records
            .iter()
            .map(|record| record.stream_version.unwrap())
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert!(records[1..].iter().all(|record| {
        record.data["permit_digest"] == permit.permit_digest
            && record.data["execution_id"] == permit.execution_id.to_string()
            && record.data["invocation_id"] == permit.invocation_id.to_string()
            && record.data["capability_request_id"] == permit.request_id.to_string()
    }));

    assert!(matches!(
        verifier.verify_and_consume(&authorized).await,
        Err(PortError::Failed(reason)) if reason == "execution_permit_already_consumed"
    ));
    assert_eq!(
        events
            .read_stream("execution_permit", &permit.execution_id.to_string())
            .await
            .unwrap()
            .len(),
        3
    );
}

#[tokio::test]
async fn preexisting_executing_fact_is_rejected_without_dispatching_or_effect() {
    let (verifier, events, authorized, permit) = prepared_verifier().await;
    let prepared = events
        .read_stream("execution_permit", &permit.execution_id.to_string())
        .await
        .unwrap()
        .remove(0);
    let forged_command = kiana_domain::derived_request_id(
        "execution.start.forged",
        &permit.execution_id.to_string(),
    );
    events
        .append(
            RuntimeEvent::new(
                forged_command,
                1,
                "invocation.executing",
                json!({
                    "run_id":permit.run_id,
                    "turn_id":permit.turn_id,
                    "invocation_id":permit.invocation_id,
                    "execution_id":permit.execution_id,
                    "capability_request_id":permit.request_id,
                    "call_id":authorized.request.arguments["call_id"],
                    "operation":authorized.request.operation,
                    "capability":authorized.request.capability,
                    "action_digest":permit.action_digest,
                    "args_fingerprint":permit.action_digest,
                    "decision_id":permit.decision_id,
                    "permit_digest":permit.permit_digest,
                    "attempt":1,
                    "started":true,
                    "effect_started":true,
                    "effect_known":true,
                    "zero_effect":false,
                    "stop_state":"not_requested",
                    "fenced":true,
                    "boundary":"handler_execution",
                    "invocation":prepared.data["invocation"],
                }),
            )
            .unwrap()
            .with_stream_metadata("execution_permit", permit.execution_id.to_string(), 2)
            .with_identity_links(
                Some(forged_command),
                Some(permit.request_id),
                Some(prepared.event_id),
                None,
            ),
        )
        .await
        .unwrap();

    assert!(matches!(
        verifier.verify_and_consume(&authorized).await,
        Err(PortError::Failed(reason)) if reason == "execution_permit_already_consumed"
    ));
    let records = events
        .read_stream("execution_permit", &permit.execution_id.to_string())
        .await
        .unwrap();
    assert_eq!(records.len(), 2);
    assert!(!records
        .iter()
        .any(|record| record.kind == "invocation.dispatching"));
}

#[tokio::test]
async fn request_drift_does_not_consume_execution_permit() {
    let (verifier, events, authorized, permit) = prepared_verifier().await;
    let mut changed_request = authorized.request.clone();
    changed_request.arguments["query"] = json!("changed");
    let changed =
        AuthorizedCapabilityRequest::new(authorized.authorization_id.clone(), changed_request)
            .unwrap();

    assert!(matches!(
        verifier.verify_and_consume(&changed).await,
        Err(PortError::Failed(reason)) if reason == "execution_permit_scope_or_expiry_mismatch"
    ));
    assert_eq!(
        events
            .read_stream("execution_permit", &permit.execution_id.to_string())
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn prepared_identity_header_drift_does_not_consume_execution_permit() {
    let (verifier, events, authorized, permit) = prepared_verifier_with_payload_change(|payload| {
        payload["action_digest"] = json!(format!("sha256:{}", "0".repeat(64)));
    })
    .await;

    assert!(matches!(
        verifier.verify_and_consume(&authorized).await,
        Err(PortError::Failed(reason)) if reason == "execution_permit_invalid"
    ));
    let records = events
        .read_stream("execution_permit", &permit.execution_id.to_string())
        .await
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, "execution.prepared");
}

#[tokio::test]
async fn opaque_or_empty_authorization_never_reaches_dispatch() {
    let events: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    let verifier = JournalPermitVerifier::new(events);
    let request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Query,
        "context.search",
        json!({"query":"permit"}),
    );
    for authorization_id in ["opaque", "permit:"] {
        let authorized =
            AuthorizedCapabilityRequest::new(authorization_id, request.clone()).unwrap();
        assert!(matches!(
            verifier.verify_and_consume(&authorized).await,
            Err(PortError::Failed(reason)) if reason == "execution_permit_required"
        ));
    }
}

#[test]
fn cap05_dispatch_has_no_authorization_or_epoch_bypass() {
    let dispatch = include_str!("../src/dispatch.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let domain = include_str!("../../kiana-domain/src/dispatch.rs");
    let event_contracts = include_str!("../../kiana-domain/src/event_contracts.rs");
    for marker in [
        "execution_permit_required",
        "execution_permit_already_consumed",
        "execution_permit_invocation_mismatch",
        "action_digest",
        "cell_reservation",
        "attempt",
        "validate_for_request",
        "old_epoch_permit_rejected",
        "authority_versions",
        "execution.prepared",
        "invocation.dispatching",
        "invocation.executing",
        "permit_digest",
        "events: vec![dispatching, executing]",
        "commit_confirmed",
        "ExecutionPermitVerifierPort",
    ] {
        assert!(
            dispatch.contains(marker)
                || broker.contains(marker)
                || domain.contains(marker)
                || event_contracts.contains(marker),
            "CAP-05 marker missing: {marker}"
        );
    }
    assert!(!dispatch.contains("commit_invocation_executing"));
    let verifier = broker
        .find("verify_and_consume(&request)")
        .expect("broker must verify the permit at dispatch");
    let handler = broker
        .find("handler\n            .execute_cancellable(request.clone(), cancellation)")
        .expect("handler invocation must remain after verifier");
    assert!(verifier < handler);
    for forbidden in [
        "nonempty_authorization_is_authority",
        "dispatch_without_permit",
        "HashSet<RequestId>",
        "execute_before_verify",
    ] {
        assert!(
            !dispatch.contains(forbidden),
            "forbidden CAP-05 path: {forbidden}"
        );
    }
}

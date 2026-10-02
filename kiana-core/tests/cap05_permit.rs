use kiana_core::JournalPermitVerifier;
use kiana_domain::{
    AggregateVersion, AuthorizedCapabilityRequest, CapabilityKind, CapabilityRequest,
    DispatchPermit, ExecutionId, InvocationId, InvocationIdentity, RequestContext, RequestId,
    RunId, RuntimeEvent, TurnId, DISPATCH_PERMIT_SCHEMA, DISPATCH_PERMIT_VERSION,
};
use kiana_eventlog::MemoryEventLog;
use kiana_ports::{EventStorePort, ExecutionPermitVerifierPort, PortError};
use serde_json::json;
use std::sync::Arc;

fn request(context: &RequestContext) -> CapabilityRequest {
    CapabilityRequest::new(
        context.request_id,
        CapabilityKind::Query,
        "context.search",
        json!({"query":"permit","call_id":"cap05-call"}),
    )
}

async fn prepared_verifier() -> (
    Arc<JournalPermitVerifier>,
    Arc<dyn EventStorePort>,
    AuthorizedCapabilityRequest,
    DispatchPermit,
) {
    let events: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    let verifier = Arc::new(JournalPermitVerifier::new(events.clone()));
    let mut context = RequestContext::local("session-cap05", ".");
    context.project_trusted = true;
    let request = request(&context);
    let execution_id = ExecutionId::new();
    let invocation_id = InvocationId::from_uuid(request.request_id.as_uuid());
    let run_id = RunId::new();
    let turn_id = TurnId::new();
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
    events
        .append(
            RuntimeEvent::new(
                prepared_command,
                1,
                "execution.prepared",
                json!({
                    "run_id":run_id,
                    "turn_id":turn_id,
                    "invocation_id":invocation_id,
                    "execution_id":execution_id,
                    "capability_request_id":request.request_id,
                    "attempt":1,
                    "action_digest":permit.action_digest,
                    "permit":permit,
                    "invocation":invocation,
                }),
            )
            .unwrap()
            .with_stream_metadata("execution_permit", execution_id.to_string(), 1)
            .with_identity_links(
                Some(prepared_command),
                Some(request.request_id),
                None,
                None,
            ),
        )
        .await
        .unwrap();
    let authorized =
        AuthorizedCapabilityRequest::new(format!("permit:{execution_id}"), request).unwrap();
    (verifier, events, authorized, permit)
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

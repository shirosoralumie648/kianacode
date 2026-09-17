use kiana_core::JournalPermitVerifier;
use kiana_domain::{
    AggregateVersion, AuthorizedCapabilityRequest, CapabilityKind, CapabilityRequest,
    DispatchPermit, ExecutionId, InvocationId, RequestContext, RequestId, RuntimeEvent,
    DISPATCH_PERMIT_SCHEMA, DISPATCH_PERMIT_VERSION,
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
        json!({"query":"permit"}),
    )
}

async fn prepared_verifier() -> (Arc<JournalPermitVerifier>, AuthorizedCapabilityRequest) {
    let events: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    let verifier = Arc::new(JournalPermitVerifier::new(events.clone()));
    let mut context = RequestContext::local("session-cap05", ".");
    context.project_trusted = true;
    let request = request(&context);
    let execution_id = ExecutionId::new();
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
        invocation_id: InvocationId::from_uuid(request.request_id.as_uuid()),
        request_id: request.request_id,
        run_id: None,
        turn_id: None,
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
    events
        .append(
            RuntimeEvent::new(
                RequestId::new(),
                1,
                "execution.prepared",
                json!({"permit":permit}),
            )
            .unwrap()
            .with_stream_metadata("execution_permit", execution_id.to_string(), 1),
        )
        .await
        .unwrap();
    let authorized =
        AuthorizedCapabilityRequest::new(format!("permit:{execution_id}"), request).unwrap();
    (verifier, authorized)
}

#[tokio::test]
async fn concurrent_dispatch_consumes_one_permit() {
    let (verifier, authorized) = prepared_verifier().await;
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
    for marker in [
        "execution_permit_required",
        "execution_permit_already_consumed",
        "validate_for_request",
        "old_epoch_permit_rejected",
        "authority_versions",
        "execution.prepared",
        "invocation.dispatching",
        "commit_confirmed",
        "ExecutionPermitVerifierPort",
    ] {
        assert!(
            dispatch.contains(marker) || broker.contains(marker) || domain.contains(marker),
            "CAP-05 marker missing: {marker}"
        );
    }
    let verifier = broker
        .find("verify_and_consume(&request)")
        .expect("broker must verify the permit at dispatch");
    let handler = broker
        .find("handler.execute_cancellable(request, cancellation).await")
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

use kiana_domain::{
    AggregateVersion, CapabilityKind, CapabilityRequest, DispatchPermit, ExecutionId, InvocationId,
    RequestContext, DISPATCH_PERMIT_SCHEMA, DISPATCH_PERMIT_VERSION,
};
use serde_json::json;

fn permit() -> (DispatchPermit, CapabilityRequest) {
    let mut context = RequestContext::local("session-1", "/repo");
    context.project_trusted = true;
    let request = CapabilityRequest::new(
        context.request_id,
        CapabilityKind::Query,
        "search",
        json!({"query":"roadmap"}),
    );
    let mut permit = DispatchPermit {
        schema: DISPATCH_PERMIT_SCHEMA.to_owned(),
        version: DISPATCH_PERMIT_VERSION,
        execution_id: ExecutionId::new(),
        invocation_id: InvocationId::from_uuid(request.request_id.as_uuid()),
        request_id: request.request_id,
        run_id: None,
        turn_id: None,
        decision_id: format!("policy:{}", request.request_id),
        approval_id: None,
        context,
        action_digest: kiana_domain::capability_action_digest(&request),
        project_identity: json!({"canonical_root":"/repo","device":1,"inode":2}),
        authority_versions: vec![AggregateVersion::new("authority", "authority-key", 1)],
        issued_at_unix_ms: 100,
        expires_at_unix_ms: 1_000,
        permit_digest: String::new(),
    };
    permit.permit_digest = permit.digest();
    (permit, request)
}

#[test]
fn dispatch_permit_is_opaque_and_binds_exact_action_and_expiry() {
    let (permit, request) = permit();
    assert!(permit.validate().is_ok());
    assert!(permit
        .validate_for_request(&request, &permit.project_identity, 500)
        .is_ok());
    let value = permit.to_json().unwrap();
    assert_eq!(DispatchPermit::from_json(&value).unwrap(), permit);
    let mut changed = request.clone();
    changed.arguments["query"] = json!("different");
    assert!(permit
        .validate_for_request(&changed, &permit.project_identity, 500)
        .is_err());
    assert!(permit
        .validate_for_request(&request, &permit.project_identity, 1_000)
        .is_err());
}

#[test]
fn dispatch_permit_rejects_tampered_digest_or_authority_read_set() {
    let (permit, _) = permit();
    let mut value = permit.to_json().unwrap();
    value["permit_digest"] = json!(format!("sha256:{}", "0".repeat(64)));
    assert!(DispatchPermit::from_json(&value).is_err());
    let mut duplicate = permit.clone();
    duplicate
        .authority_versions
        .push(duplicate.authority_versions[0].clone());
    duplicate.permit_digest = duplicate.digest();
    assert!(duplicate.validate().is_err());
}

use kiana_domain::{
    AggregateVersion, CapabilityKind, CapabilityRequest, DispatchCrashClass, DispatchPermit,
    ExecutionId, InvocationId, RequestContext, RequestId, RuntimeEvent, DISPATCH_PERMIT_SCHEMA,
    DISPATCH_PERMIT_VERSION,
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

#[test]
fn dispatch_permit_rejects_invocation_request_mismatch_even_with_valid_digest() {
    let (mut permit, _) = permit();
    permit.invocation_id = InvocationId::new();
    permit.permit_digest = permit.digest();

    assert_eq!(
        permit.validate().unwrap_err(),
        "dispatch_permit_invocation_request_mismatch"
    );
}

#[test]
fn dispatch_permit_keeps_harness_context_request_independent() {
    let (mut permit, request) = permit();
    permit.context.request_id = RequestId::new();
    permit.permit_digest = permit.digest();

    assert!(permit
        .validate_for_request(&request, &permit.project_identity, 500)
        .is_ok());
}

/// 按 stream version 顺序构造一条 `execution_permit` 流，用于崩溃分类。
fn permit_stream(kinds: &[&str]) -> Vec<RuntimeEvent> {
    kinds
        .iter()
        .enumerate()
        .map(|(index, kind)| {
            RuntimeEvent::new(
                RequestId::new(),
                index as u64 + 1,
                *kind,
                json!({"marker": index as u64 + 1}),
            )
            .unwrap()
            .with_stream_metadata("execution_permit", "execution-1", index as u64 + 1)
        })
        .collect()
}

#[test]
fn dispatch_crash_classification_never_optimistically_claims_no_handler() {
    let not_prepared = kiana_domain::classify_dispatch_stream(&[]);
    assert_eq!(not_prepared, DispatchCrashClass::NotPrepared);
    assert!(!not_prepared.handler_may_have_started());
    assert!(!not_prepared.requires_unknown_terminal());

    // prepared 已提交但派发事务未提交：handler 未被调用，但许可必须补一条 Unknown 终结事实。
    let prepared = kiana_domain::classify_dispatch_stream(&permit_stream(&["execution.prepared"]));
    assert_eq!(prepared, DispatchCrashClass::PreparedUndispatched);
    assert!(!prepared.handler_may_have_started());
    assert_eq!(prepared.requires_unknown_terminal(), true);

    // 只提交 dispatching 而没有 executing 的前缀（消费 CAS 是原子的）不能证明 handler 已开始。
    let partial = kiana_domain::classify_dispatch_stream(&permit_stream(&[
        "execution.prepared",
        "invocation.dispatching",
    ]));
    assert_eq!(partial, DispatchCrashClass::DispatchUnconfirmed);
    assert!(!partial.handler_may_have_started());

    // dispatching + executing 一起提交后，效果对恢复路径而言是未知的。
    let started = kiana_domain::classify_dispatch_stream(&permit_stream(&[
        "execution.prepared",
        "invocation.dispatching",
        "invocation.executing",
    ]));
    assert_eq!(started, DispatchCrashClass::EffectPossiblyStarted);
    assert!(started.handler_may_have_started());
    assert!(!started.is_terminal());

    let terminal = kiana_domain::classify_dispatch_stream(&permit_stream(&[
        "execution.prepared",
        "invocation.dispatching",
        "invocation.executing",
        "execution.result_committed",
    ]));
    assert_eq!(terminal, DispatchCrashClass::ResultCommitted);
    assert!(terminal.handler_may_have_started());
    assert!(terminal.is_terminal());

    // pre-Broker 终结路径：permit 未消费，直接以 Unknown 关账，也属于终态。
    let pre_broker_terminal = kiana_domain::classify_dispatch_stream(&permit_stream(&[
        "execution.prepared",
        "execution.result_committed",
    ]));
    assert_eq!(pre_broker_terminal, DispatchCrashClass::ResultCommitted);
    assert!(pre_broker_terminal.is_terminal());
}

#[test]
fn dispatch_crash_classification_rejects_corrupt_streams() {
    let mut missing_version = permit_stream(&["execution.prepared"]);
    missing_version[0].stream_version = None;
    assert_eq!(
        kiana_domain::classify_dispatch_stream(&missing_version),
        DispatchCrashClass::Corrupt
    );

    let mut skipped_version = permit_stream(&[
        "execution.prepared",
        "invocation.dispatching",
        "invocation.executing",
    ]);
    skipped_version[2].stream_version = Some(9);
    assert_eq!(
        kiana_domain::classify_dispatch_stream(&skipped_version),
        DispatchCrashClass::Corrupt
    );

    // 乱序/重复种类不是合法的派发前缀，必须 fail-closed 而不是回退到“未派发”。
    assert_eq!(
        kiana_domain::classify_dispatch_stream(&permit_stream(&[
            "invocation.dispatching",
            "execution.prepared",
        ])),
        DispatchCrashClass::Corrupt
    );
    assert_eq!(
        kiana_domain::classify_dispatch_stream(&permit_stream(&[
            "execution.prepared",
            "invocation.dispatching",
            "invocation.dispatching",
        ])),
        DispatchCrashClass::Corrupt
    );
    assert_eq!(
        kiana_domain::classify_dispatch_stream(&permit_stream(&[
            "execution.prepared",
            "execution.result_committed",
            "invocation.dispatching",
        ])),
        DispatchCrashClass::Corrupt
    );
}

#[test]
fn dispatch_reservation_digest_binds_snapshot_and_read_set() {
    let (permit, _) = permit();
    let reservation = json!({"budget":{"lease_id":"budget-1"},"cell":{"cell_id":"cell-1"}});
    let digest = kiana_domain::dispatch_reservation_digest(
        &reservation,
        &permit.authority_versions,
    );
    // 同一组快照与 read-set 每次都得到相同摘要（规范化 JSON，无墙钟/随机数）。
    assert_eq!(
        digest,
        kiana_domain::dispatch_reservation_digest(&reservation, &permit.authority_versions)
    );

    // 预留快照变化必须改变绑定摘要。
    assert_ne!(
        digest,
        kiana_domain::dispatch_reservation_digest(
            &json!({"budget":{"lease_id":"budget-2"},"cell":{"cell_id":"cell-1"}}),
            &permit.authority_versions,
        )
    );

    // authority read-set 变化同样必须改变摘要，避免“旧 read-set + 新预留”混用。
    let mut other_read_set = permit.authority_versions.clone();
    other_read_set.push(AggregateVersion::new("run", "run-2", 3));
    assert_ne!(
        digest,
        kiana_domain::dispatch_reservation_digest(&reservation, &other_read_set)
    );
}

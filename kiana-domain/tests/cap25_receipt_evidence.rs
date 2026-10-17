//! CAP-25 receipt 合同夹具：plan evidence 绑定与「无效适配器结果不能渲染成成功」。
//!
//! 路线图卡片：`docs/roadmap/capability.md#step-cap-25`。这些夹具只驱动 Domain 合同：
//! 类型校验、摘要绑定与读回；真实投影与四入口一致性行为在 `kiana-core` 的
//! `receipt_entrypoint_tests` 与 `kiana-daemon` 的 `run_stream` 测试模块中覆盖。

use kiana_domain::{
    AdapterCommitState, AdapterResult, AdapterResultKind, CapabilityExecutionState,
    CapabilityResult, CapabilityResultReceipt, EventId, ExecutionReceipt, ExecutionReceiptEvidence,
    RequestId,
};
use serde_json::json;

fn digest(ch: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (ch as u32) & 0xffff).repeat(16)
    )
}

#[test]
fn plan_evidence_is_bound_into_the_execution_receipt() {
    let request_id = RequestId::new();
    let base = ExecutionReceipt::new(
        request_id,
        None,
        None,
        1,
        digest('a'),
        CapabilityExecutionState::Succeeded,
        true,
        None,
        false,
        None,
        4,
        vec![EventId::new()],
        digest('b'),
        "implemented",
        "source",
    )
    .unwrap();
    let evidence = ExecutionReceiptEvidence {
        descriptor_id: Some("shell.exec@kiana.handler-binding.v1".to_owned()),
        descriptor_digest: Some(digest('d')),
        input_digest: Some(digest('i')),
        scope_digest: Some(digest('s')),
        plan_digest: Some(digest('p')),
        approval_id: Some("approval-1".to_owned()),
        backend: Some("bwrap".to_owned()),
        resource_usage_digest: Some(digest('u')),
        changes_digest: Some(digest('c')),
        result_digest: Some(digest('r')),
        limitations: vec!["stop_unconfirmed".to_owned()],
    };
    let bound = base.clone().with_evidence(evidence).unwrap();
    assert_ne!(base.receipt_digest, bound.receipt_digest);
    assert_eq!(
        bound.descriptor_id.as_deref(),
        Some("shell.exec@kiana.handler-binding.v1")
    );
    assert_eq!(bound.backend.as_deref(), Some("bwrap"));
    assert_eq!(bound.result_digest, Some(digest('r')));
    assert_eq!(
        ExecutionReceipt::from_json(&bound.to_json().unwrap()).unwrap(),
        bound
    );

    let mut tampered = bound.clone();
    tampered.backend = Some("host".to_owned());
    assert_eq!(
        tampered.validate().unwrap_err(),
        "execution_receipt_digest_mismatch"
    );

    // A later projection without the same facts cannot silently drop a bound plan digest.
    let rebound = bound
        .clone()
        .with_evidence(ExecutionReceiptEvidence::default())
        .unwrap();
    assert_eq!(rebound.plan_digest, bound.plan_digest);
    assert_eq!(rebound.scope_digest, bound.scope_digest);
    assert!(rebound.limitations.is_empty());

    // Free-form diagnostics are not valid bounded limitation labels.
    let mut invalid = bound;
    invalid.limitations = vec!["stop unconfirmed".to_owned()];
    invalid.receipt_digest = invalid.digest();
    assert_eq!(
        invalid.validate().unwrap_err(),
        "execution_receipt_header_invalid"
    );
}

#[test]
fn invalid_adapter_result_cannot_be_rendered_as_success() {
    let result = CapabilityResult {
        request_id: RequestId::new(),
        success: true,
        output: json!({
            "effect_known": false,
            "effect_started": true,
            "stdout": "looks successful",
        }),
        evidence_refs: Vec::new(),
    };

    // A handler claiming success while admitting it cannot prove the effect is refused, not
    // relabelled as a successful receipt.
    assert_eq!(
        CapabilityResultReceipt::from_result(&result, None, None, 1, true).unwrap_err(),
        "capability_result_receipt_inconsistent"
    );

    let mut envelope = AdapterResult::from_result(
        &result,
        AdapterResultKind::Shell,
        AdapterCommitState::Unknown,
    )
    .expect("unknown effect envelope derives with a reconciliation fence");
    assert!(envelope.reconciliation_required);
    envelope.reconciliation_required = false;
    envelope.adapter_digest = envelope.digest();
    assert_eq!(
        envelope.validate().unwrap_err(),
        "adapter_result_unknown_not_fenced"
    );
}

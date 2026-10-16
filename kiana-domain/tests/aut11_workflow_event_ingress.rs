//! AUT-11 的 Domain 合同测试：检查有限事件 envelope、来源策略过滤和 occurrence/Fire 材料形状。
//!
//! 这些用例只在内存里构造 JSON，不连接 Webhook，也不持有服务端 HMAC key。
//! `signature` 使用普通占位文本，Domain 的 `from_verified()` 也不会验签；真实 HMAC 路径由
//! `kiana-daemon/tests/aut11_workflow_ingress.rs` 单独覆盖。

use kiana_domain::{
    AutomationCommand, WorkflowEventIngress, WorkflowEventOccurrence, WorkflowEventSourcePolicy,
};
use serde_json::json;

/// 复用一条服务端策略 fixture：只允许订单创建事件及 `order_id`、`amount` 两个顶层字段。
fn policy() -> WorkflowEventSourcePolicy {
    WorkflowEventSourcePolicy::new(
        "orders",
        "orders-key-1",
        "project-1",
        ["order.created".to_owned()],
        ["order_id".to_owned()],
        ["order_id".to_owned(), "amount".to_owned()],
        10_000,
    )
    .expect("valid policy")
}

/// 创建结构和自摘要一致的 Domain 测试 envelope；传入的 signature 只是长度合格的占位值。
fn event(payload: serde_json::Value) -> WorkflowEventIngress {
    WorkflowEventIngress::new(
        "orders",
        "orders-key-1",
        "project-1",
        "trigger-orders",
        "evt-1",
        "order.created",
        1_000,
        payload,
        "signature",
    )
    .expect("valid event")
}

#[test]
fn event_filter_and_occurrence_bind_only_allowlisted_payload_fields() {
    // 这条 happy path 证明的是“DTO 可通过 policy 并投影为 Fire 所需字段”，不是签名已经可信。
    let ingress = event(json!({"order_id":"o-1","amount":3}));
    policy().matches(&ingress, 1_000).expect("filter matches");
    let occurrence =
        WorkflowEventOccurrence::from_verified(&ingress, &policy()).expect("occurrence");
    assert_eq!(occurrence.occurrence_key, "event:orders:evt-1");
    let fire = occurrence
        .to_fire_command("event:committed-1")
        .expect("fire material");
    assert!(matches!(
        fire,
        AutomationCommand::Fire {
            event_ref: Some(_),
            ..
        }
    ));
}

#[test]
fn wrong_kind_missing_field_extra_field_and_digest_tamper_fail_closed() {
    // 身份/类型字段变更后重算 ingress_digest，让用例到达 policy 分支；payload 篡改用例故意不改
    // payload_digest，以验证正文与原始内容摘要不一致时会先被拒绝。
    let mut wrong_kind = event(json!({"order_id":"o-1"}));
    wrong_kind.event_kind = "order.deleted".to_owned();
    wrong_kind.ingress_digest = wrong_kind.digest();
    assert_eq!(
        policy()
            .matches(&wrong_kind, 1_000)
            .expect_err("wrong kind"),
        "workflow_event_source_or_kind_denied"
    );

    let missing = event(json!({"amount":3}));
    assert_eq!(
        policy().matches(&missing, 1_000).expect_err("missing key"),
        "workflow_event_payload_filter_denied"
    );

    let extra = event(json!({"order_id":"o-1","secret":"do-not-forward"}));
    assert_eq!(
        policy().matches(&extra, 1_000).expect_err("extra key"),
        "workflow_event_payload_filter_denied"
    );

    let mut tampered = event(json!({"order_id":"o-1"}));
    tampered.payload = json!({"order_id":"other"});
    assert_eq!(
        tampered.validate().expect_err("payload tamper"),
        "workflow_event_payload_digest_mismatch"
    );
}

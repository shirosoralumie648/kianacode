//! AUT-11 Daemon adapter 测试：用进程内合成 key 验证签名、来源/字段拒绝和同一 verifier 的重复投递。
//!
//! fixture 不连接外部 Webhook，不读取真实密钥配置，也不把 occurrence 追加到 EventLog；它验证的是
//! `WorkflowEventVerifier` 当前实现的 adapter 行为，不证明产品入口已装配或去重可跨进程保存。

use kiana_daemon::WorkflowEventVerifier;
use kiana_domain::{WorkflowEventIngress, WorkflowEventSourcePolicy};
use ring::hmac;
use serde_json::json;

/// 只供本测试签名和验签的固定字节，不是 Kiana 的部署密钥。
const SECRET: &[u8] = b"workflow-event-test-secret-32-bytes";

/// 构造固定 source/key/project 与事件/字段 allow-list，供每个测试建立独立 verifier。
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

/// 先构造一个结构完整的 DTO，并填入格式正确但没有真实 MAC 的零值签名。
fn unsigned_event(payload: serde_json::Value) -> WorkflowEventIngress {
    WorkflowEventIngress::new(
        "orders",
        "orders-key-1",
        "project-1",
        "trigger-orders",
        "evt-1",
        "order.created",
        1_000,
        payload,
        "0000000000000000000000000000000000000000000000000000000000000000",
    )
    .expect("valid event")
}

/// 把 MAC 字节编码成 verifier 所要求的 64 位十六进制小写文本。
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 先用占位签名生成 signing_digest，再用 fixture key 签名，最后重建 envelope 以同步其 ingress_digest。
fn signed_event(payload: serde_json::Value) -> WorkflowEventIngress {
    let draft = unsigned_event(payload);
    let key = hmac::Key::new(hmac::HMAC_SHA256, SECRET);
    let signature = hex(hmac::sign(&key, draft.signing_digest().as_bytes()).as_ref());
    WorkflowEventIngress::new(
        draft.source_id,
        draft.key_id,
        draft.project_id,
        draft.trigger_id,
        draft.event_id,
        draft.event_kind,
        draft.occurred_at_unix_ms,
        draft.payload,
        signature,
    )
    .expect("signed event")
}

#[test]
fn verifier_accepts_one_event_and_deduplicates_same_digest() {
    // 首次验签产生不含原文 payload 的 occurrence；相同 source/event 与 digest 再次到达时返回 None。
    // 这个 fixture 只覆盖同一个 verifier 实例，不能推断重启后仍能去重。
    let verifier = WorkflowEventVerifier::new([(policy(), SECRET.to_vec())]).expect("verifier");
    let event = signed_event(json!({"order_id":"o-1","amount":3}));
    let first = verifier
        .verify(event.clone(), 1_000)
        .expect("verified")
        .expect("new occurrence");
    assert_eq!(first.occurrence_key, "event:orders:evt-1");
    assert!(verifier.verify(event, 1_000).expect("replay").is_none());
}

#[test]
fn verifier_rejects_bad_signature_source_project_and_payload_filter() {
    // 四种输入都应失败，但断言只检查错误类别是 Err；它们没有证明各自命中的具体错误码。
    let verifier = WorkflowEventVerifier::new([(policy(), SECRET.to_vec())]).expect("verifier");

    // 重算无密钥 ingress_digest，让结构自洽检查通过，随后真正失败于 HMAC 验证。
    let mut bad_signature = signed_event(json!({"order_id":"o-1"}));
    bad_signature.signature =
        "0000000000000000000000000000000000000000000000000000000000000000".to_owned();
    bad_signature.ingress_digest = bad_signature.digest();
    assert!(verifier.verify(bad_signature, 1_000).is_err());

    // source 不在 server-owned map 中；验证器会在尝试 HMAC 前拒绝。
    let mut other_source = signed_event(json!({"order_id":"o-2"}));
    other_source.source_id = "untrusted".to_owned();
    other_source.ingress_digest = other_source.digest();
    assert!(verifier.verify(other_source, 1_000).is_err());

    // project 标签不匹配 policy；即使原事件曾由 fixture key 签名，也不能进入后续处理。
    let mut other_project = signed_event(json!({"order_id":"o-3"}));
    other_project.project_id = "project-2".to_owned();
    other_project.ingress_digest = other_project.digest();
    assert!(verifier.verify(other_project, 1_000).is_err());

    // 额外顶层字段不在 payload allow-list；本用例通过签名重建保持摘要自洽。
    let extra = signed_event(json!({"order_id":"o-4","not_allowed":true}));
    assert!(verifier.verify(extra, 1_000).is_err());
}

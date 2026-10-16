//! CM-29 投影滞后与未知 mutation 恢复合同测试。
//!
//! 这里使用小型合成 cursor、generation、epoch 和摘要，验证 view 状态派生与原 mutation ID/request digest 绑定。
//! 测试不会查询生产 EventLog、幂等记录或真实 mutation 结果。

use kiana_domain::{
    json_digest, ProjectionLagStatus, ProjectionLagView, UnknownMutationReconciliation,
};
use serde_json::json;

/// 把 fixture 文本包成 JSON 后计算 SHA-256，生成符合 DTO 外形的测试摘要。
fn digest(value: &str) -> String {
    json_digest(&json!({"value": value}))
}

/// 验证投影落后与 cursor 缺失分别成为 Pending/Unknown，且都不能作为一致读模型。
#[test]
fn projection_lag_is_visible() {
    let lag = ProjectionLagView::new(
        10,
        Some(7),
        3,
        9,
        Some("projection_cursor_lagging".to_owned()),
    )
    .unwrap();
    assert_eq!(lag.status, ProjectionLagStatus::Pending);
    assert!(lag.projection_pending);
    assert!(!lag.read_consistent());
    lag.validate().unwrap();

    let unknown = ProjectionLagView::new(
        10,
        None,
        3,
        9,
        Some("projection_cursor_unobserved".to_owned()),
    )
    .unwrap();
    assert_eq!(unknown.status, ProjectionLagStatus::Unknown);
    assert!(unknown.projection_pending);
}

/// 验证未知 mutation 恢复只能沿用原 ID 和原 request digest 继续核查。
///
/// reconcile 成功只代表两个输入相同；并不证明真实请求已经完成状态查询或可以重放。
#[test]
fn unknown_mutation_is_not_retried_with_new_id() {
    let reconciliation =
        UnknownMutationReconciliation::new("mutation:original", digest("request")).unwrap();
    reconciliation
        .reconcile("mutation:original", &digest("request"))
        .unwrap();
    assert_eq!(
        reconciliation
            .reconcile("mutation:new", &digest("request"))
            .unwrap_err(),
        "unknown_mutation_new_id_forbidden"
    );
    assert_eq!(
        reconciliation
            .reconcile("mutation:original", &digest("different"))
            .unwrap_err(),
        "unknown_mutation_request_digest_mismatch"
    );
}

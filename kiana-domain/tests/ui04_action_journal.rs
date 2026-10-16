//! UI-04 action journal 的 Domain 层行为样例。
//!
//! 这些测试直接驱动内存中的 [`UiActionJournal`]，聚焦 command 身份、快照前置条件、终态和 receipt
//! 之间的合同：
//!
//! ```text
//! 构造带摘要的 UiActionCommand
//!     -> 首次 accept 对照给定 authority 快照
//!     -> 重复 key 返回原 record，或拒绝冲突/过期输入
//!     -> apply / unknown 验证 effect 结果边界
//! ```
//!
//! 这里不启动 ControlPlane、EventStore、UI handler 或真实外部 effect；因此只能说明纯 Domain reducer
//! 在这些合成输入下的行为，不能单独证明持久化 CAS、调用者身份绑定或端到端 UI 操作安全。

use kiana_domain::{json_digest, RequestId, UiActionCommand, UiActionJournal, UiActionState};
use serde_json::json;

/// 为每个场景构造结构相同、身份 key 和 payload 可替换的请求。
///
/// owner、scope、epoch、cursor、revision 与 deadline 使用固定 fixture 值；各测试通过 accept 的服务端
/// 快照参数改变其中一项，隔离验证拒绝原因。`RequestId::new()` 让每次 helper 调用拥有新的命令 ID。
fn command(key: &str, payload: serde_json::Value) -> UiActionCommand {
    UiActionCommand::new(
        RequestId::new(),
        key,
        "run-target",
        "owner-1",
        json_digest(&json!({"scope":"run-1"})),
        "epoch-1",
        0,
        Some(4),
        10_000,
        payload,
    )
    .unwrap()
}

#[test]
/// 相同幂等请求重放时返回原 Accepted 记录；后续只应用一次 effect。
///
/// 第二次 accept 故意传入已变化的 cursor/revision，验证 journal 先识别已接受的原命令，再走新请求的
/// 快照新鲜度检查；重复 apply 保留 effect_count=1。这里验证的是 reducer 规则，不是真实 effect 次数。
fn action_journal_replays_exact_key_without_second_effect() {
    let mut journal = UiActionJournal::new();
    let action = command("ui-04-key", json!({"decision":"approve"}));
    let (accepted, replayed) = journal
        .accept(
            &action,
            "epoch-1",
            0,
            Some(4),
            "owner-1",
            &action.scope_digest,
            1_000,
        )
        .unwrap();
    assert!(!replayed);
    assert_eq!(accepted.state, UiActionState::Accepted);
    assert_eq!(accepted.effect_count, 0);

    let (same, replayed) = journal
        .accept(
            &action,
            "epoch-1",
            9,
            Some(99),
            "owner-1",
            &action.scope_digest,
            2_000,
        )
        .unwrap();
    assert!(replayed);
    assert_eq!(same, accepted);
    let applied = journal
        .apply(
            &action.idempotency_key,
            json_digest(&json!({"receipt":1})),
            2_001,
        )
        .unwrap();
    assert_eq!(applied.state, UiActionState::Applied);
    assert_eq!(applied.effect_count, 1);
    let applied_again = journal
        .apply(
            &action.idempotency_key,
            applied.receipt_digest.clone().unwrap(),
            2_002,
        )
        .unwrap();
    assert_eq!(applied_again.effect_count, 1);
}

#[test]
/// 同一 key 不能换 payload；新 key 则逐项检查 epoch、cursor、revision 和 owner 前置条件。
///
/// scope 参数在本测试始终取自各自 command，因此函数名中的 scope 冲突没有被单独触发；deadline 过期也
/// 没有覆盖。错误断言确认失败分支返回稳定错误标签，而不是依赖错误文本中的实现细节。
fn action_journal_rejects_changed_digest_owner_scope_and_cas() {
    let action = command("ui-04-conflict", json!({"decision":"approve"}));
    let mut journal = UiActionJournal::new();
    journal
        .accept(
            &action,
            "epoch-1",
            0,
            Some(4),
            "owner-1",
            &action.scope_digest,
            1_000,
        )
        .unwrap();

    let changed = command("ui-04-conflict", json!({"decision":"deny"}));
    assert_eq!(
        journal
            .accept(
                &changed,
                "epoch-1",
                0,
                Some(4),
                "owner-1",
                &changed.scope_digest,
                1_000,
            )
            .unwrap_err(),
        "ui_action_idempotency_digest_mismatch"
    );
    let stale = command("ui-04-stale", json!({"decision":"approve"}));
    assert_eq!(
        journal
            .accept(
                &stale,
                "epoch-old",
                0,
                Some(4),
                "owner-1",
                &stale.scope_digest,
                1_000,
            )
            .unwrap_err(),
        "ui_action_stale_epoch"
    );
    assert_eq!(
        journal
            .accept(
                &stale,
                "epoch-1",
                8,
                Some(4),
                "owner-1",
                &stale.scope_digest,
                1_000,
            )
            .unwrap_err(),
        "ui_action_stale_cursor"
    );
    assert_eq!(
        journal
            .accept(
                &stale,
                "epoch-1",
                0,
                Some(5),
                "owner-1",
                &stale.scope_digest,
                1_000,
            )
            .unwrap_err(),
        "ui_action_stale_revision"
    );
    assert_eq!(
        journal
            .accept(
                &stale,
                "epoch-1",
                0,
                Some(4),
                "other-owner",
                &stale.scope_digest,
                1_000,
            )
            .unwrap_err(),
        "ui_action_owner_mismatch"
    );
}

#[test]
/// `Unknown` 必须留在原 key 上供查询，且不能再被普通 apply 当成“尚未执行”。
///
/// 场景模拟 ACK 丢失后无法确认目标是否完成：状态记作 Unknown、effect_count 维持 0，然后验证 apply 被
/// 拒绝。fixture 不模拟目标端是否实际产生了副作用。
fn unknown_requires_original_key_and_never_becomes_applied() {
    let action = command("ui-04-unknown", json!({"decision":"approve"}));
    let mut journal = UiActionJournal::new();
    journal
        .accept(
            &action,
            "epoch-1",
            0,
            Some(4),
            "owner-1",
            &action.scope_digest,
            1_000,
        )
        .unwrap();
    let unknown = journal
        .unknown(&action.idempotency_key, "ack_lost")
        .unwrap();
    assert_eq!(unknown.state, UiActionState::Unknown);
    assert_eq!(unknown.effect_count, 0);
    assert_eq!(
        journal
            .apply(
                &action.idempotency_key,
                json_digest(&json!({"receipt":1})),
                1_001
            )
            .unwrap_err(),
        "ui_action_result_unknown"
    );
    assert!(journal.query_original("ui-04-unknown").is_some());
}

#[test]
/// receipt 只允许出现在 Applied record，并且必须保持有效摘要格式。
///
/// 测试分别篡改已应用记录的摘要、移除 receipt，以及把 receipt 塞入 Accepted 记录，确认 validator
/// 会按状态合同拒绝三种不一致；record digest 的故意失配并非这里验证的错误，因为 validator 的状态检查
/// 会先返回更具体的 receipt 错误。
fn action_record_receipt_digest_is_bound_to_applied_state() {
    let action = command("ui-04-receipt-fence", json!({"decision":"approve"}));
    let mut journal = UiActionJournal::new();
    journal
        .accept(
            &action,
            "epoch-1",
            0,
            Some(4),
            "owner-1",
            &action.scope_digest,
            1_000,
        )
        .unwrap();
    let mut accepted = journal
        .query_original("ui-04-receipt-fence")
        .cloned()
        .expect("accepted record");
    let applied = journal
        .apply(
            &action.idempotency_key,
            json_digest(&json!({"receipt": 1})),
            1_001,
        )
        .unwrap();

    let mut malformed = applied.clone();
    malformed.receipt_digest = Some("not-a-digest".to_owned());
    assert_eq!(
        malformed.validate_for_query().unwrap_err(),
        "ui_action_receipt_digest_invalid"
    );

    let mut missing = applied;
    missing.receipt_digest = None;
    assert_eq!(
        missing.validate_for_query().unwrap_err(),
        "ui_action_applied_receipt_missing"
    );

    accepted.receipt_digest = Some(json_digest(&json!({"receipt": 2})));
    assert_eq!(
        accepted.validate_for_query().unwrap_err(),
        "ui_action_receipt_state_mismatch"
    );
}

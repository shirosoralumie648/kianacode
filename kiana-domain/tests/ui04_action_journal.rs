//! UI-04 action journal 的 Domain 层行为样例。
//!
//! 这些测试直接驱动内存中的 [`UiActionJournal`]，聚焦 command 身份、服务端 authority 绑定、快照前置
//! 条件、终态、receipt 和重投策略之间的合同：
//!
//! ```text
//! 构造带摘要的 UiActionCommand
//!     -> 首次 accept 对照服务端 UiActionAuthoritySnapshot
//!     -> 重复 key 返回原 record，或拒绝冲突/过期/缺 permit 输入
//!     -> apply / unknown 验证 effect 结果边界
//!     -> record 说明准入基础与重投策略
//! ```
//!
//! 这里不启动 ControlPlane、EventStore、UI handler 或真实外部 effect；因此只能说明纯 Domain reducer
//! 在这些合成输入下的行为，不能单独证明持久化 CAS、调用者身份绑定或端到端 UI 操作安全。

use kiana_domain::{
    json_digest, RequestId, UiActionAuthoritySnapshot, UiActionCommand, UiActionJournal,
    UiActionResubmitPolicy, UiActionState,
};
use serde_json::json;

/// 为每个场景构造结构相同、身份 key 和 payload 可替换的请求。
///
/// owner、scope、epoch、cursor、revision 与 deadline 使用固定 fixture 值；各测试通过 authority
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

/// 与 [`command`] 的 expected 值一致的服务端 authority 快照；permit 默认存在。
fn authority() -> UiActionAuthoritySnapshot {
    UiActionAuthoritySnapshot {
        authority_epoch: "epoch-1".to_owned(),
        cursor: 0,
        target_revision: Some(4),
        owner_id: "owner-1".to_owned(),
        scope_digest: json_digest(&json!({"scope":"run-1"})),
        permit_digest: Some(json_digest(&json!({"permit":"ui-04"}))),
        cancelled: false,
    }
}

#[test]
/// 相同幂等请求重放时返回原 Accepted 记录；后续只应用一次 effect。
///
/// 第二次 accept 故意传入已变化的 cursor/revision，并要求快照缺少 permit：原命令重放必须发生在
/// 新请求的新鲜度/permit 检查之前，否则响应丢失后的对账会因为当前快照变化而永远失败。重复 apply
/// 保留 effect_count=1。这里验证的是 reducer 规则，不是真实 effect 次数。
fn action_journal_replays_exact_key_without_second_effect() {
    let mut journal = UiActionJournal::new();
    let action = command("ui-04-key", json!({"decision":"approve"}));
    let (accepted, replayed) = journal.accept(&action, &authority(), 1_000).unwrap();
    assert!(!replayed);
    assert_eq!(accepted.state, UiActionState::Accepted);
    assert_eq!(accepted.effect_count, 0);
    assert_eq!(accepted.command_digest, action.command_digest);

    let mut changed = authority();
    changed.cursor = 9;
    changed.target_revision = Some(99);
    changed.permit_digest = None;
    let (same, replayed) = journal.accept(&action, &changed, 2_000).unwrap();
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
/// 同一 key 不能换 payload；新 key 则逐项检查 epoch、cursor、revision、owner 和 scope 前置条件。
///
/// 每个拒绝分支都使用独立 authority 快照篡改一项，确认失败原因是稳定错误标签，而不是错误文本里的
/// 实现细节；deadline 过期由单独用例覆盖。
fn action_journal_rejects_changed_digest_owner_scope_and_cas() {
    let action = command("ui-04-conflict", json!({"decision":"approve"}));
    let mut journal = UiActionJournal::new();
    journal.accept(&action, &authority(), 1_000).unwrap();

    let changed = command("ui-04-conflict", json!({"decision":"deny"}));
    assert_eq!(
        journal.accept(&changed, &authority(), 1_000).unwrap_err(),
        "ui_action_idempotency_digest_mismatch"
    );

    let stale = command("ui-04-stale", json!({"decision":"approve"}));
    let mut stale_epoch = authority();
    stale_epoch.authority_epoch = "epoch-old".to_owned();
    assert_eq!(
        journal.accept(&stale, &stale_epoch, 1_000).unwrap_err(),
        "ui_action_stale_epoch"
    );
    let mut stale_cursor = authority();
    stale_cursor.cursor = 8;
    assert_eq!(
        journal.accept(&stale, &stale_cursor, 1_000).unwrap_err(),
        "ui_action_stale_cursor"
    );
    let mut stale_revision = authority();
    stale_revision.target_revision = Some(5);
    assert_eq!(
        journal.accept(&stale, &stale_revision, 1_000).unwrap_err(),
        "ui_action_stale_revision"
    );
    let mut foreign_owner = authority();
    foreign_owner.owner_id = "other-owner".to_owned();
    assert_eq!(
        journal.accept(&stale, &foreign_owner, 1_000).unwrap_err(),
        "ui_action_owner_mismatch"
    );
    let mut foreign_scope = authority();
    foreign_scope.scope_digest = json_digest(&json!({"scope":"run-2"}));
    assert_eq!(
        journal.accept(&stale, &foreign_scope, 1_000).unwrap_err(),
        "ui_action_scope_mismatch"
    );
}

#[test]
/// 缺少服务端 permit、目标取消中或已过 deadline 的新命令必须被拒绝且不产生记录。
///
/// 三种失败都发生在建立 Accepted record 之前：journal 里不应出现该幂等键的任何痕迹，调用方也就
/// 不可能把它误当成"已经受理、只是结果未知"。
fn new_admission_requires_permit_and_live_target() {
    let mut journal = UiActionJournal::new();

    let no_permit = command("ui-04-no-permit", json!({"decision":"approve"}));
    let mut missing_permit = authority();
    missing_permit.permit_digest = None;
    assert_eq!(
        journal
            .accept(&no_permit, &missing_permit, 1_000)
            .unwrap_err(),
        "ui_action_permit_required"
    );
    assert!(journal.query_original("ui-04-no-permit").is_none());

    let cancelled = command("ui-04-cancelled", json!({"decision":"approve"}));
    let mut cancelling = authority();
    cancelling.cancelled = true;
    assert_eq!(
        journal.accept(&cancelled, &cancelling, 1_000).unwrap_err(),
        "ui_action_cancelled"
    );
    assert!(journal.query_original("ui-04-cancelled").is_none());

    let expired = command("ui-04-expired", json!({"decision":"approve"}));
    assert_eq!(
        journal.accept(&expired, &authority(), 10_001).unwrap_err(),
        "ui_action_deadline_expired"
    );
    assert!(journal.query_original("ui-04-expired").is_none());
    // 恰好等于 deadline 时仍然接受；边界属于调用方的时钟，不属于这里。
    journal.accept(&expired, &authority(), 10_000).unwrap();
}

#[test]
/// 记录保留准入时的服务端 authority 基础与 permit，篡改其中任何一项都会被 record digest 检出。
///
/// 这保证查询结果能回答"是谁、在哪个 epoch/cursor/revision 上、凭哪个 permit 准入的"，而不是只重复
/// 客户端自己声明的 expected 值。
fn accepted_record_binds_the_admitting_authority_and_permit() {
    let action = command("ui-04-binding", json!({"decision":"approve"}));
    let mut journal = UiActionJournal::new();
    let mut snapshot = authority();
    snapshot.cursor = 0;
    snapshot.target_revision = Some(4);
    let permit = snapshot.permit_digest.clone().unwrap();
    let (accepted, _) = journal.accept(&action, &snapshot, 1_000).unwrap();

    assert_eq!(accepted.authority_epoch, "epoch-1");
    assert_eq!(accepted.authority_cursor, 0);
    assert_eq!(accepted.authority_revision, Some(4));
    assert_eq!(accepted.permit_digest, permit);
    assert_eq!(accepted.owner_id, "owner-1");

    let mut tampered = accepted.clone();
    tampered.permit_digest = json_digest(&json!({"permit":"other"}));
    assert_eq!(
        tampered.validate_for_query().unwrap_err(),
        "ui_action_record_digest_mismatch"
    );

    let mut tampered_epoch = accepted;
    tampered_epoch.authority_epoch = "epoch-2".to_owned();
    assert_eq!(
        tampered_epoch.validate_for_query().unwrap_err(),
        "ui_action_record_digest_mismatch"
    );
}

#[test]
/// 重投策略没有"换新 ID 自动重做"这一档：未确认状态只能按原键查询，终态不允许再驱动。
fn resubmit_policy_never_authorizes_a_new_command_id() {
    let action = command("ui-04-policy", json!({"decision":"approve"}));
    let mut journal = UiActionJournal::new();
    let (accepted, _) = journal.accept(&action, &authority(), 1_000).unwrap();
    assert_eq!(
        accepted.resubmit_policy(),
        UiActionResubmitPolicy::QueryOriginal
    );

    let unknown = journal
        .unknown(&action.idempotency_key, "ack_lost")
        .unwrap();
    assert_eq!(
        unknown.resubmit_policy(),
        UiActionResubmitPolicy::QueryOriginal
    );

    let mut applied_journal = UiActionJournal::new();
    applied_journal
        .accept(&action, &authority(), 1_000)
        .unwrap();
    let applied = applied_journal
        .apply(
            &action.idempotency_key,
            json_digest(&json!({"receipt":1})),
            1_001,
        )
        .unwrap();
    assert_eq!(
        applied.resubmit_policy(),
        UiActionResubmitPolicy::DoNotRetry
    );

    let rejected = journal
        .reject(&action.idempotency_key, "target_revoked")
        .unwrap_err();
    assert_eq!(rejected, "ui_action_terminal_conflict");
    let rejected_action = command("ui-04-policy-rejected", json!({"decision":"deny"}));
    journal
        .accept(&rejected_action, &authority(), 2_000)
        .unwrap();
    let rejected = journal
        .reject(&rejected_action.idempotency_key, "target_revoked")
        .unwrap();
    assert_eq!(
        rejected.resubmit_policy(),
        UiActionResubmitPolicy::DoNotRetry
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
    journal.accept(&action, &authority(), 1_000).unwrap();
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
    journal.accept(&action, &authority(), 1_000).unwrap();
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

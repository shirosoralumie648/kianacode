//! AUT-21 启动恢复事实的拒绝优先夹具。
//!
//! 顺序：先证明每一条恢复屏障都能稳定拒绝 `Ready`，再证明合法 `Ready` 和
//! `ReconcileRequired` 可以通过。所有事实都由测试本地构造；没有 EventStore、模型或 worker。

use kiana_domain::{
    AutomationBootJournal, AutomationBootLease, AutomationBootRecoveryFact,
    AutomationBootRecoveryState, AutomationReconcileCase, AUTOMATION_BOOT_RECOVERY_SCHEMA,
    JOURNAL_HEADER_SCHEMA, MAX_AUTOMATION_RECONCILE_CASES,
};

fn ready() -> AutomationBootRecoveryFact {
    AutomationBootRecoveryFact {
        schema: AUTOMATION_BOOT_RECOVERY_SCHEMA.to_owned(),
        source_cursor: 10,
        projection_cursor: 10,
        projection_generation: 2,
        authority_epoch: 4,
        observed_authority_epoch: 4,
        pending_unknown_count: 0,
        stale_index: false,
        lease: AutomationBootLease::absent(),
        journal: AutomationBootJournal::healthy(),
        state: AutomationBootRecoveryState::Ready,
        reconcile_cases: Vec::new(),
        recovery_digest: String::new(),
    }
    .sealed()
}

fn case(source_cursor: u64, generation: u64, epoch: u64, unknown: u32) -> AutomationReconcileCase {
    AutomationReconcileCase {
        source_cursor,
        projection_generation: generation,
        authority_epoch: epoch,
        pending_unknown_count: unknown,
    }
}

fn expect(fact: AutomationBootRecoveryFact, reason: &'static str) {
    // 先断言摘要本身与字段自洽；拒绝必须来自状态谓词，而不是偶然的摘要不匹配。
    assert_eq!(fact.recovery_digest, fact.digest());
    assert_eq!(fact.validate(), Err(reason));
}

#[test]
fn lagging_projection_is_refused_even_when_everything_else_is_clean() {
    // AUT-21 的核心缺陷回归：旧夹具把 projection 直接设为 source_cursor，从未真正测试
    // 「投影落后」这条路径。这里让投影停在 9/10、其他字段全部干净。
    let mut lagging = ready();
    lagging.projection_cursor = 9;
    expect(
        lagging.sealed(),
        "automation_boot_recovery_projection_lagging",
    );

    // 投影也不能超前于 source。
    let mut ahead = ready();
    ahead.projection_cursor = 11;
    expect(ahead.sealed(), "automation_boot_recovery_invalid");
}

#[test]
fn pending_unknown_and_open_cases_block_ready() {
    let mut unknown = ready();
    unknown.pending_unknown_count = 1;
    unknown.reconcile_cases = vec![case(10, 2, 4, 1)];
    expect(unknown.sealed(), "automation_boot_recovery_unknown_pending");

    // 有 case 但总数对不上：结构层就拒绝，不会走到状态谓词。
    let mut mismatched = ready();
    mismatched.pending_unknown_count = 2;
    mismatched.reconcile_cases = vec![case(10, 2, 4, 1)];
    expect(
        mismatched.sealed(),
        "automation_boot_recovery_reconcile_case_total_mismatch",
    );

    // typed case 的形状必须完整：零 Unknown 的 case 是伪造的对账项。
    let mut empty_case = ready();
    empty_case.pending_unknown_count = 0;
    empty_case.reconcile_cases = vec![case(10, 2, 4, 0)];
    expect(
        empty_case.sealed(),
        "automation_boot_recovery_reconcile_case_invalid",
    );

    // case 不能声称处理了未来的 source cursor / generation / epoch。
    let mut future = ready();
    future.pending_unknown_count = 1;
    future.reconcile_cases = vec![case(11, 2, 4, 1)];
    expect(
        future.sealed(),
        "automation_boot_recovery_reconcile_case_future",
    );

    let mut too_many = ready();
    too_many.pending_unknown_count = u32::try_from(MAX_AUTOMATION_RECONCILE_CASES + 1).unwrap();
    too_many.reconcile_cases = (0..=MAX_AUTOMATION_RECONCILE_CASES)
        .map(|_| case(10, 2, 4, 1))
        .collect();
    expect(
        too_many.sealed(),
        "automation_boot_recovery_reconcile_case_limit",
    );
}

#[test]
fn stale_and_active_leases_block_ready() {
    let mut stale = ready();
    stale.lease = AutomationBootLease {
        lease_id: Some("instance-a".to_owned()),
        expires_at_unix_ms: 1_000,
        observed_at_unix_ms: 1_000,
    };
    expect(stale.sealed(), "automation_boot_recovery_stale_lease");

    let mut active = ready();
    active.lease = AutomationBootLease {
        lease_id: Some("instance-a".to_owned()),
        expires_at_unix_ms: 2_000,
        observed_at_unix_ms: 1_000,
    };
    expect(active.sealed(), "automation_boot_recovery_lease_active");

    // 形状破坏优先于 stale 判定：没有到期时间的租约不是可解释的租约。
    let mut shapeless = ready();
    shapeless.lease = AutomationBootLease {
        lease_id: Some("instance-a".to_owned()),
        expires_at_unix_ms: 0,
        observed_at_unix_ms: 1_000,
    };
    expect(shapeless.sealed(), "automation_boot_recovery_lease_invalid");

    // 无租约却带时间戳同样拒绝。
    let mut timestamped_absence = ready();
    timestamped_absence.lease = AutomationBootLease {
        lease_id: None,
        expires_at_unix_ms: 1_000,
        observed_at_unix_ms: 1_000,
    };
    expect(
        timestamped_absence.sealed(),
        "automation_boot_recovery_lease_invalid",
    );
}

#[test]
fn journal_corruption_and_schema_mismatch_block_ready() {
    let mut corrupt = ready();
    corrupt.journal = AutomationBootJournal {
        schema: JOURNAL_HEADER_SCHEMA.to_owned(),
        healthy: false,
    };
    expect(corrupt.sealed(), "automation_boot_recovery_journal_corrupt");

    let mut drifted = ready();
    drifted.journal = AutomationBootJournal {
        schema: "kiana.journal-header.v1".to_owned(),
        healthy: true,
    };
    expect(
        drifted.sealed(),
        "automation_boot_recovery_journal_schema_mismatch",
    );

    let mut empty = ready();
    empty.journal = AutomationBootJournal {
        schema: "  ".to_owned(),
        healthy: true,
    };
    expect(empty.sealed(), "automation_boot_recovery_journal_invalid");
}

#[test]
fn old_and_ahead_authority_epochs_block_ready() {
    let mut old = ready();
    old.authority_epoch = 3;
    expect(
        old.sealed(),
        "automation_boot_recovery_authority_epoch_stale",
    );

    let mut ahead = ready();
    ahead.authority_epoch = 5;
    expect(
        ahead.sealed(),
        "automation_boot_recovery_authority_epoch_ahead",
    );

    // 观测 epoch 本身为 0 表示启动路径没有拿到授权事实，不能成为 Ready 依据。
    let mut unobserved = ready();
    unobserved.observed_authority_epoch = 0;
    expect(unobserved.sealed(), "automation_boot_recovery_invalid");
}

#[test]
fn stale_index_and_structural_damage_fail_closed() {
    let mut stale_index = ready();
    stale_index.stale_index = true;
    expect(stale_index.sealed(), "automation_boot_recovery_index_stale");

    let mut unknown_field = serde_json::to_value(ready()).unwrap();
    unknown_field["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<AutomationBootRecoveryFact>(unknown_field).is_err());

    // 摘要与实际字段不一致时拒绝；不能用 self-reported digest 代替校验。
    let mut forged = ready();
    forged.projection_cursor = 9;
    assert_eq!(
        forged.validate(),
        Err("automation_boot_recovery_invalid"),
        "a stale digest must not hide a lagging projection"
    );
}

#[test]
fn blocked_facts_keep_their_evidence_but_never_become_ready() {
    // Blocked 是证据容器：允许携带损坏 journal、旧 epoch 和过期租约。
    let mut blocked = ready();
    blocked.state = AutomationBootRecoveryState::Blocked;
    blocked.journal = AutomationBootJournal {
        schema: "kiana.journal-header.v1".to_owned(),
        healthy: false,
    };
    blocked.authority_epoch = 1;
    blocked.lease = AutomationBootLease {
        lease_id: Some("crashed-instance".to_owned()),
        expires_at_unix_ms: 10,
        observed_at_unix_ms: 20,
    };
    let blocked = blocked.sealed();
    assert!(blocked.validate().is_ok());

    // 同样的证据只要声称 Ready，就必须被拒绝。
    let mut liar = blocked.clone();
    liar.state = AutomationBootRecoveryState::Ready;
    assert!(liar.sealed().validate().is_err());

    // 结构损坏即便在 Blocked 里也不能通过。
    let mut damaged = blocked;
    damaged.source_cursor = 0;
    expect(damaged.sealed(), "automation_boot_recovery_invalid");
}

#[test]
fn ready_and_reconcile_required_success_cases_stay_visible() {
    // 成功夹具在全部拒绝夹具之后。
    assert!(ready().validate().is_ok());

    let mut rebuilding = ready();
    rebuilding.state = AutomationBootRecoveryState::Rebuilding;
    // 重启默认阶段允许投影尚未追平；它不等于可 dispatch。
    rebuilding.projection_cursor = 9;
    let rebuilding = rebuilding.sealed();
    assert!(rebuilding.validate().is_ok());
    assert_ne!(rebuilding.state, AutomationBootRecoveryState::Ready);

    let mut reconcile = ready();
    reconcile.state = AutomationBootRecoveryState::ReconcileRequired;
    reconcile.pending_unknown_count = 3;
    reconcile.projection_cursor = 9;
    reconcile.reconcile_cases = vec![case(9, 2, 4, 3)];
    let reconcile = reconcile.sealed();
    assert!(reconcile.validate().is_ok());

    // ReconcileRequired 没有任何待对账证据时不成立。
    let mut empty_reconcile = ready();
    empty_reconcile.state = AutomationBootRecoveryState::ReconcileRequired;
    expect(empty_reconcile.sealed(), "automation_boot_recovery_invalid");
}

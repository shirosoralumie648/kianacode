//! AUT-18 先拒绝优先夹具：signal 事实 store 的摘要 CAS、signal_id 去重、序号连续、状态转移
//! 白名单、身份绑定、authority epoch 单调与审批单次消费。
//!
//! 夹具只驱动进程内 `MemoryAutomationSignalStore`；它不暂停/恢复 worker、不执行 effect，也不代表
//! durable 语义。

use kiana_domain::{AutomationSignalFact, AutomationSignalState};
use kiana_eventlog::MemoryAutomationSignalStore;
use kiana_ports::{AutomationSignalStore, PortError};

const DEFINITION: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const ACTION: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const PATH: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const OTHER_ACTION: &str =
    "sha256:5555555555555555555555555555555555555555555555555555555555555555";
const APPROVAL: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const FOREIGN_APPROVAL: &str =
    "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

/// 构造一条默认绑定的 signal 事实；`approval_digest` 只在恢复/消费时给出。
fn fact(
    state: AutomationSignalState,
    signal_id: &str,
    sequence: u64,
    epoch: u64,
    checkpoint_sequence: u64,
    approval_digest: Option<&str>,
) -> AutomationSignalFact {
    AutomationSignalFact::new(
        "execution-1",
        signal_id,
        sequence,
        "owner-1",
        DEFINITION,
        ACTION,
        PATH,
        epoch,
        checkpoint_sequence,
        state,
        approval_digest.map(str::to_owned),
    )
    .expect("signal fixture must be self-consistent")
}

fn conflict(reason: &str) -> String {
    PortError::Conflict(reason.to_owned()).to_string()
}

fn failed(reason: &str) -> String {
    PortError::Failed(reason.to_owned()).to_string()
}

/// 记录一条 pause 并返回其摘要，作为后续 CAS 的期望值。
async fn pause(store: &MemoryAutomationSignalStore) -> String {
    let paused = fact(AutomationSignalState::Paused, "signal-1", 1, 4, 5, None);
    let commit = store
        .commit_signal(paused.clone(), None)
        .await
        .expect("first pause must commit");
    assert!(!commit.replayed);
    assert_eq!(commit.fact.signal_digest, paused.signal_digest);
    paused.signal_digest
}

#[tokio::test]
async fn first_commit_requires_the_absent_history_and_a_paused_fact() {
    let store = MemoryAutomationSignalStore::new();
    // 历史为空时 expected_digest 必须是 None；假装“已有事实”一律拒绝。
    assert_eq!(
        store
            .commit_signal(
                fact(AutomationSignalState::Paused, "signal-1", 1, 4, 5, None),
                Some(ACTION)
            )
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_digest_conflict")
    );
    // 第一条事实必须是序号 1 的 Paused；跳号或直接恢复都拒绝。
    assert_eq!(
        store
            .commit_signal(
                fact(AutomationSignalState::Paused, "signal-2", 2, 4, 5, None),
                None
            )
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_sequence_gap")
    );
    assert_eq!(
        store
            .commit_signal(
                fact(
                    AutomationSignalState::Resumed,
                    "signal-3",
                    1,
                    4,
                    5,
                    Some(APPROVAL)
                ),
                None
            )
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_transition_refused")
    );
    // 形状不自洽的事实不会写入存储。
    let mut invalid = fact(AutomationSignalState::Paused, "signal-4", 1, 4, 5, None);
    invalid.signal_sequence = 0;
    assert_eq!(
        store
            .commit_signal(invalid, None)
            .await
            .unwrap_err()
            .to_string(),
        failed("automation_signal_fact_invalid")
    );
    assert!(store.fetch_signals("execution-1").await.unwrap().is_empty());
}

#[tokio::test]
async fn identical_replays_return_the_stored_copy_without_rewriting() {
    let store = MemoryAutomationSignalStore::new();
    let paused = pause(&store).await;
    // 同 signal_id 同内容：安全重放，不要求 CAS 期望值、不追加历史。
    let replay = store
        .commit_signal(
            fact(AutomationSignalState::Paused, "signal-1", 1, 4, 5, None),
            None,
        )
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.fact.signal_digest, paused);
    // 同 signal_id 不同内容：冲突，历史不可改写。
    let mut conflicting = fact(AutomationSignalState::Paused, "signal-1", 1, 4, 5, None);
    conflicting.checkpoint_sequence = 6;
    conflicting.recompute_digest();
    assert_eq!(
        store
            .commit_signal(conflicting, Some(&paused))
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_signal_conflict")
    );
    let history = store.fetch_signals("execution-1").await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].checkpoint_sequence, 5);
}

#[tokio::test]
async fn signal_sequence_regression_and_gaps_are_refused() {
    let store = MemoryAutomationSignalStore::new();
    let paused = pause(&store).await;
    // 先推进到序号 2，才能观察到“回退到已记录序号”（最小合法序号是 1）。
    let resumed = store
        .commit_signal(
            fact(
                AutomationSignalState::Resumed,
                "signal-2",
                2,
                4,
                5,
                Some(APPROVAL),
            ),
            Some(&paused),
        )
        .await
        .unwrap();
    let current = resumed.fact.signal_digest;
    // 回退：使用全新 signal_id，但序号回到已记录的 1。
    assert_eq!(
        store
            .commit_signal(
                fact(AutomationSignalState::Paused, "signal-back", 1, 4, 5, None),
                Some(&current)
            )
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_sequence_regression")
    );
    // 跳号：下一个合法序号是 3，提交 4。
    assert_eq!(
        store
            .commit_signal(
                fact(AutomationSignalState::Paused, "signal-gap", 4, 4, 5, None),
                Some(&current)
            )
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_sequence_gap")
    );
    // CAS 期望值过期（None 或传别的摘要）同样拒绝，且历史不变。
    for expected in [None, Some(ACTION)] {
        assert_eq!(
            store
                .commit_signal(
                    fact(AutomationSignalState::Paused, "signal-cas", 3, 4, 5, None),
                    expected
                )
                .await
                .unwrap_err()
                .to_string(),
            conflict("automation_signal_digest_conflict")
        );
    }
    assert_eq!(store.fetch_signals("execution-1").await.unwrap().len(), 2);
}

#[tokio::test]
async fn state_transitions_stay_within_the_pause_resume_consume_whitelist() {
    let store = MemoryAutomationSignalStore::new();
    let paused = pause(&store).await;
    // Resumed → Paused（再次暂停）允许。
    let first = store
        .commit_signal(
            fact(
                AutomationSignalState::Resumed,
                "signal-2",
                2,
                4,
                5,
                Some(APPROVAL),
            ),
            Some(&paused),
        )
        .await
        .unwrap();
    let resumed = first.fact.signal_digest;
    let second_pause = store
        .commit_signal(
            fact(AutomationSignalState::Paused, "signal-3", 3, 4, 5, None),
            Some(&resumed),
        )
        .await
        .unwrap();
    let paused_again = second_pause.fact.signal_digest;
    // 同状态的新事实（新 signal_id）不允许：新暂停与恢复之间必须有真实转移。
    assert_eq!(
        store
            .commit_signal(
                fact(AutomationSignalState::Paused, "signal-4", 4, 4, 5, None),
                Some(&paused_again)
            )
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_transition_refused")
    );
    // 终态 Consumed 之后不能回到任何非终态。
    let resumed_again = store
        .commit_signal(
            fact(
                AutomationSignalState::Resumed,
                "signal-4",
                4,
                4,
                5,
                Some(FOREIGN_APPROVAL),
            ),
            Some(&paused_again),
        )
        .await
        .unwrap();
    let consumed = store
        .commit_signal(
            fact(
                AutomationSignalState::Consumed,
                "signal-5",
                5,
                4,
                5,
                Some(FOREIGN_APPROVAL),
            ),
            Some(&resumed_again.fact.signal_digest),
        )
        .await
        .unwrap();
    assert!(consumed.fact.consumed);
    assert_eq!(
        store
            .commit_signal(
                fact(AutomationSignalState::Paused, "signal-6", 6, 4, 5, None),
                Some(&consumed.fact.signal_digest)
            )
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_transition_refused")
    );
    assert_eq!(store.fetch_signals("execution-1").await.unwrap().len(), 5);
}

#[tokio::test]
async fn identity_drift_inside_one_execution_is_refused() {
    let store = MemoryAutomationSignalStore::new();
    let paused = pause(&store).await;
    // owner/definition/action/path 在同一个 execution 内漂移各自拒绝。
    let cases: [(&str, Box<dyn Fn(&mut AutomationSignalFact)>); 4] = [
        (
            "automation_signal_owner_drift",
            Box::new(|value: &mut AutomationSignalFact| value.owner_id = "owner-2".to_owned()),
        ),
        (
            "automation_signal_definition_digest_drift",
            Box::new(|value: &mut AutomationSignalFact| {
                value.definition_digest =
                    "sha256:7777777777777777777777777777777777777777777777777777777777777777"
                        .to_owned()
            }),
        ),
        (
            "automation_signal_action_digest_drift",
            Box::new(|value: &mut AutomationSignalFact| {
                value.action_digest = OTHER_ACTION.to_owned()
            }),
        ),
        (
            "automation_signal_path_scope_drift",
            Box::new(|value: &mut AutomationSignalFact| {
                value.path_scope_digest =
                    "sha256:8888888888888888888888888888888888888888888888888888888888888888"
                        .to_owned()
            }),
        ),
    ];
    for (code, mutate) in cases {
        let mut drifted = fact(
            AutomationSignalState::Resumed,
            "signal-2",
            2,
            4,
            5,
            Some(APPROVAL),
        );
        mutate(&mut drifted);
        drifted.recompute_digest();
        assert_eq!(
            store
                .commit_signal(drifted, Some(&paused))
                .await
                .unwrap_err()
                .to_string(),
            conflict(code)
        );
    }
    // checkpoint 序号回退拒绝。
    assert_eq!(
        store
            .commit_signal(
                fact(
                    AutomationSignalState::Resumed,
                    "signal-2",
                    2,
                    4,
                    4,
                    Some(APPROVAL)
                ),
                Some(&paused)
            )
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_checkpoint_sequence_regression")
    );
    assert_eq!(store.fetch_signals("execution-1").await.unwrap().len(), 1);
}

#[tokio::test]
async fn authority_epoch_can_only_advance_on_a_restart_resume() {
    let store = MemoryAutomationSignalStore::new();
    let paused = pause(&store).await;
    // epoch 回退拒绝：即使状态转移本身合法（Paused → Resumed）也不接受旧 epoch。
    assert_eq!(
        store
            .commit_signal(
                fact(
                    AutomationSignalState::Resumed,
                    "signal-stale",
                    2,
                    3,
                    5,
                    Some(APPROVAL)
                ),
                Some(&paused)
            )
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_authority_epoch_stale")
    );
    // Resumed 事实可以前进 epoch（重启恢复路径），并成为新的权威副本。
    let resumed = store
        .commit_signal(
            fact(
                AutomationSignalState::Resumed,
                "signal-2",
                2,
                5,
                5,
                Some(APPROVAL),
            ),
            Some(&paused),
        )
        .await
        .unwrap();
    assert_eq!(resumed.fact.authority_epoch, 5);
    // 非 Resumed 事实不允许继续加码 epoch：再次暂停只能沿用当前 epoch。
    assert_eq!(
        store
            .commit_signal(
                fact(AutomationSignalState::Paused, "signal-3", 3, 6, 5, None),
                Some(&resumed.fact.signal_digest)
            )
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_authority_epoch_ahead")
    );
    store
        .commit_signal(
            fact(AutomationSignalState::Paused, "signal-3", 3, 5, 5, None),
            Some(&resumed.fact.signal_digest),
        )
        .await
        .unwrap();
    assert_eq!(store.fetch_signals("execution-1").await.unwrap().len(), 3);
}

#[tokio::test]
async fn an_approval_digest_supports_exactly_one_resume_and_one_consume_reference() {
    let store = MemoryAutomationSignalStore::new();
    let paused = pause(&store).await;
    let resumed = store
        .commit_signal(
            fact(
                AutomationSignalState::Resumed,
                "signal-2",
                2,
                4,
                5,
                Some(APPROVAL),
            ),
            Some(&paused),
        )
        .await
        .unwrap();
    assert!(store.approval_consumed(APPROVAL).await);
    // 再次暂停后，同一条已消费审批不能支撑第二次恢复。
    let second_pause = store
        .commit_signal(
            fact(AutomationSignalState::Paused, "signal-3", 3, 4, 5, None),
            Some(&resumed.fact.signal_digest),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .commit_signal(
                fact(
                    AutomationSignalState::Resumed,
                    "signal-4",
                    4,
                    4,
                    5,
                    Some(APPROVAL)
                ),
                Some(&second_pause.fact.signal_digest)
            )
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_approval_consumed")
    );
    // 新审批可以支撑新的恢复。
    let fresh = store
        .commit_signal(
            fact(
                AutomationSignalState::Resumed,
                "signal-4",
                4,
                4,
                5,
                Some(FOREIGN_APPROVAL),
            ),
            Some(&second_pause.fact.signal_digest),
        )
        .await
        .unwrap();
    assert_eq!(fresh.fact.state, AutomationSignalState::Resumed);
    // Consumed 事实必须引用自己 Resumed 前驱的审批摘要；别的摘要拒绝。
    assert_eq!(
        store
            .commit_signal(
                fact(
                    AutomationSignalState::Consumed,
                    "signal-5",
                    5,
                    4,
                    5,
                    Some(APPROVAL)
                ),
                Some(&fresh.fact.signal_digest)
            )
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_approval_digest_drift")
    );
    // 引用同一摘要的终态消费允许且只发生一次。
    let consumed = store
        .commit_signal(
            fact(
                AutomationSignalState::Consumed,
                "signal-5",
                5,
                4,
                5,
                Some(FOREIGN_APPROVAL),
            ),
            Some(&fresh.fact.signal_digest),
        )
        .await
        .unwrap();
    assert!(consumed.fact.consumed);
    // 终态之后的任何新事实都拒绝。
    assert_eq!(
        store
            .commit_signal(
                fact(AutomationSignalState::Paused, "signal-6", 6, 4, 5, None),
                Some(&consumed.fact.signal_digest)
            )
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_transition_refused")
    );
    assert_eq!(store.fetch_signals("execution-1").await.unwrap().len(), 5);
}

#[tokio::test]
async fn fetch_is_a_pure_read_and_missing_executions_return_an_empty_history() {
    let store = MemoryAutomationSignalStore::new();
    assert!(store
        .fetch_signals("execution-missing")
        .await
        .unwrap()
        .is_empty());
    let paused = pause(&store).await;
    // 纯读取：多次 fetch 不改变历史与审批状态。
    for _ in 0..3 {
        let history = store.fetch_signals("execution-1").await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].signal_digest, paused);
    }
    assert!(!store.approval_consumed(APPROVAL).await);
}

//! AUT-18 先拒绝优先夹具：Core signal 协调器从权威 store 历史重建账本，只在 owner/definition/
//! action/path/epoch/checkpoint/审批全部匹配时提交 pause/resume/consume，且永不派发。
//!
//! 夹具驱动真实的 `MemoryAutomationSignalStore` 与一个只会计数的 signal store 包装器；
//! `commits` 计数器代表“真正写入的 signal 事实数量”。本文件不连接 EventLog、Broker、模型、
//! 时钟或网络。

use async_trait::async_trait;
use kiana_core::{apply_automation_signal, AutomationSignalOutcome};
use kiana_domain::{
    AutomationApproverRole, AutomationCheckpoint, AutomationSignalApproval, AutomationSignalCommit,
    AutomationSignalFact, AutomationSignalKind, AutomationSignalRequest, AutomationSignalState,
};
use kiana_eventlog::MemoryAutomationSignalStore;
use kiana_ports::{AutomationSignalStore, PortError};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const DEFINITION: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const ACTION: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const PATH: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const OTHER_DEFINITION: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";
const OTHER_ACTION: &str =
    "sha256:5555555555555555555555555555555555555555555555555555555555555555";
const OTHER_PATH: &str = "sha256:6666666666666666666666666666666666666666666666666666666666666666";

/// 计数包装器：记录 fetch/commit 次数，并可让 commit 失败以验证“不确定提交绝不成功”。
#[derive(Default)]
struct SpySignalStore {
    inner: MemoryAutomationSignalStore,
    fetches: AtomicUsize,
    commits: AtomicUsize,
    fail_commits: AtomicBool,
}

impl SpySignalStore {
    fn new() -> Self {
        Self::default()
    }

    fn commits(&self) -> usize {
        self.commits.load(Ordering::SeqCst)
    }

    fn fetches(&self) -> usize {
        self.fetches.load(Ordering::SeqCst)
    }

    async fn history(&self, execution_id: &str) -> Vec<AutomationSignalFact> {
        self.inner
            .fetch_signals(execution_id)
            .await
            .expect("memory history must be readable")
    }
}

#[async_trait]
impl AutomationSignalStore for SpySignalStore {
    async fn fetch_signals(
        &self,
        execution_id: &str,
    ) -> Result<Vec<AutomationSignalFact>, PortError> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        self.inner.fetch_signals(execution_id).await
    }

    async fn commit_signal(
        &self,
        fact: AutomationSignalFact,
        expected_digest: Option<&str>,
    ) -> Result<AutomationSignalCommit, PortError> {
        self.commits.fetch_add(1, Ordering::SeqCst);
        if self.fail_commits.load(Ordering::SeqCst) {
            return Err(PortError::Unavailable(
                "automation_signal_store_unavailable".to_owned(),
            ));
        }
        self.inner.commit_signal(fact, expected_digest).await
    }
}

/// 篡改历史的 store：返回不可重放的事实，验证协调器不会把它折叠成成功。
struct TamperedHistoryStore {
    history: Vec<AutomationSignalFact>,
}

#[async_trait]
impl AutomationSignalStore for TamperedHistoryStore {
    async fn fetch_signals(
        &self,
        _execution_id: &str,
    ) -> Result<Vec<AutomationSignalFact>, PortError> {
        Ok(self.history.clone())
    }
}

fn conflict(reason: &str) -> String {
    PortError::Conflict(reason.to_owned()).to_string()
}

fn failed(reason: &str) -> String {
    PortError::Failed(reason.to_owned()).to_string()
}

/// 构造一个默认合法（字段可被测试覆盖）的请求。
fn request(
    kind: AutomationSignalKind,
    signal_id: &str,
    sequence: u64,
    epoch: u64,
    checkpoint_sequence: u64,
    approval_digest: Option<&str>,
) -> AutomationSignalRequest {
    AutomationSignalRequest::new(
        "execution-1",
        signal_id,
        sequence,
        kind,
        "owner-1",
        DEFINITION,
        ACTION,
        PATH,
        epoch,
        checkpoint_sequence,
        approval_digest.map(str::to_owned),
        None,
        "operator-signal",
    )
    .expect("request fixture must be self-consistent")
}

fn pause_request() -> AutomationSignalRequest {
    request(AutomationSignalKind::Pause, "signal-1", 1, 4, 5, None)
}

/// 构造一个默认合法（字段可被测试覆盖）的 checkpoint。
fn checkpoint(epoch: u64, sequence: u64) -> AutomationCheckpoint {
    AutomationCheckpoint::new(
        "execution-1",
        "owner-1",
        DEFINITION,
        ACTION,
        PATH,
        epoch,
        sequence,
        7,
        11,
    )
    .expect("checkpoint fixture must be self-consistent")
}

/// 构造一条绑定 pause signal 的恢复审批；身份字段显式传入以便构造“自洽但漂移”的请求。
#[allow(clippy::too_many_arguments)]
fn approval_for(
    approval_id: &str,
    pause_signal_id: &str,
    epoch: u64,
    owner: &str,
    definition: &str,
    action: &str,
    path: &str,
) -> AutomationSignalApproval {
    AutomationSignalApproval::new(
        approval_id,
        "execution-1",
        owner,
        "operator-1",
        AutomationApproverRole::Operator,
        AutomationSignalKind::Resume,
        definition,
        action,
        path,
        pause_signal_id,
        epoch,
        1,
        10,
        100,
    )
    .expect("approval fixture must be self-consistent")
}

fn approval(pause_signal_id: &str, epoch: u64) -> AutomationSignalApproval {
    approval_for(
        "approval-1",
        pause_signal_id,
        epoch,
        "owner-1",
        DEFINITION,
        ACTION,
        PATH,
    )
}

/// 构造一条绑定审批的恢复请求；身份字段显式传入以便构造“自洽但漂移”的请求。
#[allow(clippy::too_many_arguments)]
fn resume_for(
    sequence: u64,
    epoch: u64,
    approval: &AutomationSignalApproval,
    owner: &str,
    definition: &str,
    action: &str,
    path: &str,
) -> AutomationSignalRequest {
    AutomationSignalRequest::new(
        "execution-1",
        format!("signal-{sequence}"),
        sequence,
        AutomationSignalKind::Resume,
        owner,
        definition,
        action,
        path,
        epoch,
        // 恢复请求固定绑定 checkpoint 序号 5（与 pause 时的 checkpoint 身份一致）。
        5,
        Some(approval.approval_digest.clone()),
        None,
        "operator-resume",
    )
    .expect("request fixture must be self-consistent")
}

fn resume_request(
    sequence: u64,
    epoch: u64,
    approval: &AutomationSignalApproval,
) -> AutomationSignalRequest {
    resume_for(
        sequence, epoch, approval, "owner-1", DEFINITION, ACTION, PATH,
    )
}

/// 提交一次 pause，返回已提交事实。
async fn commit_pause(store: &SpySignalStore) -> AutomationSignalFact {
    let outcome =
        apply_automation_signal(store, &pause_request(), None, Some(&checkpoint(4, 5)), 20)
            .await
            .expect("pause must commit");
    match outcome {
        AutomationSignalOutcome::Paused(fact) => fact,
        other => panic!("unexpected pause outcome: {other:?}"),
    }
}

#[tokio::test]
async fn shape_failures_never_touch_the_store() {
    let store = SpySignalStore::new();
    // UI 自带 context：请求形状错误，必须在任何端口调用之前拒绝。
    let mut carried = pause_request();
    carried.ui_context_digest = Some(ACTION.to_owned());
    carried.recompute_digest();
    assert_eq!(
        apply_automation_signal(&store, &carried, None, Some(&checkpoint(4, 5)), 20)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_ui_context_forbidden")
    );
    // 自批与越权审批：审批 DTO 校验先行，store 同样不被触碰。
    let mut self_approval = approval("signal-1", 4);
    self_approval.approver_id = "owner-1".to_owned();
    self_approval.recompute_digest();
    let resume = request(
        AutomationSignalKind::Resume,
        "signal-2",
        2,
        4,
        5,
        Some(self_approval.approval_digest.as_str()),
    );
    assert_eq!(
        apply_automation_signal(
            &store,
            &resume,
            Some(&self_approval),
            Some(&checkpoint(4, 5)),
            20
        )
        .await
        .unwrap_err()
        .to_string(),
        conflict("automation_signal_approval_self_approval")
    );
    let mut unauthorized = approval("signal-1", 4);
    unauthorized.approver_role = AutomationApproverRole::Auditor;
    unauthorized.recompute_digest();
    assert_eq!(
        apply_automation_signal(
            &store,
            &resume,
            Some(&unauthorized),
            Some(&checkpoint(4, 5)),
            20
        )
        .await
        .unwrap_err()
        .to_string(),
        conflict("automation_signal_approver_unauthorized")
    );
    assert_eq!(store.fetches(), 0, "形状拒绝路径不得查询历史");
    assert_eq!(store.commits(), 0, "形状拒绝路径不得写入事实");
    // pause 自带审批：形状合法但计划函数拒绝；不写事实。
    let paused_with_approval = approval("signal-1", 4);
    assert_eq!(
        apply_automation_signal(
            &store,
            &pause_request(),
            Some(&paused_with_approval),
            Some(&checkpoint(4, 5)),
            20
        )
        .await
        .unwrap_err()
        .to_string(),
        conflict("automation_signal_approval_unexpected")
    );
    assert_eq!(store.commits(), 0);
}

#[tokio::test]
async fn pause_is_checkpoint_bound_and_requires_no_approval() {
    let store = SpySignalStore::new();
    // 缺少 checkpoint：拒绝且不写事实。
    assert_eq!(
        apply_automation_signal(&store, &pause_request(), None, None, 20)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_checkpoint_required")
    );
    assert_eq!(store.commits(), 0);
    // 正确绑定后提交，返回 Paused 且提交数恰好 1。
    let fact = commit_pause(&store).await;
    assert_eq!(fact.state, AutomationSignalState::Paused);
    assert_eq!(fact.checkpoint_sequence, 5);
    assert_eq!(fact.approval_digest, None);
    assert_eq!(store.commits(), 1);
    assert!(store.fetches() >= 1);
}

#[tokio::test]
async fn caller_snapshots_cannot_resume_after_identity_drift() {
    // 每一条漂移都是“请求与审批自洽、但与已提交 pause 不一致”，证明拒绝来自权威历史而不是调用方自述。
    let identities = [
        (
            "owner-2",
            DEFINITION,
            ACTION,
            PATH,
            "automation_signal_owner_drift",
        ),
        (
            "owner-1",
            OTHER_DEFINITION,
            ACTION,
            PATH,
            "automation_signal_definition_digest_drift",
        ),
        (
            "owner-1",
            DEFINITION,
            OTHER_ACTION,
            PATH,
            "automation_signal_action_digest_drift",
        ),
        (
            "owner-1",
            DEFINITION,
            ACTION,
            OTHER_PATH,
            "automation_signal_path_scope_drift",
        ),
    ];
    for (index, (owner, definition, action, path, code)) in identities.into_iter().enumerate() {
        let store = SpySignalStore::new();
        commit_pause(&store).await;
        let bound = approval_for(
            &format!("approval-drift-{index}"),
            "signal-1",
            4,
            owner,
            definition,
            action,
            path,
        );
        let drifted = resume_for(2, 4, &bound, owner, definition, action, path);
        let commits = store.commits();
        assert_eq!(
            apply_automation_signal(&store, &drifted, Some(&bound), Some(&checkpoint(4, 5)), 20)
                .await
                .unwrap_err()
                .to_string(),
            conflict(code)
        );
        assert_eq!(store.commits(), commits, "漂移拒绝不得写入新事实");
        let history = store.history("execution-1").await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].state, AutomationSignalState::Paused);
    }
}

#[tokio::test]
async fn duplicate_signal_replays_without_a_second_commit() {
    let store = SpySignalStore::new();
    commit_pause(&store).await;
    // 同一请求（同 signal_id 同内容）第二次提交：幂等重放，不写第二条事实、不二次消费。
    let outcome =
        apply_automation_signal(&store, &pause_request(), None, Some(&checkpoint(4, 5)), 20)
            .await
            .unwrap();
    assert!(matches!(outcome, AutomationSignalOutcome::Replayed(_)));
    assert!(!outcome.committed());
    assert_eq!(outcome.fact().state, AutomationSignalState::Paused);
    assert_eq!(store.commits(), 1, "重放不得触发第二次 commit");
    // 同一 signal_id 不同决策内容（checkpoint 序号不同）：冲突，plan 阶段就拒绝。
    let mut conflicting = pause_request();
    conflicting.checkpoint_sequence = 6;
    conflicting.recompute_digest();
    assert_eq!(
        apply_automation_signal(&store, &conflicting, None, Some(&checkpoint(4, 6)), 20)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_signal_conflict")
    );
    assert_eq!(store.commits(), 1, "冲突在计划阶段被拒，不写事实");
    assert_eq!(store.history("execution-1").await.len(), 1);
}

#[tokio::test]
async fn restart_resume_is_rebuilt_from_committed_history() {
    let store = SpySignalStore::new();
    commit_pause(&store).await;
    // “重启”：进程内账本丢失，只有已提交历史仍在 store；epoch 前推且身份/checkpoint 匹配才恢复。
    let stale_approval = approval("signal-1", 4);
    let mut old_epoch = resume_request(2, 4, &stale_approval);
    old_epoch.authority_epoch = 5;
    old_epoch.recompute_digest();
    assert_eq!(
        apply_automation_signal(
            &store,
            &old_epoch,
            Some(&stale_approval),
            Some(&checkpoint(5, 5)),
            20
        )
        .await
        .unwrap_err()
        .to_string(),
        conflict("automation_signal_approval_authority_epoch_stale")
    );
    // checkpoint 身份漂移：拒绝。
    let fresh_approval = approval("signal-1", 5);
    let fresh_resume = resume_request(2, 5, &fresh_approval);
    let mut drifted = checkpoint(5, 5);
    drifted.action_digest = OTHER_ACTION.to_owned();
    drifted.recompute_digest();
    assert_eq!(
        apply_automation_signal(
            &store,
            &fresh_resume,
            Some(&fresh_approval),
            Some(&drifted),
            20
        )
        .await
        .unwrap_err()
        .to_string(),
        conflict("automation_signal_checkpoint_action_digest_drift")
    );
    // 身份与 checkpoint 匹配：恢复成功，事实 epoch 前进并绑定审批摘要。
    let outcome = apply_automation_signal(
        &store,
        &fresh_resume,
        Some(&fresh_approval),
        Some(&checkpoint(5, 5)),
        20,
    )
    .await
    .unwrap();
    match &outcome {
        AutomationSignalOutcome::Resumed(fact) => {
            assert_eq!(fact.authority_epoch, 5);
            assert_eq!(
                fact.approval_digest.as_deref(),
                Some(fresh_approval.approval_digest.as_str())
            );
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
    assert!(outcome.committed());
    // 已在恢复态：第二次恢复拒绝（不是 Paused）。
    assert_eq!(
        apply_automation_signal(
            &store,
            &resume_request(3, 5, &fresh_approval),
            Some(&fresh_approval),
            Some(&checkpoint(5, 5)),
            20
        )
        .await
        .unwrap_err()
        .to_string(),
        conflict("automation_signal_not_paused")
    );
    assert_eq!(store.history("execution-1").await.len(), 2);
}

#[tokio::test]
async fn approval_expiry_and_single_use_are_enforced() {
    let store = SpySignalStore::new();
    commit_pause(&store).await;
    let commits = store.commits();
    // 审批窗口边界：now_tick 到达 expires_at_tick 即过期。
    let approval = approval("signal-1", 4);
    assert_eq!(
        apply_automation_signal(
            &store,
            &resume_request(2, 4, &approval),
            Some(&approval),
            Some(&checkpoint(4, 5)),
            100
        )
        .await
        .unwrap_err()
        .to_string(),
        conflict("automation_signal_approval_expired")
    );
    assert_eq!(store.commits(), commits);
    // 窗口内恢复成功，审批被消费。
    apply_automation_signal(
        &store,
        &resume_request(2, 4, &approval),
        Some(&approval),
        Some(&checkpoint(4, 5)),
        20,
    )
    .await
    .unwrap();
    // 再次暂停后复用同一审批：拒绝，且不追加事实。
    let mut second_pause = pause_request();
    second_pause.signal_id = "signal-3".to_owned();
    second_pause.signal_sequence = 3;
    second_pause.recompute_digest();
    apply_automation_signal(&store, &second_pause, None, Some(&checkpoint(4, 5)), 20)
        .await
        .unwrap();
    let commits = store.commits();
    assert_eq!(
        apply_automation_signal(
            &store,
            &resume_request(4, 4, &approval),
            Some(&approval),
            Some(&checkpoint(4, 5)),
            20
        )
        .await
        .unwrap_err()
        .to_string(),
        conflict("automation_signal_approval_pause_drift")
    );
    assert_eq!(store.commits(), commits);
}

#[tokio::test]
async fn consume_is_terminal_and_cannot_be_resurrected() {
    let store = SpySignalStore::new();
    commit_pause(&store).await;
    let approval = approval("signal-1", 4);
    apply_automation_signal(
        &store,
        &resume_request(2, 4, &approval),
        Some(&approval),
        Some(&checkpoint(4, 5)),
        20,
    )
    .await
    .unwrap();
    let consume = request(
        AutomationSignalKind::Consume,
        "signal-3",
        3,
        4,
        5,
        Some(approval.approval_digest.as_str()),
    );
    let outcome = apply_automation_signal(&store, &consume, None, None, 20)
        .await
        .unwrap();
    match &outcome {
        AutomationSignalOutcome::Consumed(fact) => assert!(fact.consumed),
        other => panic!("unexpected outcome: {other:?}"),
    }
    let commits = store.commits();
    // 第二次消费与再次暂停都必须拒绝，终态不可复活。
    let second_consume = request(
        AutomationSignalKind::Consume,
        "signal-4",
        4,
        4,
        5,
        Some(approval.approval_digest.as_str()),
    );
    assert_eq!(
        apply_automation_signal(&store, &second_consume, None, None, 20)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_already_consumed")
    );
    let mut third_pause = pause_request();
    third_pause.signal_id = "signal-5".to_owned();
    // 下一次合法序号仍是 4（第二次消费被拒，未占用序号）。
    third_pause.signal_sequence = 4;
    third_pause.recompute_digest();
    assert_eq!(
        apply_automation_signal(&store, &third_pause, None, Some(&checkpoint(4, 5)), 20)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_already_consumed")
    );
    assert_eq!(store.commits(), commits);
    assert_eq!(store.history("execution-1").await.len(), 3);
}

#[tokio::test]
async fn store_history_that_cannot_be_replayed_fails_closed() {
    // 历史以“序号 1 的 Resumed 事实”开始：无法重放成合法序列，协调器必须拒绝而不是当空历史。
    let tampered = AutomationSignalFact::new(
        "execution-1",
        "signal-1",
        1,
        "owner-1",
        DEFINITION,
        ACTION,
        PATH,
        4,
        5,
        AutomationSignalState::Resumed,
        Some(ACTION.to_owned()),
    )
    .unwrap();
    let store = TamperedHistoryStore {
        history: vec![tampered],
    };
    assert_eq!(
        apply_automation_signal(&store, &pause_request(), None, Some(&checkpoint(4, 5)), 20)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_signal_transition_refused")
    );
}

#[tokio::test]
async fn uncertain_commits_are_never_reported_as_success() {
    let store = SpySignalStore::new();
    store.fail_commits.store(true, Ordering::SeqCst);
    assert_eq!(
        apply_automation_signal(&store, &pause_request(), None, Some(&checkpoint(4, 5)), 20)
            .await
            .unwrap_err()
            .to_string(),
        PortError::Unavailable("automation_signal_store_unavailable".to_owned()).to_string()
    );
    assert_eq!(store.commits(), 1, "确实尝试了提交");
    assert!(store.history("execution-1").await.is_empty());
    // 形状问题映射为 `Failed(*_invalid)`，不冒充状态冲突。
    let mut invalid = pause_request();
    invalid.signal_sequence = 0;
    invalid.recompute_digest();
    assert_eq!(
        apply_automation_signal(&store, &invalid, None, Some(&checkpoint(4, 5)), 20)
            .await
            .unwrap_err()
            .to_string(),
        failed("automation_signal_request_invalid")
    );
}

//! H23 行为夹具：压缩结果提交、来源和失效传播。
//!
//! 夹具只驱动确定性 domain 合同（`CompactionCommitJournal` / `NativeCompactionBinding`）：不读写
//! 文件系统、不起进程、不调用模型或网络。真实字节持久化仍由 Core 的 content-addressed artifact
//! store port 负责，这里固定的是「先保存并校验 → 再 CAS 提交 → 确认后才切换视图」的顺序。
//!
//! 卡片命名验收，拒绝路径在前：
//! - `crash_before_compaction_commit_keeps_old_context`（先拒绝）
//! - `stale_summary_cannot_overwrite_new_steering`（先拒绝）
//! - `revoked_source_invalidates_summary`（先拒绝）
//! - `compacted_context_rebuilds_to_same_view_after_restart`（再成功）

use kiana_domain::{
    journal_sha256, json_digest, CapabilitySupport, CompactionArtifact, CompactionCommit,
    CompactionCommitJournal, ContextCacheBinding, ContextCheckpointState, ContextInvalidationPlan,
    DependencyEdge, DependencyNode, DependencyNodeKind, EventId, ModelProtocol, ModelRoute,
    NativeCompactionBinding, RunId, SourceDependencyGraph,
};

fn digest(value: &str) -> String {
    json_digest(&serde_json::json!({"value": value}))
}

fn artifact() -> CompactionArtifact {
    CompactionArtifact::new(
        RunId::new(),
        digest("summary"),
        digest("artifact"),
        10,
        12,
        256,
    )
    .unwrap()
}

fn commit() -> CompactionCommit {
    let artifact = artifact();
    CompactionCommit::new(
        &artifact,
        4,
        5,
        vec![EventId::new()],
        digest("prompt"),
        "builder-default",
        digest("budget"),
        8,
        3,
    )
    .unwrap()
}

#[test]
fn artifact_is_validated_before_compaction_view_commit() {
    let commit = commit();
    commit.validate().unwrap();
    commit.validate_before_commit(4, 12, 8, 3).unwrap();
    assert_eq!(
        commit.validate_before_commit(4, 13, 8, 3).unwrap_err(),
        "compaction_source_changed"
    );
}

#[test]
fn stale_steering_workspace_or_revoked_data_cannot_overwrite_summary() {
    let commit = commit();
    assert_eq!(
        commit.validate_before_commit(5, 12, 8, 3).unwrap_err(),
        "compaction_source_changed"
    );
    assert_eq!(
        commit.validate_before_commit(4, 12, 9, 3).unwrap_err(),
        "compaction_workspace_changed"
    );
    assert_eq!(
        commit.validate_before_commit(4, 12, 8, 4).unwrap_err(),
        "compaction_data_epoch_changed"
    );
}

#[test]
fn forged_artifact_or_duplicate_source_event_is_rejected() {
    let mut forged = artifact();
    forged.artifact_digest = digest("forged");
    assert_eq!(
        forged.validate().unwrap_err(),
        "compaction_artifact_digest_mismatch"
    );
    let artifact = artifact();
    let event_id = EventId::new();
    assert_eq!(
        CompactionCommit::new(
            &artifact,
            4,
            5,
            vec![event_id, event_id],
            digest("prompt"),
            "builder-default",
            digest("budget"),
            8,
            3,
        )
        .unwrap_err(),
        "compaction_commit_source_duplicate"
    );
}

/// 夹具摘要载荷：逐字节内容决定 digest，空白或重排都会改变 hash。
fn summary_bytes(goal: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema": "kiana.compact-summary.v1",
        "goal": goal,
        "next_action": "continue the task",
    }))
    .expect("fixture summary serializes")
}

fn declared_digest(bytes: &[u8]) -> String {
    format!("sha256:{}", journal_sha256(bytes))
}

fn journal() -> CompactionCommitJournal {
    CompactionCommitJournal::new(RunId::new(), 4, 10, digest("inbox"), 1).unwrap()
}

fn stage(
    journal: &mut CompactionCommitJournal,
    goal: &str,
    start: u64,
    end: u64,
) -> (usize, Vec<u8>) {
    let payload = summary_bytes(goal);
    let index = journal
        .stage(&payload, &declared_digest(&payload), start, end)
        .unwrap();
    (index, payload)
}

/// 按当前 checkpoint 的 source revision 构造一次提交引用；提交是否允许仍由 CAS 判定。
fn commit_for(journal: &CompactionCommitJournal, index: usize) -> CompactionCommit {
    let staged = &journal.staged_artifacts()[index];
    let source_revision = journal.active_checkpoint().source_context_revision;
    CompactionCommit::new(
        &staged.artifact,
        source_revision,
        source_revision + 1,
        vec![EventId::new()],
        digest("prompt"),
        "builder-default",
        digest("budget"),
        8,
        3,
    )
    .unwrap()
}

fn invalidation_plan() -> ContextInvalidationPlan {
    let mut graph = SourceDependencyGraph::new(
        1,
        vec![
            DependencyNode {
                id: "source:memory".to_owned(),
                kind: DependencyNodeKind::Memory,
                source_id: Some("source:memory".to_owned()),
            },
            DependencyNode {
                id: "summary:1".to_owned(),
                kind: DependencyNodeKind::Summary,
                source_id: None,
            },
        ],
        vec![DependencyEdge {
            from: "summary:1".to_owned(),
            to: "source:memory".to_owned(),
            relation: "summarizes".to_owned(),
        }],
    )
    .unwrap();
    let invalidation = graph.revoke_source("source:memory", 2).unwrap();
    ContextInvalidationPlan::from_source_invalidation(&graph, &invalidation).unwrap()
}

fn fixture_route() -> ModelRoute {
    ModelRoute {
        provider_id: "fixture-provider".to_owned(),
        protocol: ModelProtocol::AnthropicMessages,
        connection_id: "fixture-connection".to_owned(),
        model_id: "builder-current".to_owned(),
        profile: "builder-default".to_owned(),
        configuration_revision: digest("config"),
        streaming: false,
    }
}

/// 先拒绝：artifact 已完整保存并校验，但提交前崩溃时不得切换视图；未引用 artifact 留给 GC，
/// 旧事实仍可查询，重启读回同一 journal 也看不到新视图。
#[test]
fn crash_before_compaction_commit_keeps_old_context() {
    let mut journal = journal();
    let (first_index, _first_payload) = stage(&mut journal, "first goal", 10, 12);
    let first_commit = commit_for(&journal, first_index);
    journal
        .commit(first_index, &first_commit, 4, 12, 8, 3, digest("inbox"), 1)
        .unwrap();
    assert_eq!(journal.applied_facts().len(), 1);

    // 已提交视图是 Committed；未开始下一次压缩前不能保存新 artifact。
    let pending_payload = summary_bytes("second goal");
    assert_eq!(
        journal
            .stage(&pending_payload, &declared_digest(&pending_payload), 12, 14)
            .unwrap_err(),
        "compaction_journal_not_active"
    );

    journal.begin_next_compaction().unwrap();
    let (pending_index, _pending_payload) = stage(&mut journal, "second goal", 12, 14);
    // 保存成功但提交前崩溃：有效视图仍是第一次提交后的 revision 5。
    assert_eq!(
        journal.active_checkpoint().state,
        ContextCheckpointState::Active
    );
    assert_eq!(journal.active_checkpoint().source_context_revision, 5);
    assert_eq!(journal.gc_candidates(), vec![pending_index]);
    assert_eq!(journal.applied_facts().len(), 1);

    // 重启：从序列化 journal 读回并重新校验保存字节；旧事实、旧视图仍可查询。
    let restarted: CompactionCommitJournal =
        serde_json::from_str(&serde_json::to_string(&journal).unwrap()).unwrap();
    restarted.validate().unwrap();
    assert_eq!(restarted.applied_facts().len(), 1);
    assert_eq!(restarted.gc_candidates(), vec![pending_index]);
    assert_eq!(restarted.history().len(), 1);
    let view = restarted.rebuild_view(digest("prepared"), 3).unwrap();
    assert_eq!(view.context_revision, 5);
    assert_eq!(view.source_cursor, 12);
    assert!(!restarted.applied_facts()[0].is_invalidated());
}

/// 先拒绝：压缩期间到来的新输入与晚到源事件不能被旧摘要覆盖。
#[test]
fn stale_summary_cannot_overwrite_new_steering() {
    let mut journal = journal();
    let (index, _payload) = stage(&mut journal, "stale goal", 10, 12);
    let commit = commit_for(&journal, index);

    // 晚到源事件把当前 cursor 推到 13：按 [10,12] 计划的摘要不能提交。
    assert_eq!(
        journal
            .commit(
                index,
                &commit,
                4,
                13,
                8,
                3,
                digest("inbox-after-steering"),
                2
            )
            .unwrap_err(),
        "compaction_source_changed"
    );
    assert_eq!(
        journal.active_checkpoint().state,
        ContextCheckpointState::Active
    );
    assert_eq!(journal.active_checkpoint().active_context_revision, 4);
    assert!(journal.applied_facts().is_empty());
    assert_eq!(journal.gc_candidates(), vec![index]);

    // 旧 Inbox 快照（同序号、不同 digest）不能被当成当前状态提交。
    assert_eq!(
        journal
            .commit(
                index,
                &commit,
                4,
                12,
                8,
                3,
                digest("stale-inbox-snapshot"),
                1
            )
            .unwrap_err(),
        "context_checkpoint_inbox_changed"
    );
    assert!(journal.applied_facts().is_empty());

    // 保留新输入：提交时携带压缩期间到达的 steering，视图前进但不覆盖新输入。
    let fact_index = journal
        .commit(
            index,
            &commit,
            4,
            12,
            8,
            3,
            digest("inbox-after-steering"),
            2,
        )
        .unwrap();
    let fact = &journal.applied_facts()[fact_index];
    assert_eq!(fact.active_context_revision, 5);
    assert_eq!(fact.inbox_sequence, 2);
    assert_eq!(fact.inbox_digest, digest("inbox-after-steering"));
    assert_eq!(journal.active_checkpoint().active_context_revision, 5);
    assert_eq!(journal.active_checkpoint().inbox_sequence, 2);
    assert!(journal.gc_candidates().is_empty());
}

/// 先拒绝：源撤销/删除使已提交摘要失效，并让按旧 data epoch 计划的提交 fail closed。
#[test]
fn revoked_source_invalidates_summary() {
    // data epoch 3 → 4 之后，已按 epoch 3 计划的摘要不能再提交。
    let mut pending = journal();
    let (pending_index, _pending_payload) = stage(&mut pending, "revoked goal", 10, 12);
    let pending_commit = commit_for(&pending, pending_index);
    assert_eq!(
        pending
            .commit(
                pending_index,
                &pending_commit,
                4,
                12,
                8,
                4,
                digest("inbox"),
                1
            )
            .unwrap_err(),
        "compaction_data_epoch_changed"
    );
    assert_eq!(
        pending.active_checkpoint().state,
        ContextCheckpointState::Active
    );
    assert_eq!(pending.gc_candidates(), vec![pending_index]);

    // 已提交摘要被撤销计划标记失效；旧事实仍可查询，但同一摘要不能再保存。
    let mut journal = journal();
    let (index, payload) = stage(&mut journal, "revoked goal", 10, 12);
    let commit = commit_for(&journal, index);
    journal
        .commit(index, &commit, 4, 12, 8, 3, digest("inbox"), 1)
        .unwrap();
    journal.begin_next_compaction().unwrap();
    let plan = invalidation_plan();
    assert_eq!(journal.invalidate_from_plan(&plan).unwrap(), 1);
    let fact = &journal.applied_facts()[0];
    assert!(fact.is_invalidated());
    assert_eq!(
        fact.invalidated_by.as_deref(),
        Some(plan.plan_digest.as_str())
    );
    assert_eq!(
        journal.invalidated_summaries(),
        [declared_digest(&payload)].as_slice()
    );
    journal.validate().unwrap();
    assert_eq!(
        journal
            .stage(&payload, &declared_digest(&payload), 10, 12)
            .unwrap_err(),
        "compaction_summary_invalidated"
    );

    // 派生 cache 绑定同样按撤销计划失效，不能复用撤销前的内容。
    let cache = ContextCacheBinding::new(digest("prepared"), digest("cache"), 3).unwrap();
    let invalidated = cache.invalidate(&plan).unwrap();
    assert!(!invalidated.valid);
    assert_eq!(invalidated.data_epoch, 2);
    assert_eq!(
        invalidated.invalidated_by.as_deref(),
        Some(plan.plan_digest.as_str())
    );
}

/// Provider 原生 opaque compact：能力不明/不支持时 fail closed；切换模型必须重新编译上下文。
#[test]
fn native_opaque_compaction_requires_provider_capability_and_recompiles_on_model_switch() {
    let route = fixture_route();
    let run_id = RunId::new();
    for support in [CapabilitySupport::Unknown, CapabilitySupport::Unsupported] {
        assert_eq!(
            NativeCompactionBinding::new(support, run_id, &route, digest("plan"), digest("handle"))
                .unwrap_err(),
            "native_compaction_capability_unsupported"
        );
    }
    let binding = NativeCompactionBinding::new(
        CapabilitySupport::Supported,
        run_id,
        &route,
        digest("plan"),
        digest("handle"),
    )
    .unwrap();
    binding.reuse_for_route(&route, &digest("plan")).unwrap();

    let mut switched = fixture_route();
    switched.model_id = "builder-next".to_owned();
    assert_eq!(
        binding
            .reuse_for_route(&switched, &digest("plan"))
            .unwrap_err(),
        "native_compaction_route_changed"
    );
    assert_eq!(
        binding
            .reuse_for_route(&route, &digest("other-plan"))
            .unwrap_err(),
        "native_compaction_context_recompile_required"
    );

    let mut tampered = binding.clone();
    tampered.route_digest = digest("forged");
    assert_eq!(
        tampered.validate().unwrap_err(),
        "native_compaction_binding_digest_mismatch"
    );
}

/// 再成功：保存 artifact → CAS 提交 → 切换视图后，重启能从保存字节与已提交事实重建同一视图。
#[test]
fn compacted_context_rebuilds_to_same_view_after_restart() {
    let mut journal = journal();
    let (index, _payload) = stage(&mut journal, "committed goal", 10, 12);
    let commit = commit_for(&journal, index);
    journal
        .commit(index, &commit, 4, 12, 8, 3, digest("inbox"), 1)
        .unwrap();

    // 来源链：source event IDs、prompt/profile、budget 与旧/新 context revision 都留在事实里。
    let fact = &journal.applied_facts()[0];
    assert_eq!(fact.commit.source_event_ids.len(), 1);
    assert_eq!(fact.commit.prompt_bundle_digest, digest("prompt"));
    assert_eq!(fact.commit.model_profile, "builder-default");
    assert_eq!(fact.commit.budget_digest, digest("budget"));
    assert_eq!(fact.commit.source_context_revision, 4);
    assert_eq!(fact.commit.new_context_revision, 5);
    assert_eq!(fact.commit.source_cursor_start, 10);
    assert_eq!(fact.commit.source_cursor_end, 12);

    let prepared = digest("prepared-request");
    let first = journal.rebuild_view(&prepared, 3).unwrap();
    assert_eq!(first.context_revision, 5);
    assert_eq!(first.source_cursor, 12);

    // 重启：只靠序列化 journal（保存字节 + 已提交 checkpoint）重建同一视图。
    let encoded = serde_json::to_string(&journal).unwrap();
    let restarted: CompactionCommitJournal = serde_json::from_str(&encoded).unwrap();
    restarted.validate().unwrap();
    restarted.staged_artifacts()[index].verify().unwrap();
    let rebuilt = restarted.rebuild_view(&prepared, 3).unwrap();
    assert_eq!(first.view_digest, rebuilt.view_digest);
    assert_eq!(first.checkpoint_digest, rebuilt.checkpoint_digest);
    assert_eq!(rebuilt.inbox_digest, digest("inbox"));

    // 先拒绝：保存字节被篡改的 journal 不能被当成可重建视图。
    let mut tampered = journal.clone();
    tampered.staged[index].payload[0] ^= 0x01;
    assert_eq!(
        tampered.validate().unwrap_err(),
        "compaction_summary_digest_mismatch"
    );
}

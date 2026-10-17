//! H23 Core 守卫：artifact-first 提交顺序、来源链与失效传播的源码锚点，以及行为夹具的 CI 接线。
//!
//! 只做源码字符串核对：不运行夹具、不构造 DaemonHost、不读取远程回执。

#[test]
fn h23_compaction_commit_is_artifact_first_and_epoch_bound() {
    let domain = include_str!("../../kiana-domain/src/compaction_commit.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let protocol = include_str!("../../kiana-runner-protocol/src/lib.rs");
    let events = include_str!("../src/events.rs");
    for marker in [
        "CompactionArtifact",
        "CompactionCommit",
        "validate_before_commit",
        "compaction_source_changed",
        "compaction_workspace_changed",
        "compaction_data_epoch_changed",
        "source_event_ids",
        "append_idempotent_expected",
        "RunnerEvent::Compacted",
        "prompt_sources",
    ] {
        assert!(
            domain.contains(marker)
                || runner.contains(marker)
                || protocol.contains(marker)
                || events.contains(marker),
            "H23 compaction marker missing: {marker}"
        );
    }
    assert!(runner.contains("checkpoint"));
    assert!(!domain.contains("std::fs"));
}

/// 保存 → 校验 → CAS 提交 → 切换视图的顺序必须留在同一份确定性合同里，且未引用 artifact
/// 可以按正常 GC 回收；合同本身不读写文件系统、不起进程、不调用模型。
#[test]
fn h23_journal_is_artifact_first_gc_eligible_and_invalidation_aware() {
    let journal = include_str!("../../kiana-domain/src/compaction_journal.rs");
    for marker in [
        "CompactionCommitJournal",
        "StagedCompactionArtifact",
        "CompactionAppliedFact",
        "gc_candidates",
        "begin_next_compaction",
        "invalidate_from_plan",
        "ContextResumeView::rebuild_after_restart",
        "compaction_summary_digest_mismatch",
        "compaction_summary_invalidated",
        "compaction_journal_not_active",
        "NativeCompactionBinding",
        "native_compaction_capability_unsupported",
        "native_compaction_route_changed",
        "native_compaction_context_recompile_required",
        "MAX_STAGED_SUMMARY_BYTES",
    ] {
        assert!(
            journal.contains(marker),
            "H23 journal marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::process",
        "std::net",
        "tokio",
        "reqwest",
        "#[ignore",
    ] {
        assert!(
            !journal.contains(forbidden),
            "H23 journal must stay a pure contract: {forbidden}"
        );
    }
    // 真实字节持久化仍然是 Core 的 content-addressed artifact store port，journal 不自建存储。
    let store = include_str!("../src/artifact_store.rs");
    for marker in [
        "ContentAddressedArtifactStorePort",
        "stage_content",
        "commit_content",
        "verify_content",
    ] {
        assert!(
            store.contains(marker),
            "H23 artifact store marker missing: {marker}"
        );
    }
}

/// 接线守卫：把卡片命名的先拒绝/成功行为夹具绑定到 domain 行为夹具文件、baseline 与 H23
/// focused workflow，避免夹具被删/改名或漏出 CI。
#[test]
fn h23_behaviour_fixtures_are_wired_into_the_diagnostic_lane() {
    let behavior = include_str!("../../kiana-domain/tests/h23_compaction_commit.rs");
    let workflow = include_str!("../../.github/workflows/h23-compaction-commit-diagnostic.yml");
    let baseline = include_str!("../../docs/roadmap/h23-compaction-commit-baseline.md");

    let deny = [
        "crash_before_compaction_commit_keeps_old_context",
        "stale_summary_cannot_overwrite_new_steering",
        "revoked_source_invalidates_summary",
    ];
    let success = ["compacted_context_rebuilds_to_same_view_after_restart"];
    for fixture in deny.iter().chain(success.iter()) {
        assert!(
            behavior.contains(fixture),
            "H23 behavior fixture missing: {fixture}"
        );
        assert!(
            baseline.contains(fixture),
            "H23 baseline fixture missing: {fixture}"
        );
        assert!(
            workflow.contains(fixture),
            "H23 workflow fixture missing: {fixture}"
        );
    }

    // 先拒绝夹具必须排在成功夹具之前（卡片顺序：先证明拒绝路径）。
    let first_success = behavior
        .find(success[0])
        .expect("H23 success fixture is present");
    for fixture in deny {
        assert!(
            behavior.find(fixture).expect("H23 deny fixture is present") < first_success,
            "H23 deny fixture must precede the success fixture: {fixture}"
        );
    }

    // focused workflow 必须同时复核：domain 提交夹具、Core 守卫、fmt 门、按名 `--exact`
    // 重跑夹具，以及各 target 的 `--no-run` 编译步。
    for step in [
        "cargo fmt -p kiana-domain -p kiana-core --check",
        "cargo test -p kiana-domain --test h23_compaction_commit",
        "cargo test -p kiana-core --test h23_compaction_commit_guard",
    ] {
        assert!(
            workflow.contains(step),
            "H23 focused workflow step missing: {step}"
        );
    }
    assert!(workflow.contains("--exact"));
    assert!(workflow.contains("--no-run"));
    assert!(workflow.contains("--test-threads=1"));

    // 行为夹具必须真的驱动 journal 的顺序、GC 候选、失效传播与重启重建，而不是只检查类型存在。
    assert!(!behavior.contains("#[ignore"));
    for marker in [
        ".stage(",
        ".commit(",
        "gc_candidates",
        "rebuild_view",
        "invalidate_from_plan",
        "begin_next_compaction",
    ] {
        assert!(
            behavior.contains(marker),
            "H23 behavior marker missing: {marker}"
        );
    }
}

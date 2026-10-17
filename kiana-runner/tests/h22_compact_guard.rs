#[test]
fn h22_compaction_keeps_summary_evidence_and_complete_recent_groups() {
    let compact = include_str!("../src/compact.rs");
    let harness = include_str!("../src/harness.rs");
    let domain = include_str!("../../kiana-domain/src/compact_summary.rs");
    for marker in [
        "CompactSummary",
        "build_compacted_history_with_summary",
        "message_groups",
        "pending assistant/tool pair preserved",
        "compact_latest_group_exceeds_budget",
        "completed_action_refs",
        "verification_refs",
        "summary_digest",
    ] {
        assert!(
            compact.contains(marker) || harness.contains(marker) || domain.contains(marker),
            "H22 compaction marker missing: {marker}"
        );
    }
    assert!(!compact.contains("(no summary available)"));
    assert!(compact.contains("system"));
    assert!(harness.contains("PromptBundle::decode"));
    assert!(domain.contains("compact_summary_evidence_ref_invalid"));
}

/// 接线守卫：把卡片命名的先拒绝/成功行为夹具绑定到 runner 行为夹具文件、baseline 与
/// H22 focused workflow，避免夹具被删/改名或漏出 CI；它只做源码字符串核对，不运行夹具、
/// 不构造 DaemonHost，也不读取远程回执。
#[test]
fn h22_behaviour_fixtures_are_wired_into_the_diagnostic_lane() {
    let behavior = include_str!("h22_compaction_behavior.rs");
    let workflow = include_str!("../../.github/workflows/h22-compaction-diagnostic.yml");
    let baseline = include_str!("../../docs/roadmap/h22-compaction-baseline.md");

    let deny = ["h22_summary_cannot_forge_completed_tool_or_approval"];
    let success = [
        "h22_compaction_keeps_pending_pairs_and_latest_goal",
        "h22_agent_continues_task_after_two_compactions_without_repeating_completed_edit",
    ];
    for fixture in deny.iter().chain(success.iter()) {
        assert!(
            behavior.contains(fixture),
            "H22 behavior fixture missing: {fixture}"
        );
        assert!(
            baseline.contains(fixture),
            "H22 baseline fixture missing: {fixture}"
        );
        assert!(
            workflow.contains(fixture),
            "H22 workflow fixture missing: {fixture}"
        );
    }

    // 先拒绝夹具必须排在成功夹具之前（卡片顺序：先证明拒绝路径）。
    let first_success = behavior
        .find(success[0])
        .expect("H22 success fixture is present");
    for fixture in deny {
        assert!(
            behavior.find(fixture).expect("H22 deny fixture is present") < first_success,
            "H22 deny fixture must precede the success fixtures: {fixture}"
        );
    }

    // focused workflow 必须同时复核：域合同、runner 源码守卫与行为夹具，并按名
    // `--exact` 重跑夹具、为各 target 提供 `--no-run` 编译步。
    for step in [
        "cargo fmt -p kiana-domain -p kiana-runner --check",
        "cargo test -p kiana-domain --test h22_compact_summary",
        "cargo test -p kiana-runner --test h22_compact_guard",
        "cargo test -p kiana-runner --test h22_compaction_behavior",
    ] {
        assert!(
            workflow.contains(step),
            "H22 focused workflow step missing: {step}"
        );
    }
    assert!(workflow.contains("--exact"));
    assert!(workflow.contains("--no-run"));
    assert!(workflow.contains("--test-threads=1"));

    // 行为夹具必须是可运行的验收测试：不得被 ignore，且必须真正驱动 harness、按 purpose
    // 区分的 fake provider、服务端证据绑定与压缩解释。
    assert!(!behavior.contains("#[ignore"));
    assert!(behavior.contains("bind_compaction_evidence"));
    assert!(behavior.contains("compaction_explanation"));
    assert!(behavior.contains("ModelPurpose::Compaction"));
    assert!(behavior.contains("RunnerCommand::CapabilityResult"));

    // 三段式压缩的每一段都必须留在共享实现里，且摘要失败路径必须让运行暂停。
    assert!(compact().contains("pub fn plan_compaction"));
    assert!(compact().contains("pub fn summary_request_messages"));
    assert!(compact().contains("pub fn parse_compaction_summary"));
    assert!(compact().contains("pub fn validate_summary_for_plan"));
    assert!(compact().contains("pub fn build_compacted_view"));
    assert!(compact().contains("EXTERNALIZE_RESULT_TOKENS"));
    assert!(compact().contains("compact_summary_unavailable"));
    assert!(harness().contains("compact_history"));
    assert!(harness().contains("ModelPurpose::Compaction"));
    assert!(harness().contains("COMPACT_SUMMARY_BUDGET_TOKENS"));
}

fn compact() -> &'static str {
    include_str!("../src/compact.rs")
}

fn harness() -> &'static str {
    include_str!("../src/harness.rs")
}

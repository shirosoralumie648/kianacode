#[test]
fn question_contract_is_separate_from_approval_contract() {
    let domain = include_str!("../../kiana-domain/src/clarification.rs");
    let core = include_str!("../src/clarification.rs");
    let approval = include_str!("../src/approval_binding.rs");
    for marker in [
        "InteractionId",
        "turn_id",
        "question",
        "options",
        "required",
        "expires_at_unix_ms",
        "cancel_policy",
        "ClarificationStatus",
        "accept_answer",
        "clarification_not_pending",
        "clarification_answer_interaction_mismatch",
        "clarification_answer_turn_mismatch",
    ] {
        assert!(
            domain.contains(marker),
            "missing clarification marker: {marker}"
        );
    }
    for forbidden in ["ApprovalId", "CapabilityRequest", "CapabilityGrant"] {
        assert!(
            !domain.contains(forbidden),
            "question contract widened into authority: {forbidden}"
        );
    }
    for marker in [
        "commit_clarification_answer",
        "ClarificationResolution",
        "clarification_human_inbox_item",
        "HumanInboxKind::Question",
        "run.clarification.answer",
    ] {
        assert!(
            core.contains(marker),
            "missing core clarification marker: {marker}"
        );
    }
    let lifecycle = include_str!("../src/lifecycle.rs");
    let events = include_str!("../../kiana-domain/src/event_contracts.rs");
    for marker in [
        "RunnerEvent::ClarificationRequested",
        "run.clarification.requested",
        "CLARIFICATION_WAITING_STATUS",
        "checkpoint_run",
        "interaction_id",
    ] {
        assert!(
            lifecycle.contains(marker),
            "missing Core lifecycle clarification marker: {marker}"
        );
    }
    assert!(events.contains("run.clarification.requested"));
    assert!(events.contains("CLARIFICATION_IDS"));
    for forbidden in [
        "ApprovalId",
        "CapabilityRequest",
        "CapabilityBrokerPort",
        "ApprovalStorePort",
    ] {
        assert!(
            !core.contains(forbidden),
            "core question path consumed authority: {forbidden}"
        );
    }
    for marker in [
        "ApprovalBinding",
        "approval_id",
        "approval_inbox_not_approved",
        "consume",
    ] {
        assert!(
            approval.contains(marker),
            "approval contract marker missing: {marker}"
        );
    }
}

#[test]
fn runner_waits_for_one_question_and_rejects_duplicate_or_foreign_answer() {
    let runner = include_str!("../../kiana-runner/src/state_driver.rs");
    for marker in [
        "AwaitingInput",
        "pending_interaction_id",
        "AwaitClarification",
        "AnswerClarification",
        "ClarificationNotPending",
        "WaitForInput",
        "HarnessPhase::AwaitingInput",
    ] {
        assert!(
            runner.contains(marker),
            "missing Runner clarification fence: {marker}"
        );
    }
}

#[test]
fn restart_material_keeps_the_original_driver_and_inbox_route() {
    let harness = include_str!("../../kiana-runner/src/harness.rs");
    let recovery = include_str!("../src/recovery.rs");
    for marker in [
        "driver: Some(run.driver.clone())",
        "pending_clarification",
        "driver",
        "inbox",
    ] {
        assert!(
            harness.contains(marker),
            "checkpoint lost clarification material: {marker}"
        );
    }
    for marker in ["resume_run", "run.resume_prepared", "explicit"] {
        assert!(
            recovery.contains(marker),
            "recovery boundary missing: {marker}"
        );
    }
}

#[test]
fn tty_and_web_use_the_same_human_inbox_projection() {
    let core = include_str!("../src/platform.rs");
    let workbench = include_str!("../../kiana-entrypoints/src/workbench_chat.rs");
    let web = include_str!("../../kiana-entrypoints/src/web.rs");
    let page = include_str!("../../kiana-entrypoints/src/web_page.html");
    assert!(core.contains("human.inbox"));
    assert!(workbench.contains("human.inbox"));
    assert!(web.contains("human.inbox"));
    assert!(page.contains("queryCommand('human.inbox'"));
    assert!(page.contains("question:'澄清'"));
}

#[test]
fn execution_identity_and_turn_boundaries_are_server_owned() {
    let lifecycle = include_str!("../src/lifecycle.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let recovery = include_str!("../src/recovery.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let projection = include_str!("../src/invocation_projection.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let roadmap = include_str!("../../docs/roadmap/control-plane.md");
    let baseline = include_str!("../../docs/roadmap/control-plane-execution-identity-baseline.md");

    assert!(lifecycle.contains("continue_new_turn"));
    assert!(lifecycle.contains("run.predecessor"));
    assert!(lifecycle.contains("TurnIdentity"));
    assert!(lifecycle.contains("LegacyContinue"));
    assert!(lifecycle.contains("run_not_terminal_use_resume"));
    assert!(lifecycle.contains("run_unknown_requires_reconciliation"));
    assert!(dispatch.contains("InvocationIdentity"));
    assert!(dispatch.contains("typed_invocation_identity"));
    assert!(dispatch.contains("execution.prepared"));
    assert!(dispatch.contains("invocation.executing"));
    assert!(recovery.contains("TurnSemantics::Resume"));
    assert!(recovery.contains("run.resume_prepared"));
    assert!(capabilities.contains("invocation_id"));
    assert!(capabilities.contains("call_id"));
    assert!(capabilities.contains("attempt"));
    assert!(projection.contains("invocation_terminal_conflict"));
    assert!(projection.contains("request_id_missing"));
    assert!(protocol.contains("run.turn.v2"));
    assert!(protocol.contains("ContinueRequest"));
    assert!(protocol.contains("ResumeRequest"));
    assert!(roadmap.contains("新用户 Continue 创建新 Turn/Run"));
    assert!(roadmap.contains("legacy"));
    assert!(baseline.contains("call_id"));
    assert!(baseline.contains("New Turn"));
    assert!(baseline.contains("Resume"));
}

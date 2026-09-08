use kiana_workflow::WorkflowState;

#[test]
fn every_workflow_edge_is_explicitly_allowlisted() {
    // 用完整边表锁定工作流，新增状态默认没有转移权限，防止绕过审批或终态。
    let states = [
        WorkflowState::Requested,
        WorkflowState::Running,
        WorkflowState::AwaitingApproval,
        WorkflowState::Blocked,
        WorkflowState::Completed,
        WorkflowState::Failed,
        WorkflowState::ResultUnknown,
    ];
    let allowed = [
        (WorkflowState::Requested, WorkflowState::Running),
        (WorkflowState::Requested, WorkflowState::Blocked),
        (WorkflowState::Running, WorkflowState::AwaitingApproval),
        (WorkflowState::Running, WorkflowState::Completed),
        (WorkflowState::Running, WorkflowState::Failed),
        (WorkflowState::Running, WorkflowState::ResultUnknown),
        (WorkflowState::AwaitingApproval, WorkflowState::Running),
        (WorkflowState::AwaitingApproval, WorkflowState::Blocked),
    ];

    for from in states {
        for to in states {
            let expected = allowed.contains(&(from, to));
            assert_eq!(
                from.transition(to).is_ok(),
                expected,
                "unexpected workflow edge: {from:?}->{to:?}"
            );
        }
    }
}

#[test]
fn terminal_states_are_immutable_and_have_stable_wire_names() {
    // 终态收到任何迟到事件都必须保持终态，序列化名称同时作为线协议契约。
    for state in [
        WorkflowState::Blocked,
        WorkflowState::Completed,
        WorkflowState::Failed,
        WorkflowState::ResultUnknown,
    ] {
        assert!(state.is_terminal());
        assert!(state.transition(WorkflowState::Running).is_err());
        let encoded = serde_json::to_string(&state).unwrap();
        let expected = match state {
            WorkflowState::Blocked => "blocked",
            WorkflowState::Completed => "completed",
            WorkflowState::Failed => "failed",
            WorkflowState::ResultUnknown => "result_unknown",
            _ => unreachable!(),
        };
        assert_eq!(encoded, format!("\"{expected}\""));
    }
}

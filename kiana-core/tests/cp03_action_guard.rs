//! CP-03 跨层动作描述与执行边界的静态源码守卫。
//!
//! 本测试通过 `include_str!` 检查 Domain/Core/Broker/Daemon 文件包含约定标记，不执行动作准备、
//! 权限裁决或 handler；它防止关键接缝从源码中消失，但不是运行时路径证明。
/// 检查 Catalog metadata 会贯穿准备、Broker binding 和实际 handler 注册边界。
#[test]
fn action_catalog_preparation_and_execution_share_server_metadata() {
    let actions = include_str!("../../kiana-domain/src/actions.rs");
    let catalog = include_str!("../../kiana-domain/src/tool_catalog.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let approvals = include_str!("../src/approvals.rs");
    let events = include_str!("../src/events.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let runner_tools = include_str!("../../kiana-runner/src/tools.rs");
    let daemon_handlers = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let baseline = include_str!("../../docs/roadmap/control-plane-action-baseline.md");

    for marker in [
        "ACTION_OPERATIONS",
        "CapabilityActionDescriptor",
        "minimum_risk",
        "argument_schema",
        "result_schema",
        "resource_fields",
        "cancellation",
        "reconciliation",
        "idempotency",
        "PreparedAction",
        "validate_action_catalog",
        "action_catalog_changed",
        "capability_action_digest",
        "action_risk_downgrade",
        "action_capability_mismatch",
    ] {
        assert!(
            actions.contains(marker),
            "missing action contract marker {marker}"
        );
    }
    assert!(catalog.contains("parse_bounded_json"));
    assert!(catalog.contains("json_duplicate_key"));
    assert!(catalog.contains("json_number_invalid"));
    assert!(capabilities.contains("PreparedAction::new"));
    assert!(capabilities.contains("stamp_request_identity"));
    assert!(capabilities.contains("pin_action_authority"));
    assert!(events.contains("stamp_request_identity"));
    assert!(dispatch.contains("action_digest"));
    assert!(dispatch.contains("typed_invocation_identity"));
    assert!(runner_tools.contains("tool_schemas"));
    assert!(approvals.contains("authorize_and_execute"));
    assert!(daemon_handlers.contains("CapabilityBroker"));
    assert!(baseline.contains("cp_forged_readonly_risk_cannot_downgrade_registered_effect"));
    assert!(baseline.contains("additionalProperties"));
    assert!(baseline.contains("direct Company"));
    assert!(baseline.contains("不让模型自报 risk"));
}

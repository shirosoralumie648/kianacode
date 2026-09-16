#[test]
fn runner_and_daemon_consume_the_single_tool_authority_registry() {
    let authority = include_str!("../../kiana-domain/src/tool_authority.rs");
    let catalog = include_str!("../../kiana-domain/src/tool_catalog.rs");
    let runner = include_str!("../../kiana-runner/src/tools.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let contracts = include_str!("../../kiana-domain/src/contracts.rs");
    for marker in [
        "pub struct ToolSpec",
        "pub const TOOL_SPECS",
        "pub fn tool_spec",
        "validate_tool_authority",
        "side_effecting",
        "risk_policy",
    ] {
        assert!(
            authority.contains(marker),
            "authority marker missing: {marker}"
        );
    }
    assert!(catalog.contains("crate::tool_authority::tool_spec(name)"));
    for marker in [
        "let canonical = model_tool_name(&call.name)",
        "match canonical",
        "TOOL_SHELL =>",
        "TOOL_APPLY_PATCH =>",
        "TOOL_MCP =>",
    ] {
        assert!(
            runner.contains(marker),
            "runner registry marker missing: {marker}"
        );
    }
    assert!(daemon.contains("kiana_domain::validate_tool_authority()?"));
    assert!(contracts.contains("kiana.tool-authority.v1"));
}

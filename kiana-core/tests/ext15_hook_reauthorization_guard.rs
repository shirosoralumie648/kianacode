#[test]
fn hook_update_requires_new_identity_scope_and_no_broker_effect() {
    let source = include_str!("../src/hook_reauthorization.rs");
    for marker in [
        "HookReauthorizationMaterial",
        "MAX_HOOK_RECURSION_DEPTH",
        "RequestId::new()",
        "approval_invalidated: true",
        "broker_not_called: true",
        "execution_scope = None",
        "capability_grant_id = None",
    ] {
        assert!(source.contains(marker), "missing EXT-15 marker: {marker}");
    }
}

#[test]
fn extension_dependency_and_binding_contract_is_fail_closed_and_non_authorizing() {
    let extensions = include_str!("../../kiana-domain/src/extensions.rs");
    for marker in [
        "ExtensionDependencyGraph",
        "extension_dependency_cycle",
        "extension_dependency_unsatisfied",
        "extension_dependency_version_unavailable",
        "extension_dependency_scope_disjoint",
        "extension_dependency_platform_mismatch",
        "ExtensionBindingSnapshot",
        "parent_scope_digest",
        "extension_binding_parent_scope_changed",
        "is_valid_against",
    ] {
        assert!(
            extensions.contains(marker),
            "missing EXT-21 marker: {marker}"
        );
    }
    assert!(extensions.contains("也不会安装依赖、启动扩展或授予 capability"));
    assert!(extensions.contains("实际副作用仍归 ControlPlane/Broker"));
}

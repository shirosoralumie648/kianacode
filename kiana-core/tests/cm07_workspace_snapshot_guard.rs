//! CM-07 Core 静态源码边界守卫。
//!
//! `include_str!` 读取 Domain、Query 和 context input 源文件并检查关键类型/标记仍存在。
//! 这是编译期测试源码字符串断言，不会执行扫描器，也不能证明 caller 已验证 ProjectTrust 或把 snapshot 接入请求路径。

#[test]
/// 保留 trust、identity、资源预算及“扫描读取不等于执行能力”的源码边界标记。
fn workspace_snapshot_keeps_trust_identity_and_read_limits_explicit() {
    let domain = include_str!("../../kiana-domain/src/workspace_snapshot.rs");
    let query = include_str!("../../kiana-query/src/workspace_snapshot.rs");
    let context = include_str!("../../kiana-query/src/context_inputs.rs");
    for marker in [
        "WorkspaceTrust",
        "WorkspaceSnapshotLimits",
        "WorkspaceFileIdentity",
        "same_file",
        "WorkspaceReadDisposition",
        "instruction_safe",
        "workspace_change_between_scan_and_read",
        "hardlink_not_read",
        "symlink_not_read",
        "max_file_bytes",
        "max_elapsed_ms",
        "deny_unknown_fields",
    ] {
        assert!(
            domain.contains(marker) || query.contains(marker),
            "CM-07 marker missing: {marker}"
        );
    }
    assert!(query.contains("symlink_metadata"));
    assert!(query.contains("read_workspace_snapshot"));
    assert!(context.contains("SourceSnapshot"));
    assert!(!query.contains("ModelClient"));
    assert!(!query.contains("CapabilityBroker"));
}

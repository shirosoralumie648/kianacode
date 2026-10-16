//! PD-20 源码边界的静态文本守卫。
//!
//! `include_str!` 把 Query 实现源码作为普通文本读入，然后检查关键合同标记仍然存在，
//! 并确认 manifest 模块没有出现指定的执行层耦合词。此测试不构建清单、不读写项目文件，
//! 也不证明这些 API 已由生产请求路径调用。

#[test]
fn pd20_manifest_guard_keeps_context_material_source_bound() {
    let manifest = include_str!("../src/context_manifest.rs");
    let query = include_str!("../src/index.rs");
    // marker 用来防止清单与其上游数据结构/原子写 helper 被无意移除；只证明文本存在，
    // 不检查 marker 所在代码是否可达、是否符合业务语义或运行时是否成功。
    for marker in [
        "ContextMaterialManifest",
        "source_cursor",
        "content_hash",
        "dependency_graph",
        "context_material_root_mismatch",
        "atomic_write_json",
    ] {
        assert!(
            manifest.contains(marker) || query.contains(marker),
            "missing marker: {marker}"
        );
    }
    // 这些断言限制本模块的静态依赖边界；它们不是全 crate 的依赖分析或安全证明。
    assert!(!manifest.contains("CapabilityBroker"));
    assert!(!manifest.contains("ProviderClient"));
}

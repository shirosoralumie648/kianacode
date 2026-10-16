//! PD-20 上下文材料清单的文件系统行为 fixture。
//!
//! 测试通过小型临时项目构建 RepoMap 和 artifact store，再检查清单写入/读回及外层校验。
//! 它验证的是当前 Query helper 的本地行为；不会经过生产 Router、ControlPlane、EventLog，
//! 也不证明 source cursor 的来源真实、报告与后续磁盘内容保持同步或写入具备跨进程 CAS。

use kiana_query::{
    read_context_material_manifest, write_context_material_manifest_atomic, ContextArtifactOptions,
    ContextMaterialManifest, RepoMapOptions,
};
use std::fs;
use std::path::PathBuf;

/// 为两个用例创建带少量源码和文档的临时仓库。
///
/// 路径只由当前进程 ID 决定，方便 fixture 找回固定位置，但同一测试进程中的并行用例会
/// 共用此目录；`remove_dir_all` 和写文件可能互相干扰。测试串行时可避免这类并发争用。
fn root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("kiana-pd20-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn one() {}\n").unwrap();
    fs::write(root.join("README.md"), "# PD-20\n").unwrap();
    root
}

/// 验证构建结果能写为 JSON 并读回，同时 RepoMap、artifact store 与依赖图使用同一个根。
///
/// 输入是合成目录、固定游标 `7` 和测试工具版本；断言只比较清单字段与摘要往返，不验证
/// source cursor 的 EventLog 来源，也不验证原子写 helper 在断电、多进程竞争中的完整保证。
#[test]
fn material_manifest_binds_repo_map_artifacts_and_dependency_graph() {
    let root = root();
    let manifest = ContextMaterialManifest::build(
        &root,
        RepoMapOptions::default(),
        ContextArtifactOptions::default(),
        7,
        "kiana-query-pd20",
    )
    .unwrap();
    assert_eq!(manifest.source_cursor, 7);
    assert_eq!(manifest.repo_map.root, manifest.root);
    assert_eq!(manifest.artifact_store.root, manifest.root);
    assert_eq!(manifest.artifact_store.dependency_graph.root, manifest.root);

    let path = root.join(".kiana/context-material-manifest.json");
    write_context_material_manifest_atomic(&path, &manifest).unwrap();
    let loaded = read_context_material_manifest(&path).unwrap();
    assert_eq!(loaded.manifest_digest, manifest.manifest_digest);
    let _ = fs::remove_dir_all(root);
}

/// 分别验证顶层根绑定和文件摘要文本格式拒绝路径。
///
/// 第二个对象在修改 content hash 后重新计算 manifest digest，因此此断言能先到达文件摘要
/// 格式检查；它不验证错误摘要与磁盘文件字节不匹配时会被重新计算出来，因为读取清单不会扫磁盘。
#[test]
fn tampered_root_or_content_hash_is_rejected() {
    let root = root();
    let manifest = ContextMaterialManifest::build(
        &root,
        RepoMapOptions::default(),
        ContextArtifactOptions::default(),
        1,
        "kiana-query-pd20",
    )
    .unwrap();
    let mut forged = manifest.clone();
    forged.root = "/foreign".to_owned();
    assert_eq!(
        forged.validate().unwrap_err(),
        "context_material_root_mismatch"
    );
    let mut forged_hash = manifest;
    forged_hash.repo_map.files[0].content_hash = "not-a-digest".to_owned();
    forged_hash.manifest_digest = forged_hash.digest();
    assert_eq!(
        forged_hash.validate().unwrap_err(),
        "context_material_repo_content_hash_invalid"
    );
    let _ = fs::remove_dir_all(root);
}

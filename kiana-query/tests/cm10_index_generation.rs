//! CM-10 Query 文件发布边界的集成 fixture。
//!
//! 该测试写入系统临时目录并回读 manifest，覆盖 Building 拒绝和 Ready 原子发布的接口路径；
//! 它不是生产索引 reader 或崩溃/掉电耐久性演练。

use kiana_domain::{IndexComponentKind, IndexGenerationState};
use kiana_query::{read_index_manifest, write_index_manifest_atomic};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

/// 用重复字符构造格式正确的测试摘要，不对应真实源文件或组件内容。
fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

/// 为 domain manifest 准备四种合成组件摘要。
fn components(value: char) -> BTreeMap<String, String> {
    IndexComponentKind::ALL
        .into_iter()
        .map(|kind| (kind.as_str().to_owned(), digest(value)))
        .collect()
}

/// 创建当前进程共用的临时目录。
///
/// ⚠ 路径只含 process ID，因此同一测试进程并行运行两个用例时会共享目录；若其中一个先清理，另一个可能受影响。
/// 当前注释只记录 fixture 约束，没有改变目录命名或测试执行方式。
fn root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("kiana-cm10-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

/// 用真实临时文件验证 Ready manifest 通过 temp-file、sync、rename 后可以解码回同一 DTO。
#[test]
fn ready_manifest_publishes_by_temp_sync_and_atomic_rename() {
    let root = root();
    let path = root.join("index/manifest.json");
    let mut state = IndexGenerationState::new();
    let building = state.begin(digest('a'), components('b')).unwrap();
    let ready = state.commit(building).unwrap();
    write_index_manifest_atomic(&path, &ready).unwrap();
    let loaded = read_index_manifest(&path).unwrap();
    assert_eq!(loaded, ready);
    let _ = fs::remove_dir_all(root);
}

/// 验证 Building manifest 在创建临时文件前被拒绝，目标路径不会被发布。
#[test]
fn building_manifest_cannot_be_published_as_ready() {
    let root = root();
    let path = root.join("manifest.json");
    let mut state = IndexGenerationState::new();
    let building = state.begin(digest('a'), components('b')).unwrap();
    assert!(write_index_manifest_atomic(&path, &building).is_err());
    assert!(!path.exists());
    let _ = fs::remove_dir_all(root);
}

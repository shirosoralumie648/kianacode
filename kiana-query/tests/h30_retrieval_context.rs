//! H30 Query adapter 到统一检索候选合同的行为 fixture。
//!
//! 文件摘要、根目录和权限 scope 都由测试直接构造；测试调用 adapter 后再调用候选合同验证。
//! 它不运行 RepoMap 扫描器、不读取工作区文件、不编译 ContextPlan，也不证明生产入口已接线。

use kiana_domain::{ContextMaterialClass, RetrievalSourceKind};
use kiana_query::{repo_map_candidate, RepoMapFile};

/// 检查 RepoMap 摘要会被标注为工作区文件来源和项目知识候选。
#[test]
fn repo_map_results_enter_the_shared_context_candidate_contract() {
    // 这是合成 RepoMapFile；`sha256:` 值只用于满足 Domain 合同，不是由本 fixture 对真实文件计算。
    let file = RepoMapFile {
        content_hash: "sha256:".to_owned() + &"a".repeat(64),
        path: "src/lib.rs".to_owned(),
        language: Some("rust".to_owned()),
        bytes: 128,
        estimated_tokens: 32,
        symbols: vec!["fn main".to_owned()],
    };
    // adapter 使用调用方提供的根路径、scope 和数据 epoch 生成候选 DTO。
    let candidate = repo_map_candidate("/repo", &file, "project:read", 4).unwrap();
    candidate.validate().unwrap();
    assert_eq!(candidate.source_kind, RetrievalSourceKind::RepoMap);
    assert_eq!(
        candidate.material_class,
        ContextMaterialClass::ProjectKnowledge
    );
    assert_eq!(
        candidate.source_snapshot.freshness,
        kiana_domain::Freshness::Current
    );
}

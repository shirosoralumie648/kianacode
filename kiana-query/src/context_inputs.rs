//! 本文件把 Query 层的 RepoMap 文件摘要转换成 Domain 统一的检索候选合同。
//!
//! 在整个系统中的位置：RepoMap 扫描器负责读取工作区并产生 `RepoMapFile`；本 adapter 把该摘要
//! 包装成 Domain 的 `SourceRef`、`SourceSnapshot` 和 `RetrievalCandidate`，供后续候选整理流程
//! 转成唯一的 `ContextPlan` 输入形状。当前仓内直接调用点是 `kiana-query/tests/h30_retrieval_context.rs`；
//! crate 根模块公开 re-export 了此函数，但尚未找到生产请求路径调用。
//!
//! ```text
//! 工作区文件 -> repo_map::build_repo_map() -> RepoMapFile
//!            -> repo_map_candidate(root, file, permission_scope, data_epoch)
//!            -> SourceRef + SourceSnapshot + RetrievalCandidate
//!            -> Domain RetrievalPack / ContextPlan 候选流程
//! ```
//!
//! 本文件只整理已经传入的摘要，不会重新打开文件、读取 Memory、校验项目授权或执行能力。
//! `permission_scope` 和 `data_epoch` 只参与候选附带的摘要元数据；它们不是本函数完成的 ACL
//!（访问控制列表）判定。构造出的 `Freshness::Current` 也只是 DTO 字段，adapter 没有重新扫描
//! 文件来确认 RepoMap 仍然新鲜。

use crate::repo_map::RepoMapFile;
use kiana_domain::{
    json_digest, ContextMaterialClass, EvidenceStatus, Freshness, RetrievalCandidate,
    RetrievalSourceKind, SourceKind, SourceRef, SourceSnapshot,
};
use serde_json::json;

/// 把一个 RepoMap 文件摘要包装成 Domain 可接收的统一检索候选。
///
/// 【调用者】当前仓内直接调用者是 H30 Query fixture；公开 re-export 允许其他 Query 使用者调用，
/// 但仓内未找到生产路由直接使用它。之后 Domain 的 retrieval pack 可把 `RetrievalCandidate`
/// 转成 `ContextPlan` 候选；本函数本身不负责 pack 聚合或 ContextPlan 编译。
/// 【输入】`root` 是摘要所属仓库的根路径文本；`file` 带相对路径、语言、字节数、粗略 token 数、
/// 符号名和可选内容摘要；`permission_scope` 与 `data_epoch` 由调用方提供。
/// 【输出】成功返回标记为 RepoMap / ProjectKnowledge 的 `RetrievalCandidate`；其文本只包含路径、
/// 语言、字节数和符号摘要，不包含源文件正文。
/// 【数据流】优先沿用带 `sha256:` 前缀的文件摘要；无此前缀时以 path+symbols 计算一个替代摘要；
/// 再用摘要构造 WorkspaceFile `SourceRef`，嵌入 Current/Attributed `SourceSnapshot`，最后调用
/// Domain `RetrievalCandidate::new()` 计算并校验候选自摘要。
/// 【副作用】只在内存中创建 DTO，不访问文件系统、网络、Memory 或数据库。
/// 【失败情况】`SourceRef::new()` 会拒绝格式错误的摘要及无效来源文本；`SourceSnapshot::new()`
/// 会拒绝无效来源/快照合同；`RetrievalCandidate::new()` 会拒绝超长文本、非法 scope/ACL digest、
/// 错误的 source kind 等合同问题。错误字符串原样向调用者返回，不会自动修复候选。
/// 【设计原因】统一来源、证据、新鲜度和 scope 字段后，后续规划器能用同一合同处理不同来源的
/// 材料；它仍然是低信任检索材料，不能因为进入候选结构就升级成 Product 指令或已授权读取。
/// 【边界】这里把 `Freshness` 固定为 `Current`、证据固定为 `Attributed`，没有核对生成摘要之后
/// 文件是否变化，也不验证 `file.path` 是否仍在 `root` 下。调用方必须确保输入确实来自可信的
/// RepoMap 扫描结果，并在真正读取/使用数据时经过对应的权限与新鲜度检查。
pub fn repo_map_candidate(
    root: &str,
    file: &RepoMapFile,
    permission_scope: &str,
    data_epoch: u64,
) -> Result<RetrievalCandidate, String> {
    // 正常 RepoMap scanner 会给出原始文件字节的 sha256 摘要。兼容没有此前缀的旧/合成对象时，
    // 这里只能用 path 和符号名生成摘要；它不覆盖文件正文、语言、大小或源文件真实性。
    // 带有此前缀但格式错误的值不会走 fallback，而会在 SourceRef::new() 中被拒绝。
    let content_digest = if file.content_hash.starts_with("sha256:") {
        file.content_hash.clone()
    } else {
        json_digest(&json!({"path":file.path,"symbols":file.symbols}))
    };
    // `source_id` 和 revision 都从路径/摘要派生，source_cursor 留空表示此 adapter 没有 EventLog 游标。
    // Attributed 代表来源被标注，不代表摘要经过签名或外部事实验证。
    let source = SourceRef::new(
        format!("repo-map:{}", file.path),
        SourceKind::WorkspaceFile,
        format!("{root}/{}", file.path),
        content_digest.clone(),
        content_digest,
        None,
        EvidenceStatus::Attributed,
    )?;
    // Snapshot 记录本次包装采用的状态，但不检查文件在扫描之后是否又发生变化。
    let snapshot =
        SourceSnapshot::new(source, Freshness::Current, EvidenceStatus::Attributed, None)?;
    // `500` 使用 Domain 的 0..=1000 relevance_milli 标度，是固定的中间优先级提示，不是检索模型评分。
    // ACL digest 只是对调用方给的 scope 和 epoch 做摘要绑定；真正的访问许可必须由外层权威判断。
    RetrievalCandidate::new(
        format!("repo-map:{}", file.path),
        RetrievalSourceKind::RepoMap,
        ContextMaterialClass::ProjectKnowledge,
        format!(
            "{} ({:?}, {} bytes): {}",
            file.path,
            file.language,
            file.bytes,
            file.symbols.join(", ")
        ),
        snapshot,
        permission_scope,
        json_digest(&json!({"scope": permission_scope, "data_epoch": data_epoch})),
        500,
        vec![format!("source:repo-map:{}", file.path)],
    )
}

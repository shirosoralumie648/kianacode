//! 将既有只读 RepoMap 转为任务相关候选的 Query 适配器。
//!
//! 仓库结构扫描器（RepoMap scanner）负责遍历项目文本文件并生成 `RepoMapFile`；本文件不重复扫描，也不读取
//! 文件，而是把已有 `RepoMap` 与任务文本转换为 Domain 的 `RepoMapTaskCandidate`，再让
//! Domain 按估算词元（token）预算做选择。数据流为：
//!
//! ```text
//! build_repo_map() → RepoMap.files
//!     → 本文件计算路径/符号文本匹配分数、标注启发式符号、构造候选
//!     → RepoMapTaskSelection::new() 稳定排序并按估算预算筛选
//! ```
//!
//! `build_repo_map()` 本身有 Daemon/Direct Connect 使用者，但当前搜索只找到测试调用
//! `select_repo_map_for_task()`；因此 task-lens 还未接入生产请求主链。函数不调用模型、不
//! 启动工具、不读取或写入索引，也不授予访问权限。

use crate::repo_map::RepoMap;
use kiana_domain::{
    json_digest, memory_tokens, RepoMapSymbol, RepoMapTaskCandidate, RepoMapTaskSelection,
};
use serde_json::json;

/// 按任务词项为已有 RepoMap 文件打分，并请求 Domain 生成预算受限的选择结果。
///
/// 【调用者】当前只见 `kiana-query/tests/cm13_repo_map_task.rs` fixture 调用；函数从 Query
/// crate 公开导出，但没有发现生产 route/handler 调用它。
///
/// 【输入】
/// - `map`：上游 `build_repo_map()` 生成的只读文件摘要集合；已经被 scanner token 预算截断的
///   文件不会在本函数中重新扫描出来。
/// - `task`：用来提取检索词的用户任务文本；Domain 报告只保存它的 digest。
/// - `token_budget`：候选选择阶段可使用的估算 token 上限；0 会由 Domain constructor 拒绝。
///
/// 【核心流程】`memory_tokens()` 按 Unicode 小写化并拆分普通词、中文重叠二元词；对每个
/// 文件把路径和符号拼成 haystack，再按词项位置给 substring 命中数加权。随后所有符号都
/// 标为 Heuristic，候选只继承现有内容摘要/语言/估算 token，依赖边列表留空，最后交给
/// Domain 统一排序、预算筛选、遗漏记录和摘要计算。
///
/// 【输出/失败】成功返回 `RepoMapTaskSelection`；符号文本不合法、候选字段无效或预算不合法
/// 时传递 Domain 返回的 `String` 错误。不会自动重试或修改 RepoMap。
///
/// 【设计原因与边界】把 task 相关排序与 scanner 分开，可重用只读扫描结果；但分数只是路径/
/// 符号文本的子串匹配，不是语法分析、依赖图或语义相关度。`estimated_tokens` 也是粗估，
/// 不能用于供应商精确计费或授权判断。
pub fn select_repo_map_for_task(
    map: &RepoMap,
    task: impl Into<String>,
    token_budget: u64,
) -> Result<RepoMapTaskSelection, String> {
    let task = task.into();
    // 保留 token 顺序，因为 Domain 适配器按索引给更靠前的任务词更高权重。
    let terms = memory_tokens(&task);
    let candidates = map
        .files
        .iter()
        .map(|file| {
            // 只使用相对路径和 scanner 已提取的符号名；不把整份源码塞入候选或重新读取。
            // `matches()` 按子串计数，不要求词项恰好位于语言语法边界。
            let haystack = format!("{} {}", file.path, file.symbols.join(" ")).to_ascii_lowercase();
            let task_score = terms
                .iter()
                .enumerate()
                .map(|(index, term)| {
                    let matches = haystack.matches(term).count() as u64;
                    matches.saturating_mul((terms.len() - index) as u64)
                })
                .sum();
            let symbols = file
                .symbols
                .iter()
                // Scanner 的符号识别是启发式文本处理，因此这里不能标记为 Declared。
                .map(|symbol| RepoMapSymbol::heuristic(symbol.clone()))
                .collect::<Result<Vec<_>, _>>()?;
            // 正常 scanner 会提供源码字节摘要；旧版/手工 RepoMap 没有 `sha256:` 前缀时，
            // 退化为对路径与符号列表求摘要。带此前缀但格式错误的值会交给 Domain 拒绝。
            // 这个 fallback 不代表源码内容 hash。
            let content_digest = if file.content_hash.starts_with("sha256:") {
                file.content_hash.clone()
            } else {
                json_digest(&json!({"path":file.path,"symbols":file.symbols}))
            };
            Ok(RepoMapTaskCandidate {
                path: file.path.clone(),
                language: file.language.clone(),
                content_digest,
                symbols,
                dependencies: Vec::new(),
                task_score,
                // Domain 禁止零估算；即使上游手工 RepoMap 填 0，也给选择器一个最小占位成本。
                estimated_tokens: file.estimated_tokens.max(1),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    RepoMapTaskSelection::new(task, token_budget, candidates)
}

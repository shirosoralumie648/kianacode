//! RepoMap 按任务筛选（task lens）的领域合同。
//!
//! 本文件定义单个文件候选、符号/依赖证据和最终选择报告，并负责在给定 token 预算下稳定
//! 排序与挑选。Query 适配器把已有只读 RepoMap 转成候选，再调用本文件的
//! `RepoMapTaskSelection::new()`；目前工作区只发现 Domain/Query fixture 直接调用，没有
//! 生产任务入口使用这套选择合同。
//!
//! ```text
//! RepoMapFile
//!    ↓ query adapter：路径/符号相关度、启发式证据、token 估算
//! RepoMapTaskCandidate
//!    ↓ 本文件：字段校验 → 相关度/路径/摘要排序 → 预算内贪心选择
//! RepoMapTaskSelection（selected + omitted_paths + selection_digest）
//! ```
//!
//! `Heuristic` 表示从文本扫描得到的启发式提示；`Declared` 表示调用方声明的关系。两者都
//! 不是编译器证明、权限事实或受信任来源证明。这里仅筛选可读上下文值，不读取源码、不
//! 调模型、不执行候选内容；摘要用于检测字段自洽，不是签名。
//!
//! `Serialize` / `Deserialize` 来自 serde：前者把 Rust DTO 编码成 JSON 等外部格式，后者
//! 把外部格式读回 Rust 值；`deny_unknown_fields` 会拒绝当前结构未声明的额外字段。
//! `serde_json::json!` 宏在摘要函数中构造待哈希的 JSON 对象，`json_digest()` 再把它变成
//! SHA-256 文本。`SchemaVersion` 是项目内的 major/minor 版本值，本合同使用精确版本匹配。

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const REPO_MAP_SYMBOL_SCHEMA: &str = "kiana.repo-map-symbol.v1";
pub const REPO_MAP_DEPENDENCY_SCHEMA: &str = "kiana.repo-map-dependency.v1";
pub const REPO_MAP_TASK_SELECTION_SCHEMA: &str = "kiana.repo-map-task-selection.v1";
/// 选择报告使用的精确 schema 版本；当前只接受 major=1、minor=0。
pub const REPO_MAP_TASK_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 一次选择最多接收的候选文件数；这是构造入口的上限。
pub const MAX_REPO_MAP_CANDIDATES: usize = 4_096;
/// 单个候选最多记录的符号数，避免外部 DTO 携带过大列表。
pub const MAX_REPO_MAP_SYMBOLS: usize = 32;
/// 单个候选最多记录的依赖边数，避免外部 DTO 携带过大列表。
pub const MAX_REPO_MAP_EDGES: usize = 64;

/// 检查非空文本字段的公共形状约束。
///
/// `max` 按 UTF-8 字节数计算；此 helper 不检查路径是否安全、标识是否真实或文本来自谁。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 只检查摘要字符串是否使用固定 SHA-256 文本格式，不重算摘要内容。
///
/// 前缀必须精确为 `sha256:`，后面必须是 64 个十六进制字符；大小写十六进制均接受。
/// `field` 用于构造稳定错误码。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 说明符号/依赖边的证据来自启发式提取，还是由调用方显式声明。
///
/// 该枚举随结构体序列化为 snake_case 文本；它是来源类别标签，不会验证来源本身。
pub enum RepoMapEvidenceLevel {
    /// 从 RepoMap 文本、路径或符号中推断；不能当作语法树/编译器结论。
    Heuristic,
    /// 调用方直接提供的声明；Domain 不会查询原始文件确认它是否正确。
    Declared,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一个代码符号名称及其证据类别和自摘要。
pub struct RepoMapSymbol {
    /// 固定的符号 wire schema 名称。
    pub schema: String,
    /// 符号的显示名称，例如 `fn deploy`；名称最多 256 字节。
    pub name: String,
    /// 此名称是启发式发现还是调用方声明；摘要会绑定该分类。
    pub evidence: RepoMapEvidenceLevel,
    /// 对 schema、name、evidence 计算的无密钥自摘要。
    pub symbol_digest: String,
}

impl RepoMapSymbol {
    /// 创建明确标记为 Heuristic 的符号，并生成其自摘要后立即验证。
    ///
    /// `impl Into<String>` 允许调用方传入 `&str` 或 `String`，函数统一取得拥有所有权的名称。
    /// Query task-lens adapter 对 RepoMap scanner 提取的每个名称调用此函数。名称为空、过长
    /// 或内部摘要无法通过校验时返回稳定错误字符串；函数不解析源码，也不证明名称对应真实
    /// Rust/其他语言符号。
    pub fn heuristic(name: impl Into<String>) -> Result<Self, String> {
        let mut symbol = Self {
            schema: REPO_MAP_SYMBOL_SCHEMA.to_owned(),
            name: name.into(),
            evidence: RepoMapEvidenceLevel::Heuristic,
            symbol_digest: String::new(),
        };
        symbol.symbol_digest = symbol.digest();
        symbol.validate()?;
        Ok(symbol)
    }

    /// 检查 schema、名称长度和摘要形状/匹配关系。
    ///
    /// 成功仅表示对象自洽；如果调用方直接构造 `Declared` 值，validator 不会要求它来自
    /// 编译器或受信任的 manifest。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != REPO_MAP_SYMBOL_SCHEMA {
            return Err("repo_map_symbol_schema_invalid".to_owned());
        }
        required(&self.name, "repo_map_symbol_name", 256)?;
        digest(&self.symbol_digest, "repo_map_symbol_digest")?;
        if self.symbol_digest != self.digest() {
            return Err("repo_map_symbol_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算 symbol digest；摘要字段自身不参与输入，避免递归。
    ///
    /// 这是无密钥的 SHA-256 内容指纹，用于发现字段变化，不是签名。
    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "name": self.name,
            "evidence": self.evidence,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 两个源码路径之间的一条依赖关系声明。
///
/// Query task-lens 当前不从 RepoMap 推导依赖边，所以它生成的候选边列表为空；该结构为
/// 后续或其他显式调用方保留带证据级别的输入形状。
pub struct RepoMapDependencyEdge {
    /// 固定的依赖边 wire schema 名称。
    pub schema: String,
    /// 关系起点路径；最多 4,096 字节。
    pub source_path: String,
    /// 关系终点路径；最多 4,096 字节。
    pub target_path: String,
    /// 关系类别文本，例如 fixture 中使用的 `heuristic_import`，最多 128 字节。
    pub relation: String,
    /// 启发式或调用方声明的来源类型，不认证关系真实性。
    pub evidence: RepoMapEvidenceLevel,
    /// 对 schema、路径、relation 和 evidence 的无密钥摘要。
    pub edge_digest: String,
}

impl RepoMapDependencyEdge {
    /// 创建一条启发式依赖边，计算摘要并验证。
    ///
    /// `impl Into<String>` 让调用方可直接传入字符串切片或已拥有的 String。路径和 relation
    /// 均由调用者提供。格式不合法时返回稳定错误；这个 helper 不解析 import、
    /// 不追踪文件，也不建立图中的传递闭包。
    pub fn heuristic(
        source_path: impl Into<String>,
        target_path: impl Into<String>,
        relation: impl Into<String>,
    ) -> Result<Self, String> {
        let mut edge = Self {
            schema: REPO_MAP_DEPENDENCY_SCHEMA.to_owned(),
            source_path: source_path.into(),
            target_path: target_path.into(),
            relation: relation.into(),
            evidence: RepoMapEvidenceLevel::Heuristic,
            edge_digest: String::new(),
        };
        edge.edge_digest = edge.digest();
        edge.validate()?;
        Ok(edge)
    }

    /// 检查 schema、两端路径、关系文本和自摘要。
    ///
    /// 验证的是单条 DTO 的局部形状；路径是否存在以及依赖关系是否真实不在这里判断。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != REPO_MAP_DEPENDENCY_SCHEMA {
            return Err("repo_map_dependency_schema_invalid".to_owned());
        }
        required(&self.source_path, "repo_map_dependency_source", 4_096)?;
        required(&self.target_path, "repo_map_dependency_target", 4_096)?;
        required(&self.relation, "repo_map_dependency_relation", 128)?;
        digest(&self.edge_digest, "repo_map_dependency_digest")?;
        if self.edge_digest != self.digest() {
            return Err("repo_map_dependency_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算依赖边摘要；摘要字段自身不参与输入。
    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "source_path": self.source_path,
            "target_path": self.target_path,
            "relation": self.relation,
            "evidence": self.evidence,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一个可参与任务相关度排序和 token 预算选择的文件候选。
pub struct RepoMapTaskCandidate {
    /// 文件在 RepoMap 中使用的路径文本；Domain 只检查非空、长度和 NUL，不规范化路径。
    pub path: String,
    /// 从文件扩展名推断的语言标签；未知语言允许为 None，Domain 不校验该字符串的含义。
    pub language: Option<String>,
    /// 候选内容摘要；此结构只检查 SHA-256 文本格式，不读取原文件核验。
    pub content_digest: String,
    /// 关联的符号及其证据等级；最多 32 项且符号名不能重复。
    pub symbols: Vec<RepoMapSymbol>,
    /// 显式依赖边；最多 64 项且 edge digest 不能重复。
    pub dependencies: Vec<RepoMapDependencyEdge>,
    /// 任务与路径/符号匹配的相对分数；分数越高越先参与选择。
    pub task_score: u64,
    /// 调用方估算的上下文 token 数；必须大于 0，不是精确模型 tokenizer 结果。
    pub estimated_tokens: u64,
}

impl RepoMapTaskCandidate {
    /// 校验候选自身字段、符号/依赖列表上限、重复项和非零 token 估算。
    ///
    /// 【失败】字段无效时返回 `repo_map_candidate_*` 错误；不会尝试修复或删除候选项。
    /// 【边界】这里不校验 path 指向真实文件，也不重算 content_digest/task_score。
    pub fn validate(&self) -> Result<(), String> {
        required(&self.path, "repo_map_candidate_path", 4_096)?;
        digest(&self.content_digest, "repo_map_candidate_content_digest")?;
        if self.symbols.len() > MAX_REPO_MAP_SYMBOLS || self.dependencies.len() > MAX_REPO_MAP_EDGES
        {
            return Err("repo_map_candidate_limit".to_owned());
        }
        // BTreeSet 记住已见名称；insert 返回 false 表示当前候选中出现重复值。
        let mut symbols = BTreeSet::new();
        for symbol in &self.symbols {
            symbol.validate()?;
            if !symbols.insert(symbol.name.clone()) {
                return Err("repo_map_candidate_symbol_duplicate".to_owned());
            }
        }
        let mut edges = BTreeSet::new();
        for edge in &self.dependencies {
            edge.validate()?;
            if !edges.insert(edge.edge_digest.clone()) {
                return Err("repo_map_candidate_dependency_duplicate".to_owned());
            }
        }
        if self.estimated_tokens == 0 {
            return Err("repo_map_candidate_token_estimate_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一次按任务和预算选出的 RepoMap 文件集合及其摘要。
///
/// `selected` 是已纳入上下文的候选，`omitted_paths` 是有候选但预算放不下的路径；
/// `estimated_tokens` 只累计所选候选的估算。报告不保存原始任务文字，只保存 task digest。
pub struct RepoMapTaskSelection {
    /// 固定的 selection wire schema 名称。
    pub schema: String,
    /// 选择结果的精确兼容版本；当前 `validate()` 不做跨 minor 的兼容协商。
    pub version: SchemaVersion,
    /// 原始任务文本的摘要；任务正文不会存进报告。
    pub task_digest: String,
    /// 调用方给定的最大估算 token 数；必须大于 0。
    pub token_budget: u64,
    /// 已选择候选的估算总 token 数，不能大于 token_budget。
    pub estimated_tokens: u64,
    /// 依确定性顺序纳入预算的候选文件。
    pub selected: Vec<RepoMapTaskCandidate>,
    /// 因预算放不下而省略的路径；保留候选遍历顺序以便报告遗漏。
    pub omitted_paths: Vec<String>,
    /// 对 schema/version/task/budget/selected/omitted 内容计算的无密钥自摘要。
    pub selection_digest: String,
}

impl RepoMapTaskSelection {
    /// 校验、排序并在 token 预算内贪心选择 RepoMap 候选。
    ///
    /// 【调用者】Query 的 `select_repo_map_for_task()` 把 scanner 输出转换为候选后调用；目前
    /// workspace 只发现 Query/Domain fixture 直接使用该合同，没有生产任务入口。
    ///
    /// `impl Into<String>` 允许调用方传入 `&str` 或 `String`；函数把原文仅用于计算 task digest。
    ///
    /// 【输入】原始任务文本、正数 `token_budget`、最多 4,096 个候选。每个候选必须先通过
    /// 自身校验，包含有效摘要、唯一符号/边、且 token 估算非零。
    ///
    /// 【核心流程】按 task_score 从高到低，再按 path、content_digest 升序稳定排序；逐个候选
    /// 检查加上它是否仍不超过预算。放得下就选入，放不下就记录路径并继续检查后续候选，
    /// 因此一个较大的高分文件被省略后，较小的低分文件仍可能被选中。最后只保存任务摘要、
    /// selected/omitted 与计数，并计算 selection digest，再调用 `validate()` 复核。
    ///
    /// 【失败/副作用】预算为 0、候选数过多或候选字段无效时返回错误；不做磁盘 I/O、不重试、
    /// 不改变输入 map。`saturating_add()` 防止 u64 回绕，但在 token_budget 本身为 u64::MAX
    /// 且真实累加溢出时会饱和到 MAX；调用方应把 token 数看作受限估算，而非任意精度计量。
    ///
    /// 【为什么这样设计】稳定 tie-break 避免相同输入只因容器顺序不同而选择不同文件；
    /// 贪心筛选保证报告不超过声明的估算预算，并明确记录被省略内容，供上游理解上下文缺口。
    pub fn new(
        task: impl Into<String>,
        token_budget: u64,
        mut candidates: Vec<RepoMapTaskCandidate>,
    ) -> Result<Self, String> {
        if token_budget == 0 || candidates.len() > MAX_REPO_MAP_CANDIDATES {
            return Err("repo_map_selection_header_invalid".to_owned());
        }
        for candidate in &candidates {
            candidate.validate()?;
        }
        // 相关度优先；路径和内容摘要作为确定性 tie-break，消除候选输入顺序的影响。
        candidates.sort_by(|left, right| {
            right
                .task_score
                .cmp(&left.task_score)
                .then(left.path.cmp(&right.path))
                .then(left.content_digest.cmp(&right.content_digest))
        });
        let mut selected = Vec::new();
        let mut omitted_paths = Vec::new();
        let mut estimated_tokens: u64 = 0;
        for candidate in candidates {
            // 超预算候选记录为省略项，但继续遍历；后面较小的候选仍可能放入剩余空间。
            if estimated_tokens.saturating_add(candidate.estimated_tokens) <= token_budget {
                estimated_tokens = estimated_tokens.saturating_add(candidate.estimated_tokens);
                selected.push(candidate);
            } else {
                omitted_paths.push(candidate.path);
            }
        }
        let mut selection = Self {
            schema: REPO_MAP_TASK_SELECTION_SCHEMA.to_owned(),
            version: REPO_MAP_TASK_VERSION,
            task_digest: json_digest(&json!({"task": task.into()})),
            token_budget,
            estimated_tokens,
            selected,
            omitted_paths,
            selection_digest: String::new(),
        };
        selection.selection_digest = selection.digest();
        selection.validate()?;
        Ok(selection)
    }

    /// 校验一个已构造或反序列化的选择报告是否自洽。
    ///
    /// 检查 schema/version/budget、task digest、每个 selected 候选、selected 与 omitted 路径
    /// 去重、token 总数和 selection digest。若某一步失败，返回对应 `repo_map_selection_*`
    /// 错误；不读取 task 原文、RepoMap 或源文件。
    ///
    /// ⚠ `new()` 会限制输入候选最多 4,096 个并进行稳定排序，但此 `validate()` 目前不复核
    /// 整体 selected+omitted 数量上限，也不重新要求列表保持 `new()` 的排序顺序。对于从外部
    /// 反序列化而来的报告，不能把此方法当作完整 canonical-form 或资源用量校验。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != REPO_MAP_TASK_SELECTION_SCHEMA
            || self.version != REPO_MAP_TASK_VERSION
            || self.token_budget == 0
            || self.estimated_tokens > self.token_budget
        {
            return Err("repo_map_selection_header_invalid".to_owned());
        }
        digest(&self.task_digest, "repo_map_selection_task_digest")?;
        // 同一个集合覆盖 selected 与 omitted，insert=false 表示报告重复描述同一路径。
        let mut paths = BTreeSet::new();
        let mut total = 0_u64;
        for candidate in &self.selected {
            candidate.validate()?;
            if !paths.insert(candidate.path.clone()) {
                return Err("repo_map_selection_duplicate_path".to_owned());
            }
            total = total.saturating_add(candidate.estimated_tokens);
        }
        // selected 与 omitted 共用同一个集合，避免同一路径同时被报告为已选和已省略。
        for path in &self.omitted_paths {
            required(path, "repo_map_selection_omitted_path", 4_096)?;
            if !paths.insert(path.clone()) {
                return Err("repo_map_selection_duplicate_path".to_owned());
            }
        }
        if total != self.estimated_tokens {
            return Err("repo_map_selection_token_mismatch".to_owned());
        }
        digest(&self.selection_digest, "repo_map_selection_digest")?;
        if self.selection_digest != self.digest() {
            return Err("repo_map_selection_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算选择报告摘要；原始任务和摘要字段自身都不在摘要输入中。
    ///
    /// task_digest 间接绑定任务文本，selected/omitted 的顺序也参与 JSON 摘要；值没有密钥，
    /// 不能认证选择者身份或候选来源。
    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "task_digest": self.task_digest,
            "token_budget": self.token_budget,
            "estimated_tokens": self.estimated_tokens,
            "selected": self.selected,
            "omitted_paths": self.omitted_paths,
        }))
    }
}

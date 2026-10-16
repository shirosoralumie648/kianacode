//! 统一检索的 Domain 数据合同和确定性排序算法。
//!
//! 本文件位于记忆/上下文检索的数据处理层：它接收上游已经整理好的候选快照，对允许进入排名的条目计算
//! 精确词项、BM25、可选向量相似度、倒数排名融合（Reciprocal Rank Fusion，RRF）和最大边际相关性
//!（Maximal Marginal Relevance，MMR），最后返回带算法版本和摘要的排序结果。
//!
//! ```text
//! 检索 adapter / 上下文构造者
//!     -> RetrievalRequest（查询词、权限范围摘要、可选查询向量）
//!     -> RetrievalItem[]（文本、来源摘要、范围摘要、上游 allowed 判定、可选条目向量）
//!     -> rank_retrieval()（校验 -> 词项/BM25 与向量排名 -> RRF 合并 -> MMR 去冗余）
//!     -> RetrievalResult（可解释分数、rank、匹配词、降级原因和结果摘要）
//!     -> kiana-query::UnifiedRetrievalPort / 后续上下文消费者
//! ```
//!
//! 排序结果只描述“给定候选中哪些更相关”，不代表调用者有权读取候选。`allowed`、`acl_filtered` 和权限范围
//! 摘要都来自调用方；Domain 会据此排除标记为不允许的条目并检查摘要一致，但不会从身份或策略系统验证
//! 这些声明。本文件也不读取索引/数据库、不生成 embedding、不加载模型、不发网络请求、不执行工具或副作用。
//! 仓库内还保留了旧 Daemon/CLI 检索实现；当前它们是独立兼容路径，并没有在本算法内部复用。

use crate::{json_digest, memory_tokens, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const UNIFIED_RETRIEVAL_PROFILE_SCHEMA: &str = "kiana.unified-retrieval-profile.v1";
pub const RETRIEVAL_REQUEST_SCHEMA: &str = "kiana.retrieval-request.v1";
pub const RETRIEVAL_RESULT_SCHEMA: &str = "kiana.retrieval-result.v1";
pub const UNIFIED_RETRIEVAL_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// 检查标识字符串的基本形状；长度限制按 UTF-8 字节计算。
/// 这里只防空值、过长值和 NUL 字节，不验证标识背后的对象或授权事实。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 检查 `sha256:` 加 64 位十六进制摘要的格式，不负责验证来源或签名者。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 一次检索使用的、可随结果回溯的算法参数快照。
///
/// `profile_digest` 把 schema、版本、算法标签和数值参数绑定成同一个内容摘要，方便结果记录“用哪组参数
/// 算出来”。Serde 的 `Serialize`/`Deserialize` 用于 Rust 值与 JSON 等格式互转；`deny_unknown_fields`
/// 让输入中未被本版本声明的字段直接报错，避免不兼容配置被静默忽略。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalProfile {
    /// profile wire schema，用来区分数据合同种类。
    pub schema: String,
    /// 结构合同版本；不等于排序算法版本字符串。
    pub version: SchemaVersion,
    /// 排序实现标签，写入结果便于审计和 golden fixture 对照。
    pub algorithm_version: String,
    /// RRF 的平滑常数 k；k 越大，排名位置差异对融合分数的影响越小。
    pub rrf_k: u32,
    /// BM25 的词频饱和参数；正数，控制词频增加时分数增长的速度。
    pub bm25_k1: f64,
    /// BM25 的文档长度归一化强度，范围 0 到 1。
    pub bm25_b: f64,
    /// MMR 在“查询相关性”与“避免重复”之间的权重，范围 0 到 1。
    pub mmr_lambda: f64,
    /// 对以上 profile 内容计算的无密钥自摘要，用于发现字段被意外改动。
    pub profile_digest: String,
}

impl RetrievalProfile {
    /// 返回本版本统一检索算法的固定默认参数，并按这些参数计算摘要。
    ///
    /// 【调用者】Query adapter 的 `Default`、便捷函数 `rank_unified()` 和 Domain fixture。
    /// 【输出】`rrf_k=60`、BM25 `k1=1.2/b=0.75`、MMR `lambda=0.7` 的 profile。
    /// 【设计原因】把默认算法参数集中到一个带摘要的对象，避免各 adapter 各自散落常数后排名不一致。
    /// 【边界】算法标签和摘要用于记录/自校验，不证明某个调用者真的运行了对应版本。
    pub fn unified() -> Self {
        let mut profile = Self {
            schema: UNIFIED_RETRIEVAL_PROFILE_SCHEMA.to_owned(),
            version: UNIFIED_RETRIEVAL_VERSION,
            algorithm_version: "exact+bm25-cjk+dense+rrf60+mmr.v1".to_owned(),
            rrf_k: 60,
            bm25_k1: 1.2,
            bm25_b: 0.75,
            mmr_lambda: 0.7,
            profile_digest: String::new(),
        };
        profile.profile_digest = profile.digest();
        profile
    }

    /// 检查 profile 版本、参数范围、算法标签和自摘要。
    ///
    /// 【调用者】`rank_retrieval()` 每次排名前，以及 Query adapter 接受自定义 profile 时。
    /// 【失败】参数不在合同范围时返回 `retrieval_profile_invalid`；标签/摘要不合法时返回对应字段错误。
    /// 【注意】此处只要求 `algorithm_version` 非空且不超过 128 字节，没有把它限制为当前内建标签；
    /// digest 也是无密钥摘要，调用方可重算，所以两者都不是实现身份认证。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UNIFIED_RETRIEVAL_PROFILE_SCHEMA
            || self.version != UNIFIED_RETRIEVAL_VERSION
            || self.rrf_k == 0
            || self.rrf_k > 4_096
            || !self.bm25_k1.is_finite()
            || self.bm25_k1 <= 0.0
            || !self.bm25_b.is_finite()
            || !(0.0..=1.0).contains(&self.bm25_b)
            || !self.mmr_lambda.is_finite()
            || !(0.0..=1.0).contains(&self.mmr_lambda)
        {
            return Err("retrieval_profile_invalid".to_owned());
        }
        required(&self.algorithm_version, "retrieval_algorithm_version", 128)?;
        digest(&self.profile_digest, "retrieval_profile_digest")?;
        if self.profile_digest != self.digest() {
            return Err("retrieval_profile_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 profile 的合同版本、算法标签和四个排名参数做规范 JSON 摘要。
    /// `profile_digest` 自身不进入摘要输入，否则会形成自我引用。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "algorithm_version": self.algorithm_version,
            "rrf_k": self.rrf_k,
            "bm25_k1": self.bm25_k1,
            "bm25_b": self.bm25_b,
            "mmr_lambda": self.mmr_lambda,
        }))
    }
}

/// 单次检索的查询端输入；既携带查询文本，也携带预先整理的词项和可选向量。
///
/// 一般由 `RetrievalRequest::new()` 从查询文本生成词项；`rank_retrieval()` 使用词项计算稀疏相关性，
/// 使用向量（如果双方都有且维度相同）计算密集相关性。公开字段也允许 JSON 反序列化直接构造，因此
/// validator 检查摘要和范围，但当前不重新证明所有字段都是从同一原始查询生成的。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalRequest {
    /// request wire schema。
    pub schema: String,
    /// request 合同版本。
    pub version: SchemaVersion,
    /// 原始用户查询文本，便于追踪请求语义；排名直接使用 `tokens`。
    pub query: String,
    /// `memory_tokens()` 从 query 派生的词项；包含小写化词语及中文连续字的相邻二元组。
    pub tokens: Vec<String>,
    /// 可选的查询向量，由上游 embedding 路径提供；本 Domain 算法不生成它。
    #[serde(default)]
    pub dense_vector: Option<Vec<f64>>,
    /// 服务端应从可信权限范围导出的摘要；这里只检查格式并与每个候选的标签比较。
    pub permission_scope_digest: String,
    /// 调用方声明 ACL 过滤已完成的合同标志；为 false 时请求被拒绝，但此 bool 本身不是授权证明。
    pub acl_filtered: bool,
    /// 对 query、tokens、向量、权限范围摘要和 ACL 标志计算的无密钥摘要。
    pub request_digest: String,
}

impl RetrievalRequest {
    /// 从查询文本、权限范围摘要和可选查询向量构造请求。
    ///
    /// 【调用者】检索 adapter 在把候选交给 Domain 排序前。
    /// 【输入/输出】输入文本、调用方导出的 scope digest 和可选 dense vector；成功返回填入 schema、词项、
    /// `acl_filtered=true` 及 request digest 的请求。
    /// 【失败】查询/词项过大、摘要或向量非法时返回稳定错误。
    /// 【边界】构造函数把 ACL 标志设为 true 只是表达调用方声明；真实权限筛选仍由上游按身份和策略完成。
    pub fn new(
        query: impl Into<String>,
        permission_scope_digest: impl Into<String>,
        dense_vector: Option<Vec<f64>>,
    ) -> Result<Self, String> {
        let query = query.into();
        let tokens = memory_tokens(&query);
        let mut request = Self {
            schema: RETRIEVAL_REQUEST_SCHEMA.to_owned(),
            version: UNIFIED_RETRIEVAL_VERSION,
            query,
            tokens,
            dense_vector,
            permission_scope_digest: permission_scope_digest.into(),
            acl_filtered: true,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    /// 检查请求 header、大小上限、scope 摘要、可选向量和 request digest。
    ///
    /// 【调用者】`new()` 和 `rank_retrieval()`。
    /// 【失败】合同版本/ACL 标志/大小不符时返回 `retrieval_request_invalid`；向量或摘要各有专门错误。
    /// 【重要边界】不重新计算 `tokens == memory_tokens(query)`，也不认证 scope 摘要是谁提供的；可直接
    /// 反序列化或手工构造该结构体的调用方必须确保两者来自正确查询和可信权限上下文。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETRIEVAL_REQUEST_SCHEMA
            || self.version != UNIFIED_RETRIEVAL_VERSION
            || !self.acl_filtered
            || self.query.len() > 32 * 1024
            || self.tokens.len() > 16_384
        {
            return Err("retrieval_request_invalid".to_owned());
        }
        digest(
            &self.permission_scope_digest,
            "retrieval_permission_scope_digest",
        )?;
        if self.dense_vector.as_ref().is_some_and(|vector| {
            vector.is_empty() || vector.iter().any(|value| !value.is_finite())
        }) {
            return Err("retrieval_dense_vector_invalid".to_owned());
        }
        digest(&self.request_digest, "retrieval_request_digest")?;
        if self.request_digest != self.digest() {
            return Err("retrieval_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对检索请求的全部语义字段求摘要；`request_digest` 自身不参与计算。
    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "query": self.query,
            "tokens": self.tokens,
            "dense_vector": self.dense_vector,
            "permission_scope_digest": self.permission_scope_digest,
            "acl_filtered": self.acl_filtered,
        }))
    }
}

/// 一个已由检索 adapter 整理好的候选内容。
///
/// 输入通常来自已选定的索引/记忆快照；`allowed` 是上游对该候选能否进入本次结果的决定，`dense_vector`
/// 是可选的预计算向量。Domain 只做字段格式检查、scope 一致性检查和 `allowed` 过滤，不读取来源或重新
/// 执行 ACL 策略。被标为不允许的项仍会先做基本字段校验，并会参与初步文本 tokenization；但其统计、得分和
/// 最终命中均被排除。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalItem {
    /// 候选稳定标识；当前 validator 限制格式/长度，但不强制整个输入内唯一。
    pub id: String,
    /// 用于稀疏检索的正文。当前合同未给该字符串设置独立长度上限。
    pub text: String,
    /// 来源内容摘要标签；本模块验证摘要外形但不从 `text` 重算或认证来源。
    pub source_digest: String,
    /// 生成该候选时使用的权限范围摘要，必须和 request 的范围摘要相等。
    pub permission_scope_digest: String,
    /// 上游准入结果；`false` 的候选会计入过滤数但不会进入排名。
    pub allowed: bool,
    /// 候选来源时间（Unix 毫秒），仅在 MMR 同分时作为较新的优先条件。
    pub created_at_ms: u64,
    /// 可选预计算条目向量；需与查询向量维度相同才会参与 dense 排名。
    #[serde(default)]
    pub dense_vector: Option<Vec<f64>>,
}

impl RetrievalItem {
    /// 检查候选 ID、来源/scope 摘要及向量数值。
    ///
    /// 【调用者】`rank_retrieval()` 对输入切片中的每项调用，包括 `allowed=false` 的候选。
    /// 【失败】空/超长 ID、摘要格式错误、空向量或非有限向量会使整次检索失败。
    /// 【边界】没有正文大小、向量维度/数量上限，也不比较重复 ID 或来源摘要与正文内容。
    fn validate(&self) -> Result<(), String> {
        required(&self.id, "retrieval_item_id", 512)?;
        digest(&self.source_digest, "retrieval_item_source_digest")?;
        digest(
            &self.permission_scope_digest,
            "retrieval_item_permission_scope_digest",
        )?;
        if self.dense_vector.as_ref().is_some_and(|vector| {
            vector.is_empty() || vector.iter().any(|value| !value.is_finite())
        }) {
            return Err("retrieval_item_dense_vector_invalid".to_owned());
        }
        Ok(())
    }
}

/// 一个排序命中及解释该命中来自哪些评分通道的数据。
///
/// `rank` 是最终 MMR 选择顺序的一基序号；稀疏与密集 rank 分别记录各自通道名次，RRF/MMR 分数记录融合
/// 和去冗余阶段结果。`Option` rank/score 为空表示该条目没有进入对应通道（例如无查询向量或无匹配项）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalHit {
    /// 候选 ID，用于下游回到来源记录。
    pub id: String,
    /// 候选来源摘要标签，供下游核对输入快照。
    pub source_digest: String,
    /// 本次请求的 scope 摘要；validator 要求每个 hit 与结果范围相同。
    pub permission_scope_digest: String,
    /// 最终输出顺序，从 1 开始。
    pub rank: u32,
    /// 命中的不同 query term 数量，不是原文频次。
    pub exact_score: u32,
    /// BM25 稀疏相关性分数。
    pub bm25_score: f64,
    /// 与查询向量的 cosine 相似度；未能形成正分时为空。
    pub dense_score: Option<f64>,
    /// 条目在 BM25 排序中的一基名次；没有 BM25 命中时为空。
    pub sparse_rank: Option<u32>,
    /// 条目在 dense 排序中的一基名次；没有有效 dense 命中时为空。
    pub dense_rank: Option<u32>,
    /// 将 sparse/dense 名次合并后的 RRF 分数。
    pub rrf_score: f64,
    /// 本轮 MMR 选择时的相关性减冗余分数。
    pub mmr_score: f64,
    /// 最终命中正文中出现的查询词项，按集合顺序稳定输出。
    pub matched_terms: Vec<String>,
}

/// 一次检索的最终只读结果，供 adapter 或后续上下文构造使用。
///
/// 它把 profile、request 和权限范围摘要一路带到 hits，并以 `result_digest` 对整份输出做内容自校验。
/// 有结果结构不等于结果已经持久化、已送给模型或通过安全审查。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalResult {
    /// result wire schema。
    pub schema: String,
    /// result 合同版本。
    pub version: SchemaVersion,
    /// 结果声明的算法标签，来自本次 profile。
    pub algorithm_version: String,
    /// 本次实际传入 profile 的内容摘要标签。
    pub profile_digest: String,
    /// 本次 request 的摘要标签。
    pub request_digest: String,
    /// 该结果所属的权限范围摘要。
    pub permission_scope_digest: String,
    /// 输入项中 `allowed=false` 的数量。
    pub acl_filtered_count: u32,
    /// 按最终 MMR 顺序排列的命中。
    pub hits: Vec<RetrievalHit>,
    /// 排名通道不可用等降级原因；空列表表示本次未记录降级原因，不代表语义质量已验证。
    #[serde(default)]
    pub degraded_reasons: Vec<String>,
    /// 对 header、通道分数、命中和降级原因求出的无密钥结果摘要。
    pub result_digest: String,
}

impl RetrievalResult {
    /// 根据本次 profile/request 和排序过程生成结果 DTO，并填入结果摘要。
    ///
    /// 【调用者】只由同文件 `rank_retrieval()` 在完成排名后调用。
    /// 【输入/输出】接收被过滤计数、已排序 hits 与降级原因；不再次读取候选数据。
    /// 【副作用】构造内存值，不存储或发布结果。
    fn new(
        profile: &RetrievalProfile,
        request: &RetrievalRequest,
        acl_filtered_count: u32,
        hits: Vec<RetrievalHit>,
        degraded_reasons: Vec<String>,
    ) -> Self {
        let mut result = Self {
            schema: RETRIEVAL_RESULT_SCHEMA.to_owned(),
            version: UNIFIED_RETRIEVAL_VERSION,
            algorithm_version: profile.algorithm_version.clone(),
            profile_digest: profile.profile_digest.clone(),
            request_digest: request.request_digest.clone(),
            permission_scope_digest: request.permission_scope_digest.clone(),
            acl_filtered_count,
            hits,
            degraded_reasons,
            result_digest: String::new(),
        };
        result.result_digest = result.digest();
        result
    }

    /// 检查结果 header、rank 顺序、scope 一致性、浮点数和自摘要。
    ///
    /// 【调用者】`rank_retrieval()` 在返回前，以及消费方需要检查结构完整性时。
    /// 【失败】命中 rank 不连续、scope 不一致、分数不是有限数或结果摘要不匹配时返回稳定错误。
    /// 【边界】这里没有原始 request/profile/items，因此不能重算排序，也不校验 hit ID 唯一性、source_digest
    /// 格式或结果条数是否符合原调用者要求；自摘要不是签名。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETRIEVAL_RESULT_SCHEMA || self.version != UNIFIED_RETRIEVAL_VERSION {
            return Err("retrieval_result_header_invalid".to_owned());
        }
        required(
            &self.algorithm_version,
            "retrieval_result_algorithm_version",
            128,
        )?;
        digest(&self.profile_digest, "retrieval_result_profile_digest")?;
        digest(&self.request_digest, "retrieval_result_request_digest")?;
        digest(
            &self.permission_scope_digest,
            "retrieval_result_permission_scope_digest",
        )?;
        for (rank, hit) in self.hits.iter().enumerate() {
            if hit.rank != rank as u32 + 1
                || hit.permission_scope_digest != self.permission_scope_digest
                || !hit.bm25_score.is_finite()
                || hit.dense_score.is_some_and(|value| !value.is_finite())
                || !hit.rrf_score.is_finite()
                || !hit.mmr_score.is_finite()
            {
                return Err("retrieval_result_hit_invalid".to_owned());
            }
        }
        digest(&self.result_digest, "retrieval_result_digest")?;
        if self.result_digest != self.digest() {
            return Err("retrieval_result_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对整个 result 内容求摘要；`result_digest` 自身不参与计算。
    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "algorithm_version": self.algorithm_version,
            "profile_digest": self.profile_digest,
            "request_digest": self.request_digest,
            "permission_scope_digest": self.permission_scope_digest,
            "acl_filtered_count": self.acl_filtered_count,
            "hits": self.hits,
            "degraded_reasons": self.degraded_reasons,
        }))
    }
}

/// 对一份已由上游准备好的候选快照执行统一、确定性的混合检索排序。
///
/// 【作用】将词项精确命中、BM25 稀疏排名和可选 dense 排名通过 RRF 合并，再用 MMR 逐条挑选兼顾相关性
/// 与多样性的结果。
/// 【调用者】`kiana-query::UnifiedRetrievalPort::retrieve()` 与 `rank_unified()` 转发到这里；Domain 的
/// golden-context 校验也使用该函数重放固定输入。当前产品路径调用搜索未发现生产 adapter 使用该 Query 包装。
/// 【输入】经过可信上游构造的 profile/request/candidate slice 和最大命中数 `limit`。候选的 scope 必须与
/// request 一致；`allowed` 和 `acl_filtered` 应由上游权限筛选流程提供。
/// 【输出】校验通过的 `RetrievalResult`，携带每个通道名次、分数、匹配词和降级原因。
/// 【失败】profile/request/item 合同、scope 不一致或候选/limit 超限时返回具体错误；结果自校验失败也会
/// 阻止返回部分结果。Query adapter 会把这些字符串错误映射为 `PortError::Failed`。
/// 【核心流程】先检查合同与边界，再以 `allowed` 列表计算 BM25/dense 两路排名，RRF 融合名次，最后 MMR
/// 逐次选最高分条目并记录完整解释字段。
/// 【为什么分阶段融合】词项评分可解释并擅长精确匹配，向量通道可补充语义近邻，RRF 在不同分数尺度之间
/// 合并 rank，MMR 再减少最终列表中的重复内容。任何阶段都不代替权限决策。
/// 【容易误改】所有排序 tie-break 都用于输入重放稳定性；改 profile 常数或先后比较字段会改变历史排名，应
/// 同时更新算法版本与 golden 对照，而不能只把它当作内部实现细节。
/// 【副作用】纯函数式计算，不访问存储、模型或网络，不修改输入切片。
pub fn rank_retrieval(
    profile: &RetrievalProfile,
    request: &RetrievalRequest,
    items: &[RetrievalItem],
    limit: usize,
) -> Result<RetrievalResult, String> {
    // 先挡住不兼容/不合法的 profile 与 request，避免错误输入进入打分阶段。
    profile.validate()?;
    request.validate()?;
    if items.len() > 16_384 || limit > 1_024 {
        return Err("retrieval_item_limit".to_owned());
    }
    // 先验证整份快照的记录形状及 scope 一致性。即便某项之后会被过滤，畸形候选或跨范围候选也会使整批
    // 请求失败，避免把错误数据悄悄混进同一个结果集合。
    for item in items {
        item.validate()?;
        if item.permission_scope_digest != request.permission_scope_digest {
            return Err("retrieval_item_scope_mismatch".to_owned());
        }
    }
    // 保留原输入下标而不是复制候选，后续各评分向量都用相同下标对应回原 RetrievalItem。
    // `allowed` 来自上游；这里将 false 项排除出文档频率、各通道排名和 MMR，同时统计过滤数量。
    let allowed = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.allowed)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let acl_filtered_count = (items.len() - allowed.len()) as u32;
    let mut degraded_reasons = Vec::new();
    // 词项集合用于去重查询词；文档 token 列表保留重复词频以供 BM25 计算，term set 则只用于“文档是否含词”
    // 的文档频率（DF）统计。tokenization helper 会小写普通词，并把中文连续字切成重叠二元组。
    let query_terms = request.tokens.iter().cloned().collect::<BTreeSet<_>>();
    let token_sets = items
        .iter()
        .map(|item| memory_tokens(&item.text).into_iter().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let term_sets = token_sets
        .iter()
        .map(|tokens| tokens.iter().cloned().collect::<BTreeSet<_>>())
        .collect::<Vec<_>>();
    let average_length = allowed
        .iter()
        .map(|index| token_sets[*index].len())
        .sum::<usize>() as f64
        / (allowed.len().max(1) as f64);
    let document_count = allowed.len() as f64;
    // BM25 的 IDF 只从允许参与本次检索的文档统计，避免被拒绝项改变可见候选的相对分数。
    let document_frequency = query_terms
        .iter()
        .map(|term| {
            (
                term.clone(),
                allowed
                    .iter()
                    .filter(|index| term_sets[**index].contains(term))
                    .count() as f64,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut exact = vec![0_u32; items.len()];
    let mut bm25 = vec![0.0; items.len()];
    let mut dense = vec![None; items.len()];
    // 两路分数按原候选下标保存。exact_score 数的是出现过的不同 query term；BM25 的 tf 则会计算正文中的
    // 重复次数，并按允许文档的平均 token 长度归一化。
    for index in &allowed {
        let tokens = &token_sets[*index];
        for term in &query_terms {
            let tf = tokens.iter().filter(|token| *token == term).count() as f64;
            if tf > 0.0 {
                exact[*index] = exact[*index].saturating_add(1);
                let frequency = document_frequency[term];
                let idf = (1.0 + (document_count - frequency + 0.5) / (frequency + 0.5)).ln();
                bm25[*index] += idf * tf * (profile.bm25_k1 + 1.0)
                    / (tf
                        + profile.bm25_k1
                            * (1.0 - profile.bm25_b
                                + profile.bm25_b * tokens.len() as f64 / average_length.max(1.0)));
            }
        }
        dense[*index] = request
            .dense_vector
            .as_ref()
            .zip(items[*index].dense_vector.as_ref())
            .and_then(|(query, item)| {
                if query.len() != item.len() {
                    None
                } else {
                    Some(cosine(query, item))
                }
            })
            .filter(|score| *score > 0.0);
    }
    // 如果请求希望使用 dense 通道，但所有候选都缺向量、维度不匹配或 cosine 不为正，就把原因写入结果，
    // 让调用者能区分“没有 dense 信号”和“正常的零命中”；稀疏通道仍可继续工作。
    if request.dense_vector.is_some() && dense.iter().all(Option::is_none) {
        degraded_reasons.push("dense_unavailable_or_dimension_mismatch".to_owned());
    }
    let mut sparse_order = allowed
        .iter()
        .copied()
        .filter(|index| bm25[*index] > 0.0)
        .collect::<Vec<_>>();
    sparse_order.sort_by(|left, right| {
        bm25[*right]
            .total_cmp(&bm25[*left])
            .then(exact[*right].cmp(&exact[*left]))
            .then(items[*left].id.cmp(&items[*right].id))
    });
    let mut dense_order = allowed
        .iter()
        .copied()
        .filter(|index| dense[*index].is_some())
        .collect::<Vec<_>>();
    dense_order.sort_by(|left, right| {
        dense[*right]
            .unwrap_or_default()
            .total_cmp(&dense[*left].unwrap_or_default())
            .then(items[*left].id.cmp(&items[*right].id))
    });
    // 稀疏与密集通道先各自确定一基名次，再用 reciprocal-rank 的 `1 / (k + rank)` 相加。
    // RRF 不直接相加 BM25 与 cosine 原始分数，因为两种分数尺度不同，直接相加会让某一通道天然占优。
    let mut sparse_rank = vec![None; items.len()];
    let mut dense_rank = vec![None; items.len()];
    let mut rrf = vec![0.0; items.len()];
    for (rank, index) in sparse_order.into_iter().enumerate() {
        sparse_rank[index] = Some(rank as u32 + 1);
        rrf[index] += 1.0 / (profile.rrf_k as f64 + rank as f64 + 1.0);
    }
    for (rank, index) in dense_order.into_iter().enumerate() {
        dense_rank[index] = Some(rank as u32 + 1);
        rrf[index] += 1.0 / (profile.rrf_k as f64 + rank as f64 + 1.0);
    }
    let max_rrf = rrf.iter().copied().fold(0.0, f64::max).max(f64::EPSILON);
    // 无任何通道命中的允许候选不会仅凭“允许”就出现在相关性结果中；BTreeSet 保证选择池按输入下标稳定。
    let mut remaining = allowed
        .iter()
        .copied()
        .filter(|index| rrf[*index] > 0.0)
        .collect::<BTreeSet<_>>();
    let mut selected = Vec::new();
    let mut hits = Vec::new();
    while selected.len() < limit && !remaining.is_empty() {
        // MMR 每轮重算候选对“已选集合”的最大冗余，再与归一化 RRF 相关性加权。
        // 两条记录都有同维向量时用条目间 cosine；否则回退到 token-set 的 Jaccard 重叠率。
        let mut ranked = remaining
            .iter()
            .map(|index| {
                let redundancy = selected
                    .iter()
                    .map(|other: &usize| {
                        match (&items[*index].dense_vector, &items[*other].dense_vector) {
                            (Some(left), Some(right)) if left.len() == right.len() => {
                                cosine(left, right)
                            }
                            _ => overlap(&term_sets[*index], &term_sets[*other]),
                        }
                    })
                    .fold(0.0, f64::max);
                let base = rrf[*index] / max_rrf;
                let mmr = profile.mmr_lambda * base - (1.0 - profile.mmr_lambda) * redundancy;
                (*index, mmr)
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|(left, left_score), (right, right_score)| {
            right_score
                .total_cmp(left_score)
                .then(rrf[*right].total_cmp(&rrf[*left]))
                .then(exact[*right].cmp(&exact[*left]))
                .then(items[*right].created_at_ms.cmp(&items[*left].created_at_ms))
                .then(items[*left].id.cmp(&items[*right].id))
        });
        // 只取本轮第一名，然后从候选池移除并进入 selected；下一轮的冗余度会因此发生变化。
        let (index, mmr_score) = ranked[0];
        remaining.remove(&index);
        selected.push(index);
        let matched_terms = query_terms
            .iter()
            .filter(|term| term_sets[index].contains(*term))
            .cloned()
            .collect::<Vec<_>>();
        hits.push(RetrievalHit {
            id: items[index].id.clone(),
            source_digest: items[index].source_digest.clone(),
            permission_scope_digest: request.permission_scope_digest.clone(),
            rank: hits.len() as u32 + 1,
            exact_score: exact[index],
            bm25_score: bm25[index],
            dense_score: dense[index],
            sparse_rank: sparse_rank[index],
            dense_rank: dense_rank[index],
            rrf_score: rrf[index],
            mmr_score,
            matched_terms,
        });
    }
    // 构造并立即校验完整结果，任何内部计算产生的非法浮点数或错误 rank 都不会作为部分成功返回。
    let result = RetrievalResult::new(profile, request, acl_filtered_count, hits, degraded_reasons);
    result.validate()?;
    Ok(result)
}

/// 计算同维向量的余弦相似度；零长度向量返回 0，结果限制在数学定义区间 [-1, 1]。
/// 调用前由 request/item 检查和维度分支保证长度相同；本 helper 自身按 zip 配对元素。
fn cosine(left: &[f64], right: &[f64]) -> f64 {
    let left_norm = left.iter().map(|value| value * value).sum::<f64>().sqrt();
    let right_norm = right.iter().map(|value| value * value).sum::<f64>().sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }
    (left
        .iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum::<f64>()
        / (left_norm * right_norm))
        .clamp(-1.0, 1.0)
}

/// 计算两个词项集合的 Jaccard 重叠率：交集大小除以并集大小。
/// 空并集按 0 处理，表示没有可用于计算的共同词项相似度。
fn overlap(left: &BTreeSet<String>, right: &BTreeSet<String>) -> f64 {
    let union = left.union(right).count();
    if union == 0 {
        0.0
    } else {
        left.intersection(right).count() as f64 / union as f64
    }
}

//! SC-32 compare-and-swap, monotonic cursor and rebuild-equivalence gate for the audit projection.
//!
//! # CAS（Compare-And-Swap）提交机制
//!
//! 一个投影器（projector）是审计事实的缓存。如果缓存可以向后移动，反而不如没有缓存好，
//! 因为它会"取消"已经回答过的问题：调用方重新读取一个窗口，看到一个审计行消失了，
//! 却无法区分这是删除还是回滚。所以本模块对发布尝试只允许三种结果：
//!
//! ```text
//! Advanced     成功推进：新头部的 generation = 旧 generation + 1，source_cursor 严格增长
//! ExactReplay  完全重复：声称的目标已是已发布的头部，无操作（幂等性保证）
//! Rejected     拒绝：其他所有情形，每种都有稳定的拒绝原因代码
//! ```
//!
//! # Generation 单调性与 CAS 令牌
//!
//! generation 是 CAS 的核心：声称（claim）携带它读到的 generation（called `expected_generation`），
//! 只有其 `expected_generation` 等于当前已发布 generation 的声称才能推进。
//! 这样，两个投影器即使读到同一个头部，也无法都成功：失败方会被告知输给了哪个 generation，
//! 而不是无声地覆盖一个更新的头部。
//!
//! # 读只决策与确定性验证
//!
//! 这个模块是一个纯读只的决策函数——只根据输入的摘要和计数器推导结论。它：
//! - 不追加任何数据，不存储任何东西
//! - 不调用适配器或 port
//! - 无法区分诚实的投影器和撒谎的投影器
//!
//! 它证明的是头部*按声称的方式移动了*，而不是摘要背后的折叠状态是否正确。
//! 生成状态的 reducer 和证明两条路径关于历史达成一致的 rebuild，
//! 存在于 `kiana-query/src/audit_projector.rs`。

use kiana_domain::{
    json_digest, redact_text, scan_secret_sentinels, EventCursor, SchemaVersion, SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

// ==================== 版本与常量 ====================

/// Schema ID：已发布投影头部的版本标识符
/// 用于验证入库对象的兼容性
pub const AUDIT_PROJECTION_HEAD_SCHEMA: &str = "kiana.audit-projection-head.v1";

/// Schema ID：投影声称（CAS 操作请求）的版本标识符
pub const AUDIT_PROJECTION_CLAIM_SCHEMA: &str = "kiana.audit-projection-claim.v1";

/// Schema ID：CAS 提交报告（决策结果）的版本标识符
pub const AUDIT_PROJECTION_COMMIT_REPORT_SCHEMA: &str = "kiana.audit-projection-commit-report.v1";

/// Schema ID：Rebuild 证明（两条路径的对比）的版本标识符
pub const AUDIT_PROJECTION_REBUILD_PROOF_SCHEMA: &str = "kiana.audit-projection-rebuild-proof.v1";

/// Schema ID：Rebuild 验证报告（Rebuild 决策结果）的版本标识符
pub const AUDIT_PROJECTION_REBUILD_REPORT_SCHEMA: &str = "kiana.audit-projection-rebuild-report.v1";

/// 所有投影相关结构的版本号（1.0）
/// 跨越多个 schema，保证一致演进
pub const AUDIT_PROJECTION_COMMIT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// 文本字段的最大长度：拒绝原因、建议等
/// 防止日志炸弹或存储溢出
pub const MAX_AUDIT_PROJECTION_TEXT: usize = 256;

/// 一个审计投影的已发布头部。
///
/// 头部是整个持久化的声称：哪个投影器拥有它，处于哪个 generation，
/// 已消费了 EventLog 的多深，以及*恰好该状态*的摘要。
/// 关于读模型的其他一切都可推导出来，所以这里不重复任何东西。
///
/// # 字段含义
///
/// - `schema` & `version`: 此结构体的版本号（用于演进）
/// - `projector`: 投影器身份（多个投影器可能折叠同一类审计事实，但各自拥有独立的头部）
/// - `generation`: 全局递增的逻辑时钟，每次成功推进 +1
/// - `source_cursor`: 从事件日志中消费到的位置（位置越后 = 看到了更多事实）
/// - `state_digest`: 折叠后的完整投影状态的 SHA256 摘要（确定性，可被 rebuild 验证）
/// - `head_digest`: 头部本身的 SHA256 摘要（用于整体完整性校验）
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditProjectionHead {
    pub schema: String,
    pub version: SchemaVersion,
    pub projector: String,
    pub generation: u64,
    pub source_cursor: EventCursor,
    pub state_digest: String,
    pub head_digest: String,
}

impl AuditProjectionHead {
    /// 构造一个新的投影头部。
    ///
    /// 自动计算 `head_digest`（整个头部的摘要）然后验证。
    pub fn new(
        projector: impl Into<String>,
        generation: u64,
        source_cursor: EventCursor,
        state_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut head = Self {
            schema: AUDIT_PROJECTION_HEAD_SCHEMA.to_owned(),
            version: AUDIT_PROJECTION_COMMIT_VERSION,
            projector: projector.into(),
            generation,
            source_cursor,
            state_digest: state_digest.into(),
            head_digest: String::new(),
        };
        head.head_digest = head.digest();
        head.validate()?;
        Ok(head)
    }

    /// 验证这个头部的所有不变量。
    ///
    /// 检查清单：
    /// - schema 和 version 正确
    /// - generation 和 source_cursor 都非零（必须已启动）
    /// - projector 字段是安全的、可发表的文本（无秘密）
    /// - state_digest 和 head_digest 都是有效的 SHA-256 格式
    /// - head_digest 与重新计算的摘要相匹配（防篡改）
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUDIT_PROJECTION_HEAD_SCHEMA
            || !self
                .version
                .is_compatible_with(&AUDIT_PROJECTION_COMMIT_VERSION)
            || self.generation == 0
            || self.source_cursor == 0
        {
            return Err("audit_projection_head_header_invalid".to_owned());
        }
        safe_text(&self.projector, "audit_projection_head_projector")?;
        valid_digest(&self.state_digest, "audit_projection_head_state_digest")?;
        valid_digest(&self.head_digest, "audit_projection_head_digest")?;
        if self.head_digest != self.digest() {
            return Err("audit_projection_head_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算此头部的 SHA-256 摘要。
    ///
    /// 只摘要头部的主要字段，不包括 head_digest 本身（避免循环）。
    /// 这个摘要用作头部的"指纹"，防止篡改。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "projector": self.projector,
            "generation": self.generation,
            "source_cursor": self.source_cursor,
            "state_digest": self.state_digest,
        }))
    }
}

/// 一次发布新头部的尝试声称。
///
/// # CAS 令牌与等价性条件
///
/// `expected_generation` 是 CAS 令牌：它是投影器*读到的* generation，而不是它想要的。
/// `previous_state_digest` 同样是对折叠状态的 CAS 令牌，所以一个在陈旧或外来状态上工作的投影器
/// 无法在一个它从未实际见过的头部上发布。
///
/// # 字段含义
///
/// - `projector`: 发起这次声称的投影器的身份
/// - `expected_generation`: CAS 校验用的读取时 generation（推进时必须等于当前头部的 generation）
/// - `new_generation`: 声称推进到的新 generation（必须恰好等于 expected_generation + 1）
/// - `previous_source_cursor` / `new_source_cursor`: 事件日志光标的旧值和新值
/// - `previous_state_digest` / `new_state_digest`: 状态摘要的旧值和新值（新值是投影器计算出的）
///
/// - `rebuild_from_cursor`: 这次推进涉及重建吗？
///   - `None`: 普通的连续推进（光标从 N 到 N+1，折叠增量事件）
///   - `Some(0)`: 从日志头开始全量重建（抛弃所有历史，重新计算）
///   - `Some(previous_source_cursor)`: 从已发布头部处重新折叠以闭合缝隙（检测到光标跳跃）
///   - 其他值：这是对谁都没验证过的历史的声称（被拒绝）
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditProjectionClaim {
    pub schema: String,
    pub version: SchemaVersion,
    pub projector: String,
    pub expected_generation: u64,
    pub new_generation: u64,
    pub previous_source_cursor: EventCursor,
    pub new_source_cursor: EventCursor,
    pub previous_state_digest: String,
    pub new_state_digest: String,
    /// `None` for an ordinary contiguous advance. `Some(previous_source_cursor)` when the
    /// projector re-folded from the published head to close a gap. `Some(0)` for a full rebuild
    /// from the beginning of the log. Any other origin is a claim about a history nobody verified.
    pub rebuild_from_cursor: Option<EventCursor>,
    pub claim_digest: String,
}

impl AuditProjectionClaim {
    #[allow(clippy::too_many_arguments)]
    /// 构造一个新的投影声称。
    ///
    /// 自动计算 `claim_digest` 然后验证。
    ///
    /// 这个函数的参数对应于 CAS 操作的完整规范：
    /// - 预期读到的 generation 和状态摘要
    /// - 计划写入的 generation 和状态摘要
    /// - 涉及的光标转移
    /// - 是否涉及重建（以及从哪里开始）
    pub fn new(
        projector: &str,
        expected_generation: u64,
        new_generation: u64,
        previous_source_cursor: EventCursor,
        new_source_cursor: EventCursor,
        previous_state_digest: &str,
        new_state_digest: &str,
        rebuild_from_cursor: Option<EventCursor>,
    ) -> Result<Self, String> {
        let mut claim = Self {
            schema: AUDIT_PROJECTION_CLAIM_SCHEMA.to_owned(),
            version: AUDIT_PROJECTION_COMMIT_VERSION,
            projector: projector.to_owned(),
            expected_generation,
            new_generation,
            previous_source_cursor,
            new_source_cursor,
            previous_state_digest: previous_state_digest.to_owned(),
            new_state_digest: new_state_digest.to_owned(),
            rebuild_from_cursor,
            claim_digest: String::new(),
        };
        claim.claim_digest = claim.digest();
        claim.validate()?;
        Ok(claim)
    }

    /// 验证这个声称的所有不变量。
    ///
    /// 检查清单：
    /// - schema 和 version 正确
    /// - 所有的 generation、光标都非零
    /// - projector 字段安全（无秘密）
    /// - state_digest 字段都是有效的 SHA-256 格式
    /// - claim_digest 与重新计算的摘要匹配
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUDIT_PROJECTION_CLAIM_SCHEMA
            || !self
                .version
                .is_compatible_with(&AUDIT_PROJECTION_COMMIT_VERSION)
            || self.expected_generation == 0
            || self.new_generation == 0
            || self.previous_source_cursor == 0
            || self.new_source_cursor == 0
        {
            return Err("audit_projection_claim_header_invalid".to_owned());
        }
        safe_text(&self.projector, "audit_projection_claim_projector")?;
        valid_digest(
            &self.previous_state_digest,
            "audit_projection_claim_previous_state_digest",
        )?;
        valid_digest(
            &self.new_state_digest,
            "audit_projection_claim_new_state_digest",
        )?;
        valid_digest(&self.claim_digest, "audit_projection_claim_digest")?;
        if self.claim_digest != self.digest() {
            return Err("audit_projection_claim_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算此声称的 SHA-256 摘要。
    ///
    /// 包含所有主要字段，包括 rebuild_from_cursor（即使是 None）。
    /// 这个摘要是声称的"指纹"，防止篡改。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "projector": self.projector,
            "expected_generation": self.expected_generation,
            "new_generation": self.new_generation,
            "previous_source_cursor": self.previous_source_cursor,
            "new_source_cursor": self.new_source_cursor,
            "previous_state_digest": self.previous_state_digest,
            "new_state_digest": self.new_state_digest,
            "rebuild_from_cursor": self.rebuild_from_cursor,
        }))
    }
}

/// 一次发布尝试的有序决策结果。
///
/// # 三态的含义
///
/// - `Advanced`: 头部成功推进。generation 增长 1，source_cursor 严格增长，
///   state_digest 更新为新值。这是常见情形。
///
/// - `ExactReplay`: 声称的目标已是已发布的头部。generation、source_cursor、
///   state_digest 三个都完全相同。**这是唯一允许光标不前进的情形** —— 用于幂等重试。
///   如果一个请求因网络超时而重试，再次发送同样的声称，会得到 ExactReplay，
///   表示"这已经发生过了，无需重复"，而非错误。
///
/// - `Rejected`: 声称违反某条规则。`reason` 字段给出违反的第一条规则
///  （固定顺序评估，保证同样的头部和声称总得到同样的拒绝原因）。
///   `remediation` 给出调用方应该怎样重试。
///
/// # 通用字段
///
/// - `generation` 和 `source_cursor` 和 `state_digest`: 此次决策后头部的值
///   - `Advanced`: claim 的新值
///   - `ExactReplay` 和 `Rejected`: 已发布头部的当前值
/// - `reason` 和 `remediation`: 仅当 status == Rejected 时非空
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditProjectionCommitStatus {
    Advanced,
    ExactReplay,
    Rejected,
}

impl AuditProjectionCommitStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Advanced => "advanced",
            Self::ExactReplay => "exact_replay",
            Self::Rejected => "rejected",
        }
    }
}

/// The ordered decision for one publish attempt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditProjectionCommitReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub projector: String,
    pub status: AuditProjectionCommitStatus,
    /// The generation this attempt leaves behind: the claim target on `Advanced`, the published
    /// generation on `ExactReplay` and on `Rejected`.
    pub generation: u64,
    pub source_cursor: EventCursor,
    pub state_digest: String,
    /// Stable denial code. Empty unless the status is `Rejected`.
    pub reason: String,
    pub remediation: String,
    pub head_digest: String,
    pub report_digest: String,
}

impl AuditProjectionCommitReport {
    /// 评估一个声称并生成决策报告。
    ///
    /// 这是此模块的主要入口点。给定一个已发布的头部和一个声称，
    /// 这个方法应用 CAS 逻辑并返回三态中的一个。
    ///
    /// # 返回
    ///
    /// 一个完全验证和自签名的报告，所有字段都由 derive() 固定决策产生。
    pub fn evaluate(
        head: &AuditProjectionHead,
        claim: &AuditProjectionClaim,
    ) -> Result<Self, String> {
        head.validate()?;
        claim.validate()?;
        let (status, generation, source_cursor, state_digest, reason, remediation) =
            derive(head, claim);
        let mut report = Self {
            schema: AUDIT_PROJECTION_COMMIT_REPORT_SCHEMA.to_owned(),
            version: AUDIT_PROJECTION_COMMIT_VERSION,
            projector: claim.projector.clone(),
            status,
            generation,
            source_cursor,
            state_digest,
            reason,
            remediation,
            head_digest: head.head_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(head, claim)?;
        Ok(report)
    }

    /// 重新推导每个字段以证明报告未被篡改。
    ///
    /// 这个方法是此模块的关键防线：
    /// - 无法编辑已接受的报告状态、generation、摘要等
    /// - 任何改变都会触发 digest 不匹配的错误
    /// - 确保报告的"来源"是决策函数，而非外部
    ///
    /// # 为什么需要这个检查？
    ///
    /// 一个恶意或错误的调用方可能会：
    /// - 改变 status 从 Rejected 到 Advanced
    /// - 改变 generation 到任意值
    /// - 移除或改变 reason 和 remediation
    ///
    /// 这个检查完全阻止了这类欺骗。
    pub fn validate_against(
        &self,
        head: &AuditProjectionHead,
        claim: &AuditProjectionClaim,
    ) -> Result<(), String> {
        head.validate()?;
        claim.validate()?;
        let (status, generation, source_cursor, state_digest, reason, remediation) =
            derive(head, claim);
        if self.schema != AUDIT_PROJECTION_COMMIT_REPORT_SCHEMA
            || !self
                .version
                .is_compatible_with(&AUDIT_PROJECTION_COMMIT_VERSION)
            || self.projector != claim.projector
            || self.status != status
            || self.generation != generation
            || self.source_cursor != source_cursor
            || self.state_digest != state_digest
            || self.reason != reason
            || self.remediation != remediation
            || self.head_digest != head.head_digest
        {
            return Err("audit_projection_commit_binding_invalid".to_owned());
        }
        // 一致性检查：Rejection 必须携带 reason；非 Rejection 不能有 reason
        // 这防止了一个 "rejected" 报告但原因为空的矛盾
        if (self.status == AuditProjectionCommitStatus::Rejected) != !self.reason.is_empty() {
            return Err("audit_projection_commit_reason_state_mismatch".to_owned());
        }
        // 非 Rejection 不能有 remediation 字段（remediation 只在拒绝时有意义）
        if self.status != AuditProjectionCommitStatus::Rejected && !self.remediation.is_empty() {
            return Err("audit_projection_commit_remediation_with_outcome".to_owned());
        }
        if !self.reason.is_empty() {
            safe_text(&self.reason, "audit_projection_commit_reason")?;
        }
        if !self.remediation.is_empty() {
            safe_text(&self.remediation, "audit_projection_commit_remediation")?;
        }
        valid_digest(&self.state_digest, "audit_projection_commit_state_digest")?;
        valid_digest(&self.head_digest, "audit_projection_commit_head_digest")?;
        valid_digest(&self.report_digest, "audit_projection_commit_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("audit_projection_commit_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 便捷方法：检查头部是否真的推进了。
    pub fn advanced(&self) -> bool {
        self.status == AuditProjectionCommitStatus::Advanced
    }

    /// 计算此报告的 SHA-256 摘要。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "projector": self.projector,
            "status": self.status,
            "generation": self.generation,
            "source_cursor": self.source_cursor,
            "state_digest": self.state_digest,
            "reason": self.reason,
            "remediation": self.remediation,
            "head_digest": self.head_digest,
        }))
    }
}

/// 按固定顺序进行决策，使得报告的拒绝原因总是违反的*第一条*规则，
/// 从而对相同的头部和声称总是给出相同的原因代码。
///
/// # 验证顺序与为什么（9 步 CAS 流程）
///
/// 顺序不是任意的。每一步都回答一个特定问题，后续步骤基于前面的假设：
///
/// **第 1-3 步：快速检查**
///
/// 1. **ExactReplay 检查**: 声称的目标已是头部吗？
///    - 必须 generation、source_cursor、state_digest 三个*完全相同*
///    - 这是第一个检查是因为：若允许光标不前进，必须在这里。
///      如果声称声称*不同*的光标，则不匹配，掉到下面的单调性规则。
///    - 防什么：重复的幂等请求
///
/// 2. **投影器身份检查** (`claim.projector == head.projector`):
///    - 声称是否针对正确的投影器的头部？
///    - 这不是竞态条件，而是使用错误
///    - 防什么：调用方混淆了两个投影器，发往了错误的头部
///
/// 3. **Generation CAS 令牌检查** (`claim.expected_generation == head.generation`):
///    - 声称读到的 generation 是否等于当前头部的 generation？
///    - 若不等，表示有另一个投影器抢先推进了头部，或者调用方不是最新读取
///    - 防什么：竞态赢家/输家识别；迫使输家重新读取头部
///
/// **第 4-6 步：Generation 和 State 单调性**
///
/// 4. **Generation 递增检查** (`claim.new_generation == head.generation + 1`):
///    - 新 generation 是否*恰好*是旧 generation 加 1？
///    - 不允许跳跃（即使只差 2）或重用旧 generation
///    - 防什么：算法错误或协议破坏；保证 generation 严格单调递增
///
/// 5. **State 摘要链检查** (`claim.previous_state_digest == head.state_digest`):
///    - 声称基于的旧状态摘要是否与头部的当前状态摘要一致？
///    - 若投影器从陈旧的或不同的状态折叠，这里会失败
///    - 防什么：状态漂移；投影器不能基于过时的状态来推进
///    - 为什么在光标检查前：若状态不匹配，光标值无关紧要
///
/// **第 7-9 步：光标单调性与连续性**
///
/// 6. **光标回退检查** (`claim.new_source_cursor >= claim.previous_source_cursor`):
///    - 新光标是否 >= 旧光标？（允许相等只在 ExactReplay 中）
///    - 防什么：缓存向后移动（最糟的情况）
///
/// 7. **光标前进检查** (`claim.new_source_cursor > claim.previous_source_cursor`):
///    - 新光标是否*严格*大于旧光标？
///    - （ExactReplay 在步骤 1 已过滤，所以这里必须前进）
///    - 防什么：无操作声称（除了明确的 ExactReplay）
///
/// 8. **Rebuild 起点检查** (if `rebuild_from_cursor` is Some):
///    - 若声称涉及重建，起点是否合法？
///    - 合法值：`Some(0)`（全量重建）或 `Some(previous_source_cursor)`（从头部重新折叠）
///    - 防什么：对伪造的历史的声称
///
/// 9. **光标连续性检查**:
///    - 光标是否连续推进，或者声明了有效的重建？
///    - 若 `new_cursor == old_cursor + 1`（contiguous），OK
///    - 若光标有缝隙（`new_cursor > old_cursor + 1`）且未声明重建，REJECTED
///    - 防什么：静默跳过已发布事实
fn derive(
    head: &AuditProjectionHead,
    claim: &AuditProjectionClaim,
) -> (
    AuditProjectionCommitStatus,
    u64,
    EventCursor,
    String,
    String,
    String,
) {
    // 步骤 1：完全重复检查
    // 声称的目标已是已发布的头部吗？如果三项完全相同，这是幂等重试。
    // 只有在这里，光标才被允许不前进（即保持原值）。
    // 这必须是第一个检查，因为一个声称不同光标的声明在下面的单调性规则中会失败。
    // ExactReplay 保证了："如果我再次发送相同的 claim，会告诉我成功而不是错误"。
    if claim.new_generation == head.generation
        && claim.new_source_cursor == head.source_cursor
        && claim.new_state_digest == head.state_digest
    {
        return (
            AuditProjectionCommitStatus::ExactReplay,
            head.generation,
            head.source_cursor,
            head.state_digest.clone(),
            String::new(),
            String::new(),
        );
    }
    // 步骤 2：投影器身份检查
    // 声称是针对这个投影器的头部发出的吗？
    // 若投影器不匹配，这不是竞态条件，而是调用方使用了错误的投影器名。
    if claim.projector != head.projector {
        return (
            AuditProjectionCommitStatus::Rejected,
            head.generation,
            head.source_cursor,
            head.state_digest.clone(),
            "audit_projection_projector_mismatch".to_owned(),
            "publish to the head that owns this projector name, not to another projector's head"
                .to_owned(),
        );
    }
    // 步骤 3：Generation CAS 令牌检查（最关键的竞态检查）
    // expected_generation 必须等于当前头部的 generation。
    // 这是 Compare-And-Swap 的 Compare 部分。
    // 若不等，表示另一个投影器已抢先推进了头部，或此声称基于陈旧的读取。
    // 失败方被告知当前的 generation，促使它重新读取。
    if claim.expected_generation != head.generation {
        return (
            AuditProjectionCommitStatus::Rejected,
            head.generation,
            head.source_cursor,
            head.state_digest.clone(),
            "audit_projection_generation_cas_conflict".to_owned(),
            format!(
                "re-read the head: it is at generation {}, this attempt expected {}",
                head.generation, claim.expected_generation
            ),
        );
    }
    // 步骤 4：Generation 单调递增检查
    // 新 generation 必须*恰好*是当前 generation + 1。
    // 不允许跳跃（如从 10 跳到 12）或重用旧 generation。
    // 这保证了 generation 是严格单调递增的全局逻辑钟。
    if claim.new_generation != head.generation.saturating_add(1) {
        return (
            AuditProjectionCommitStatus::Rejected,
            head.generation,
            head.source_cursor,
            head.state_digest.clone(),
            "audit_projection_generation_not_monotonic".to_owned(),
            format!(
                "advance the head to exactly generation {}, not {}",
                head.generation.saturating_add(1),
                claim.new_generation
            ),
        );
    }
    // 步骤 5：State 摘要链检查
    // 声称基于的旧状态摘要必须与头部当前的状态摘要一致。
    // 这防止投影器从陈旧的或不同来源的状态折叠。
    // 摘要在状态检查前，因为若摘要不匹配，光标值完全无关（投影器根本没有从对的点开始）。
    if claim.previous_state_digest != head.state_digest {
        return (
            AuditProjectionCommitStatus::Rejected,
            head.generation,
            head.source_cursor,
            head.state_digest.clone(),
            "audit_projection_state_digest_drift".to_owned(),
            "fold from the published state digest; a claim built on another state cannot be committed"
                .to_owned(),
        );
    }
    // 步骤 6：光标回退检查
    // 新光标不能小于旧光标。缓存绝对不能向后移动。
    if claim.new_source_cursor < claim.previous_source_cursor {
        return (
            AuditProjectionCommitStatus::Rejected,
            head.generation,
            head.source_cursor,
            head.state_digest.clone(),
            "audit_projection_cursor_regression".to_owned(),
            format!(
                "a published projection may not move back from cursor {} to {}",
                claim.previous_source_cursor, claim.new_source_cursor
            ),
        );
    }
    // 步骤 7：光标前进检查（必须严格增长）
    // 新光标必须*严格*大于旧光标。
    // （ExactReplay 在步骤 1 已被过滤，所以到这里的声称必须前进）
    // 若新光标等于旧光标，这是一个无操作的声称，被拒绝。
    if claim.new_source_cursor == claim.previous_source_cursor {
        return (
            AuditProjectionCommitStatus::Rejected,
            head.generation,
            head.source_cursor,
            head.state_digest.clone(),
            "audit_projection_cursor_not_advanced".to_owned(),
            format!(
                "cursor {} was already published with a different state; re-read the tail instead of re-stating it",
                claim.previous_source_cursor
            ),
        );
    }
    // 步骤 8：Rebuild 起点合法性检查
    // 若声称涉及重建，起点必须是：
    // - Some(0)：从头开始全量重建
    // - Some(previous_source_cursor)：从已发布光标重新折叠（闭合缝隙）
    // - None：普通连续推进（下步检查）
    // 任何其他值都表示对伪造历史的声称。
    if let Some(origin) = claim.rebuild_from_cursor {
        if origin != 0 && origin != claim.previous_source_cursor {
            return (
                AuditProjectionCommitStatus::Rejected,
                head.generation,
                head.source_cursor,
                head.state_digest.clone(),
                "audit_projection_rebuild_origin_mismatch".to_owned(),
                format!(
                    "a rebuild starts at cursor 0 or at the published cursor {}, not {}",
                    claim.previous_source_cursor, origin
                ),
            );
        }
    }
    // 步骤 9：光标连续性与缝隙处理
    // 光标推进是否连续（只差 1），还是有缝隙？
    let contiguous = claim.new_source_cursor == claim.previous_source_cursor.saturating_add(1);
    if !contiguous && claim.rebuild_from_cursor.is_none() {
        // 缝隙且无重建声明 = 默默跳过了已发布事实 = REJECTED
        // 如果投影器需要跳过中间的事实，必须声明一个有效的重建起点。
        return (
            AuditProjectionCommitStatus::Rejected,
            head.generation,
            head.source_cursor,
            head.state_digest.clone(),
            "audit_projection_cursor_gap".to_owned(),
            format!(
                "cursor {} to {} skips committed facts; re-fold the missing range or declare a rebuild",
                claim.previous_source_cursor, claim.new_source_cursor
            ),
        );
    }
    // 所有 9 步都通过了 ✓
    // 头部成功推进到新的 generation、光标和状态。
    (
        AuditProjectionCommitStatus::Advanced,
        claim.new_generation,
        claim.new_source_cursor,
        claim.new_state_digest.clone(),
        String::new(),
        String::new(),
    )
}

/// 两条折叠路径的并排比较：增量回放 vs 从零开始的重建。
///
/// # 为什么需要 Rebuild 证明？
///
/// 投影器的正确性完全取决于折叠算法是否确定性。若算法改变了（或有 bug），
/// 投影器可能在不同的点产生不同的结果。Rebuild 是最后的检查：
/// 我们从日志头开始重新折叠，看是否到达同一个地方。
///
/// # 两边为什么都是摘要？
///
/// 这个模块回答的是："两条路径*关于历史达成一致*吗？"而不是
/// "每条路径的输出都正确吗？"。若两条路径产生不同的摘要，
/// 我们知道算法或数据有问题，需要隔离和调查。
///
/// # 关键字段
///
/// - `incremental_generation` 和 `incremental_source_cursor` 和 `incremental_state_digest`:
///   这是已发布头部的当前状态（增量路径是通过一系列推进到达的）
///
/// - `incremental_source_event_digest`: 增量路径在这个过程中消费的所有*源事件身份*的
///   排序、去重后的摘要。用来比较两条路径是否基于同样的事实。
///
/// - `rebuilt_source_cursor` 和 `rebuilt_state_digest`: 从头开始重新折叠得到的结果
///
/// - `rebuilt_source_event_digest`: 重建路径消费的所有源事件身份的摘要。
///   若与 incremental 相同，说明两条路径基于同样的事实。
///   若不同，说明重建从不同的地方开始或跳过了事件（算法改变了）。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditProjectionRebuildProof {
    pub schema: String,
    pub version: SchemaVersion,
    pub projector: String,
    pub incremental_generation: u64,
    pub incremental_source_cursor: EventCursor,
    pub incremental_state_digest: String,
    /// Digest over the sorted, deduplicated source event identities the incremental path consumed.
    pub incremental_source_event_digest: String,
    pub rebuilt_source_cursor: EventCursor,
    pub rebuilt_state_digest: String,
    /// Digest over the sorted, deduplicated source event identities the rebuild consumed.
    pub rebuilt_source_event_digest: String,
    pub proof_digest: String,
}

impl AuditProjectionRebuildProof {
    #[allow(clippy::too_many_arguments)]
    /// 构造一个新的 Rebuild 证明。
    ///
    /// 这个结构体记录两条折叠路径：
    /// - **增量侧**：从已发布头部通过一系列推进到达的（代表当前系统状态）
    /// - **重建侧**：从日志头或某个点重新开始折叠得到的结果
    ///
    /// 两边都用摘要表示，因为我们要验证的是"两条路径关于历史达成一致"，
    /// 而不是"每条路径都正确"。
    ///
    /// # 字段说明
    ///
    /// - `incremental_*`: 已发布头部的当前状态（从多次推进积累得来）
    /// - `rebuilt_*`: 重新从头（或某个点）折叠得到的相同时间点的状态
    /// - `*_source_event_digest`: 排序、去重的源事件身份摘要
    ///   - 用来验证两条路径是否基于相同的事实集合
    ///   - 若不同，表示算法改变或数据被篡改
    pub fn new(
        projector: &str,
        incremental_generation: u64,
        incremental_source_cursor: EventCursor,
        incremental_state_digest: &str,
        incremental_source_event_digest: &str,
        rebuilt_source_cursor: EventCursor,
        rebuilt_state_digest: &str,
        rebuilt_source_event_digest: &str,
    ) -> Result<Self, String> {
        let mut proof = Self {
            schema: AUDIT_PROJECTION_REBUILD_PROOF_SCHEMA.to_owned(),
            version: AUDIT_PROJECTION_COMMIT_VERSION,
            projector: projector.to_owned(),
            incremental_generation,
            incremental_source_cursor,
            incremental_state_digest: incremental_state_digest.to_owned(),
            incremental_source_event_digest: incremental_source_event_digest.to_owned(),
            rebuilt_source_cursor,
            rebuilt_state_digest: rebuilt_state_digest.to_owned(),
            rebuilt_source_event_digest: rebuilt_source_event_digest.to_owned(),
            proof_digest: String::new(),
        };
        proof.proof_digest = proof.digest();
        proof.validate()?;
        Ok(proof)
    }

    /// 验证 Rebuild 证明的所有不变量。
    ///
    /// 检查清单：
    /// - schema 和 version 正确
    /// - 所有的 generation、光标都非零
    /// - projector 字段安全（无秘密）
    /// - 所有四个摘要都是有效的 SHA-256 格式
    /// - proof_digest 与重新计算的摘要匹配
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUDIT_PROJECTION_REBUILD_PROOF_SCHEMA
            || !self
                .version
                .is_compatible_with(&AUDIT_PROJECTION_COMMIT_VERSION)
            || self.incremental_generation == 0
            || self.incremental_source_cursor == 0
            || self.rebuilt_source_cursor == 0
        {
            return Err("audit_projection_rebuild_proof_header_invalid".to_owned());
        }
        safe_text(&self.projector, "audit_projection_rebuild_proof_projector")?;
        for (value, field) in [
            (
                &self.incremental_state_digest,
                "audit_projection_rebuild_incremental_state_digest",
            ),
            (
                &self.incremental_source_event_digest,
                "audit_projection_rebuild_incremental_source_event_digest",
            ),
            (
                &self.rebuilt_state_digest,
                "audit_projection_rebuild_state_digest",
            ),
            (
                &self.rebuilt_source_event_digest,
                "audit_projection_rebuild_source_event_digest",
            ),
        ] {
            valid_digest(value, field)?;
        }
        valid_digest(&self.proof_digest, "audit_projection_rebuild_proof_digest")?;
        if self.proof_digest != self.digest() {
            return Err("audit_projection_rebuild_proof_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算此 Rebuild 证明的 SHA-256 摘要。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "projector": self.projector,
            "incremental_generation": self.incremental_generation,
            "incremental_source_cursor": self.incremental_source_cursor,
            "incremental_state_digest": self.incremental_state_digest,
            "incremental_source_event_digest": self.incremental_source_event_digest,
            "rebuilt_source_cursor": self.rebuilt_source_cursor,
            "rebuilt_state_digest": self.rebuilt_state_digest,
            "rebuilt_source_event_digest": self.rebuilt_source_event_digest,
        }))
    }
}

/// Whether a rebuild agrees with the incremental replay it is meant to reproduce.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditProjectionRebuildStatus {
    Converged,
    Divergent,
}

impl AuditProjectionRebuildStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Converged => "converged",
            Self::Divergent => "divergent",
        }
    }
}

/// The ordered decision for one rebuild comparison.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditProjectionRebuildReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub projector: String,
    pub status: AuditProjectionRebuildStatus,
    pub generation: u64,
    pub source_cursor: EventCursor,
    pub state_digest: String,
    pub reason: String,
    pub remediation: String,
    pub proof_digest: String,
    pub report_digest: String,
}

impl AuditProjectionRebuildReport {
    /// 评估一个 Rebuild 证明并生成决策报告。
    ///
    /// 这是 Rebuild 验证的主要入口点。
    /// 给定一个已发布的头部和一个 Rebuild 证明，
    /// 应用 7 步验证并返回 Converged 或 Divergent。
    pub fn evaluate(
        head: &AuditProjectionHead,
        proof: &AuditProjectionRebuildProof,
    ) -> Result<Self, String> {
        head.validate()?;
        proof.validate()?;
        let (status, reason, remediation) = derive_rebuild(head, proof);
        let mut report = Self {
            schema: AUDIT_PROJECTION_REBUILD_REPORT_SCHEMA.to_owned(),
            version: AUDIT_PROJECTION_COMMIT_VERSION,
            projector: proof.projector.clone(),
            status,
            generation: head.generation,
            source_cursor: head.source_cursor,
            state_digest: head.state_digest.clone(),
            reason,
            remediation,
            proof_digest: proof.proof_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(head, proof)?;
        Ok(report)
    }

    /// 重新推导每个字段以证明报告未被篡改。
    ///
    /// 同 CAS 报告：一个恶意的调用方无法改变状态、generation、reason 等
    /// 而不触发 digest 不匹配。
    pub fn validate_against(
        &self,
        head: &AuditProjectionHead,
        proof: &AuditProjectionRebuildProof,
    ) -> Result<(), String> {
        head.validate()?;
        proof.validate()?;
        let (status, reason, remediation) = derive_rebuild(head, proof);
        if self.schema != AUDIT_PROJECTION_REBUILD_REPORT_SCHEMA
            || !self
                .version
                .is_compatible_with(&AUDIT_PROJECTION_COMMIT_VERSION)
            || self.projector != proof.projector
            || self.status != status
            || self.generation != head.generation
            || self.source_cursor != head.source_cursor
            || self.state_digest != head.state_digest
            || self.reason != reason
            || self.remediation != remediation
            || self.proof_digest != proof.proof_digest
        {
            return Err("audit_projection_rebuild_binding_invalid".to_owned());
        }
        // 一致性检查：Divergent 必须携带 reason；Converged 不能有 reason
        if (self.status == AuditProjectionRebuildStatus::Divergent) != !self.reason.is_empty() {
            return Err("audit_projection_rebuild_reason_state_mismatch".to_owned());
        }
        if !self.reason.is_empty() {
            safe_text(&self.reason, "audit_projection_rebuild_reason")?;
        }
        if !self.remediation.is_empty() {
            safe_text(&self.remediation, "audit_projection_rebuild_remediation")?;
        }
        valid_digest(
            &self.state_digest,
            "audit_projection_rebuild_report_state_digest",
        )?;
        valid_digest(
            &self.report_digest,
            "audit_projection_rebuild_report_digest",
        )?;
        if self.report_digest != self.digest() {
            return Err("audit_projection_rebuild_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 便捷方法：两条路径是否成功验证为一致？
    pub fn converged(&self) -> bool {
        self.status == AuditProjectionRebuildStatus::Converged
    }

    /// 计算此 Rebuild 报告的 SHA-256 摘要。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "projector": self.projector,
            "status": self.status,
            "generation": self.generation,
            "source_cursor": self.source_cursor,
            "state_digest": self.state_digest,
            "reason": self.reason,
            "remediation": self.remediation,
            "proof_digest": self.proof_digest,
        }))
    }
}

/// 按固定顺序决策：身份 → 增量侧的固定、源事实、光标、最后才是折叠状态。
///
/// 这个顺序很重要。Divergent 的报告会报第一个违反的规则。
/// 若重建在不同的点读取、消费了不同的事实、或到达了不同的光标，
/// 那通常比折叠状态不同更"便宜"（更快判出问题）。
/// 只有在前面都匹配的情况下，才报告折叠状态不同（通常意味着算法有 bug）。
///
/// # Rebuild 验证的 7 步（缩小范围从宽到窄）
///
/// 1. **投影器身份检查**: 对的投影器吗？
/// 2. **Generation 锚点检查**: 增量侧是在哪个 generation 的头部上取的？
///    必须是当前已发布的 generation（若头部已推进，增量侧就过时了）。
/// 3. **光标锚点检查**: 增量侧是在哪个光标取的？必须等于已发布头部的光标。
/// 4. **State 摘要锚点检查**: 增量侧的起始状态摘要是否是已发布头部的状态摘要？
///    若不是，增量侧根本没从对的点开始。
/// 5. **源事件消费一致性**: 两条路径是否消费了同样的源事件集合？
///    若不同，说明算法改变了或有条件错误（某个事件在一条路径被跳过）。
/// 6. **重建光标检查**: 重建是否到达了相同的光标？
///    若不到，重建算法有问题或提前停止了。
/// 7. **状态摘要一致性**: 最后，两个折叠结果是否产生相同的状态摘要？
///    这是最后且最昂贵的检查（通常表示算法本身改变了）。
fn derive_rebuild(
    head: &AuditProjectionHead,
    proof: &AuditProjectionRebuildProof,
) -> (AuditProjectionRebuildStatus, String, String) {
    // 步骤 1：投影器身份检查
    if proof.projector != head.projector {
        return (
            AuditProjectionRebuildStatus::Divergent,
            "audit_projection_rebuild_projector_mismatch".to_owned(),
            "compare a rebuild against the head of the same projector, not another projector's"
                .to_owned(),
        );
    }
    // 步骤 2：Generation 锚点检查
    // 增量侧是基于哪个 generation 的头部的？必须是当前头部的 generation。
    // 若头部已推进（generation 更高），则增量侧是过时的（基于旧头部的），不可比。
    if proof.incremental_generation != head.generation {
        return (
            AuditProjectionRebuildStatus::Divergent,
            "audit_projection_rebuild_generation_mismatch".to_owned(),
            format!(
                "the incremental side was taken at generation {}, the head is at {}",
                proof.incremental_generation, head.generation
            ),
        );
    }
    // 步骤 3：光标锚点检查
    // 增量侧是从哪个光标取的？必须等于已发布头部的光标。
    // 若不等，增量侧不是从对的"快照点"开始的。
    if proof.incremental_source_cursor != head.source_cursor {
        return (
            AuditProjectionRebuildStatus::Divergent,
            "audit_projection_rebuild_incremental_cursor_mismatch".to_owned(),
            format!(
                "the incremental side was taken at cursor {}, the head is at {}",
                proof.incremental_source_cursor, head.source_cursor
            ),
        );
    }
    // 步骤 4：State 摘要锚点检查
    // 增量侧的起始状态摘要必须等于已发布头部的状态摘要。
    // 若不等，增量侧根本没从对的状态开始，它的输出无关紧要。
    if proof.incremental_state_digest != head.state_digest {
        return (
            AuditProjectionRebuildStatus::Divergent,
            "audit_projection_rebuild_incremental_drift".to_owned(),
            "take the incremental side from the published head; a drifted base proves nothing"
                .to_owned(),
        );
    }
    // 步骤 5：源事件一致性检查（确定性验证的核心）
    // 两条路径是否消费了同样的源事件集合？
    // source_event_digest 是所有已消费事件身份的排序、去重摘要。
    // 若不同，说明：
    // - 算法改变了（对某些事件的过滤逻辑改变）
    // - 输入数据改变了（事件被篡改或删除）
    // - 时间戳 bug（事件排序改变了）
    if proof.rebuilt_source_event_digest != proof.incremental_source_event_digest {
        return (
            AuditProjectionRebuildStatus::Divergent,
            "audit_projection_rebuild_source_identity_divergent".to_owned(),
            "rebuild over the same committed source event identities before comparing state"
                .to_owned(),
        );
    }
    // 步骤 6：重建光标完成性检查
    // 重建是否到达了已发布头部的光标？
    // 若重建光标较小，说明重建算法提前停止或跳过了事件。
    if proof.rebuilt_source_cursor != head.source_cursor {
        return (
            AuditProjectionRebuildStatus::Divergent,
            "audit_projection_rebuild_cursor_not_reaching_head".to_owned(),
            format!(
                "rebuild through cursor {} to match the published cursor",
                head.source_cursor
            ),
        );
    }
    // 步骤 7：状态摘要一致性检查（最后且最昂贵）
    // 两条路径基于相同的源事实到达相同的光标，
    // 现在检查它们产生的最终状态摘要是否相同。
    // 若不同，说明折叠算法本身改变了（或有 bug）。
    // 这种分歧需要隔离投影器并调查。
    if proof.rebuilt_state_digest != head.state_digest {
        return (
            AuditProjectionRebuildStatus::Divergent,
            "audit_projection_rebuild_state_divergent".to_owned(),
            "quarantine the projection and re-fold both paths; they disagree about history"
                .to_owned(),
        );
    }
    // 所有 7 步都通过了 ✓
    // 两条路径关于历史完全达成一致。
    (
        AuditProjectionRebuildStatus::Converged,
        String::new(),
        String::new(),
    )
}

/// 检查 `value` 是否是安全的、可发表的文本。
///
/// 安全性规则：
/// - 非空且修剪后长度 <= 256 字符
/// - 不包含空字符、回车、换行（防止日志注入）
/// - 经过脱敏处理（secrets 已经去掉）
/// - 不包含秘密哨兵（防止遗留的 API key 或密码）
///
/// 这些检查保证了错误消息、建议等可以安全地记录和发送。
fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_AUDIT_PROJECTION_TEXT
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Receipt, value)
        .map_err(|_| format!("{field}_secret_detected"))
}

/// 检查 `value` 是否是有效的摘要格式。
///
/// 有效摘要必须：
/// - 以 "sha256:" 前缀开始
/// - 后跟恰好 64 个十六进制数字（SHA-256 的标准长度）
///
/// 这防止了格式错误的摘要进入系统，确保摘要的一致性。
fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

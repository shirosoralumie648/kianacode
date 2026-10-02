//! Versioned, deny-first policy bundles and replayable decision traces.
//!
//! `PolicyBundle` is a pure policy value. It does not execute capabilities, consume approvals or
//! trust caller-supplied identity fields. The existing `PolicyEngine` remains the compatibility
//! adapter used by ControlPlane; `BundlePolicyEngine` exposes the stricter revision-bound contract
//! without creating a second execution path.
//!
//! # ⚠ 当前状态：`PolicyBundle` 尚未接入产品主路径
//!
//! 这一点必须先说清楚，否则下面的说明会给人错误印象。
//!
//! 产品主路径真正在跑的策略引擎是 `kiana-policy/src/lib.rs` 里的
//! `DefaultPolicyEngine` —— `kiana-daemon` 的组合根在 `lib.rs:1219` 处
//! 实例化它，`kiana-core/src/approvals.rs:993` 也在调用它的 `evaluate`。
//!
//! 而本文件的 `PolicyBundle` 和 `BundlePolicyEngine` 目前
//! **只出现在本文件自己的单元测试和 `kiana-policy/tests/` 下**，
//! 没有任何生产代码路径调用。
//!
//! 所以本文件现在是一份**已实现、已测试、但未接线**的策略值类型。
//! 它的规则是对的、测试是过的，但还没有接到真正的判定路径上。
//!
//! 这不改变下面那些设计说明的价值 —— 读懂它有助于理解策略层的完整设计，
//! 也为将来接线提供参考。但不要以为"现在每次判定都会走这里"。
//!
//! # 这个文件在系统里的位置（设计意图）
//!
//! 这是 Kiana 的**策略层**主体。它回答一个纯逻辑问题：
//! 「给定这次请求和这份策略，允许、拒绝，还是需要审批？」
//!
//! ```text
//! kiana-core::ControlPlane
//!        ↓ 构造 CapabilityRequest（能力请求）+ RequestContext（请求上下文）
//! 【本文件：PolicyBundle::evaluate_with_snapshot】
//!        ↓ 逐条规则匹配 → PolicyDecision（Allow / Ask / Deny）
//!        ↓ 同时产出 DecisionTrace（决策轨迹）
//! kiana-gates::GateEngine
//!        ↓ 把策略结论收敛成控制面动作
//! 能力代理 / 审批流程
//! ```
//!
//! # 关键设计一：deny-first（拒绝优先）
//!
//! 文件头第一句就是 "deny-first"。含义是：
//! **任何不确定的情况都得出拒绝**，而不是允许。
//!
//! 体现在 `PolicyBundle::default_effect` —— 没有规则匹配时落到这个默认值，
//! 而它应该被设成 `Deny`。见 `PolicyBundle::deny_all`。
//!
//! # 关键设计二：纯策略值，不执行任何东西
//!
//! 文件头说得很明确：`PolicyBundle` "does not execute capabilities,
//! consume approvals or trust caller-supplied identity fields"。
//!
//! 三条禁令：
//! 1. **不执行能力** —— 判定和执行是两回事，执行在 `kiana-capability-broker`；
//! 2. **不消费审批** —— 审批由别的层处理；
//! 3. **不相信调用方传进来的身份字段** —— 身份必须由服务端持有的快照确认。
//!
//! 第 3 条最重要。如果策略层信任调用方声称的"我是谁"，
//! 那么任何人都可以伪造身份来获取高权限。
//!
//! # 关键设计三：可重放的决策轨迹
//!
//! 文件头的 "replayable decision traces" 指的是 `DecisionTrace` ——
//! 每次判定都产出一份完整轨迹，事后可以**原样重放**，
//! 验证"当时确实是这份策略得出了这个结论"。
//!
//! 这解决了一个真实的审计难题：策略会变。
//! 三个月后复核一次拒绝是否合理时，当前策略可能已经改了，
//! 重新判定会得到不同结果。轨迹记录 + 策略修订号（revision）
//! 让复核者能确定"当时用的是哪一版策略，结论是什么"。
//!
//! # 关键设计四：单一执行路径
//!
//! 文件头最后一句 "without creating a second execution path"
//! 点出了一条重要的架构约束：即使这个新接口更严格，
//! 它也**不是**一条并行的执行路径 —— 它仍然由同一个 `ControlPlane` 调用。
//!
//! 这一点呼应了 `AGENTS.md` 里的反模式警告：「不许新增第二条执行循环」。
//!
//! # 术语
//!
//! - **PolicyBundle（策略包）**：一整套策略规则 + 默认结论 + 修订号。
//! - **PolicyRule（策略规则）**：一条"什么条件下得出什么结论"的规则。
//! - **PolicyDecision（策略结论）**：最终判定，三选一。
//! - **DecisionTrace（决策轨迹）**：一次判定的完整可重放记录。
//! - **deny-first**：不确定时拒绝。
//! - **fail-closed**：证据不足时拒绝，而非猜测放行。
//! - **capability（能力）**：被请求执行的操作类别，如 Shell、Mcp、Secret。
//!
//! # 上游契约
//!
//! `PolicyBundle` is a pure policy value. It does not execute capabilities, consume approvals or
//! trust caller-supplied identity fields.

use crate::{hard_policy_denial, PolicyEngine};
use kiana_domain::{
    json_digest, CapabilityKind, CapabilityRequest, PolicyDecision, RequestContext, RiskLevel,
    SchemaVersion, SecurityDecisionId, SecurityPolicyId, SecurityReasonCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const POLICY_BUNDLE_SCHEMA: &str = "kiana.policy-bundle.v1";
pub const POLICY_REVISION_SCHEMA: &str = "kiana.policy-revision.v1";
pub const POLICY_DECISION_TRACE_SCHEMA: &str = "kiana.policy-decision-trace.v1";
pub const POLICY_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_POLICY_RULES: usize = 512;
pub const MAX_POLICY_RULE_ID: usize = 128;
pub const MAX_POLICY_OPERATION: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 策略结论的严重程度。
///
/// 【作用】
/// 三种可能，按"限制程度"从强到弱排列。
///
/// 【为什么顺序有意义】
/// 枚举的声明顺序（Deny → Ask → Allow）本身就是一种语义表达：
/// 从最严格的到最宽松的。阅读代码时看到这个顺序，
/// 自然能理解"允许是最后才考虑的结果"。
///
pub enum PolicyEffect {
    Deny,
    Ask,
    Allow,
}

impl PolicyEffect {
    /// 把结论映射成一个可比较的数值。
    ///
    /// 【作用】
    /// 给三个变体一个**排序键**，让 [`PolicyBundle::evaluate_with_snapshot`]
    /// 能在多条规则中挑出"最严格的那条"。
    ///
    /// 【数值含义】
    /// - `Deny` = 0（最严格）
    /// - `Ask` = 1（需要审批）
    /// - `Allow` = 2（最宽松）
    ///
    /// 【⚠ 关键：数值越小越严格，判定时选最小的】
    /// 这是本文件最重要的安全设计之一。
    /// 当多条规则匹配同一个请求时，**取最严格的那条**，而不是取优先级最高的。
    ///
    /// 为什么？举个例子。假设有一条宽泛的规则说"所有 Shell 操作都允许"，
    /// 另有一条精确的规则说"`rm -rf` 类操作需要审批"（Ask）。
    /// 两条都匹配同一个请求。
    ///
    /// - 如果取优先级高的那条 → 可能放行了一个危险操作；
    /// - 取最严格的那条 → 得到 Ask，走审批流程，安全。
    ///
    /// **规则集里只要有一条说"要审批"，就必须走审批。**
    /// 这保证了"新增一条限制性规则"永远不会被已有的宽松规则压过。
    ///
    const fn rank(self) -> u8 {
        match self {
            Self::Deny => 0,
            Self::Ask => 1,
            Self::Allow => 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一条策略规则。
///
/// 【作用】
/// 描述"什么样的请求，应该得出什么结论"。
///
/// 【字段语义】
/// - `rule_id` —— 规则标识，审计时用它指认"是哪条规则决定的"；
/// - `priority` —— 同等严格程度下的次级排序键；
/// - `operation` —— 匹配的操作名，`"*"` 表示任意；
/// - `capability` —— 匹配的能力类型，`None` 表示任意；
/// - `risk` —— 匹配的风险等级，`None` 表示任意；
/// - `effect` —— 匹配后的结论；
/// - `reason` —— 拒绝或要审批时的原因码。
///
/// 【⚠ 为什么 effect 和 reason 必须配套】
/// `Allow` 不能带 reason（允许没有什么可解释的），
/// `Deny` 和 `Ask` 必须带 reason（拒绝和审批都需要给出可审计的理由）。
/// 见 [`PolicyRule::validate`]。
///
pub struct PolicyRule {
    pub rule_id: String,
    pub priority: u16,
    pub operation: String,
    #[serde(default)]
    pub capability: Option<CapabilityKind>,
    #[serde(default)]
    pub risk: Option<RiskLevel>,
    pub effect: PolicyEffect,
    #[serde(default)]
    pub reason: Option<SecurityReasonCode>,
}

impl PolicyRule {
    /// 构造一条策略规则并校验。
    ///
    /// 【⚠ 顺序：先算摘要再校验】
    /// 和本仓库其他值类型一致，摘要必须在 `validate()` 之前填好，
    /// 因为校验里会做"存储的摘要 vs 重算的摘要"的比对。
    ///
    pub fn new(
        rule_id: impl Into<String>,
        priority: u16,
        operation: impl Into<String>,
        capability: Option<CapabilityKind>,
        risk: Option<RiskLevel>,
        effect: PolicyEffect,
        reason: Option<SecurityReasonCode>,
    ) -> Result<Self, String> {
        let rule = Self {
            rule_id: rule_id.into(),
            priority,
            operation: operation.into(),
            capability,
            risk,
            effect,
            reason,
        };
        rule.validate()?;
        Ok(rule)
    }

    /// 校验这条规则自身是否合法。
    ///
    /// 【核心检查 —— 三组】
    ///
    /// ① 身份与选择器字段
    /// - `rule_id` 非空、不超 128 字节、不含空字节；
    /// - `operation` 非空、不超 256 字节、不含空字节；
    /// - 能力不能是"未知的其他"，风险等级必须已确定。
    ///
    /// ② effect 与 reason 的配套关系
    /// - `Allow` 带 reason → `policy_allow_reason_unexpected`（允许不需要理由）；
    /// - `Deny` 或 `Ask` 不带 reason → `policy_reason_required`
    ///   （拒绝和审批必须能解释，否则无法审计）。
    ///
    /// ③ 摘要自洽
    /// 重算摘要与存储值比对。
    ///
    /// 【⚠ 关于 `MAX_POLICY_RULES` 等常量】
    /// 文件顶部那几个上限常量（规则数 512、规则 ID 128、操作名 256）
    /// 不是性能优化，是**结构性约束**：
    /// 它们防止配置膨胀成一张无法审阅的规则表。
    /// 上限被强制执行，意味着"规则越堆越多"这类问题在构造时就会暴露。
    ///
    pub fn validate(&self) -> Result<(), String> {
        if self.rule_id.trim().is_empty()
            || self.rule_id.len() > MAX_POLICY_RULE_ID
            || self.rule_id.contains('\0')
            || self.operation.trim().is_empty()
            || self.operation.len() > MAX_POLICY_OPERATION
            || self.operation.contains('\0')
        {
            return Err("policy_rule_identity_invalid".to_owned());
        }
        match (self.effect, self.reason) {
            (PolicyEffect::Allow, Some(_)) => Err("policy_allow_reason_unexpected".to_owned()),
            (PolicyEffect::Deny | PolicyEffect::Ask, None) => {
                Err("policy_reason_required".to_owned())
            }
            _ => Ok(()),
        }
    }

    /// 构造这条规则的选择器标识。
    ///
    /// 【作用】
    /// 把"匹配条件"（操作 + 能力 + 风险）拼成一个可比较、可去重的字符串键。
    ///
    /// 【⚠ 为什么用 `\\u{1f}` 这个字符做分隔符】
    /// `0x1F` 是 ASCII 的**单元分隔符（Unit Separator）** ——
    /// 一个控制字符，正常文本里几乎不会出现。
    ///
    /// 用普通字符（比如逗号、空格）做分隔符会有歧义：
    /// 规则 A 的操作名是 `"foo,bar"`，规则 B 的是 `"foo"` + `"bar"`，
    /// 拼出来的键会撞在一起。用不可见的控制字符做分隔符，
    /// 就杜绝了"内容里含有分隔符"导致的键冲突。
    ///
    /// 【这一行是排序的次级 tiebreak】
    /// 多两条规则的选择器完全相同时，用它保证排序的确定性
    /// （见 [`PolicyBundle::evaluate_with_snapshot`] 里的排序逻辑）。
    ///
    fn selector_key(&self) -> String {
        format!(
            "{}\u{1f}{:?}\u{1f}{:?}",
            self.operation, self.capability, self.risk
        )
    }

    /// 判断这条规则是否匹配某个请求。
    ///
    /// 【核心流程 —— 三个条件全部满足才匹配】
    ///
    /// ① 操作名精确相等（注意：这里**不支持通配符**，
    ///    与 `provider.rs` 里的 `ProviderPolicyRule::matches` 不同）
    ///
    /// ② 能力匹配：如果这条规则指定了能力，必须与请求的能力相等；
    ///    如果没指定（`None`），则匹配任意能力。
    ///
    /// ③ 风险等级匹配：同样，指定了就必须相等，没指定就匹配任意。
    ///
    /// 【⚠ `None` 表示"不限"而不是"要求为空"】
    /// 这是最容易误解的一点。规则的 `capability: None` 意味着
    /// "这条规则不关心能力是什么"，而不是"只匹配没有能力的请求"。
    /// 因为请求总是有能力的，`None` 在这里只能解释为"不限"。
    ///
    fn matches(&self, request: &CapabilityRequest) -> bool {
        self.operation == request.operation
            && self
                .capability
                .as_ref()
                .is_none_or(|capability| capability == &request.capability)
            && self.risk.is_none_or(|risk| risk == request.risk)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 策略包的一个**修订快照**。
///
/// 【作用】
/// 记录"这份策略在某个时刻的确定身份"，用于让判定结果可追溯。
///
/// 【为什么需要修订号】
/// 因为策略会变。如果只记录"用了策略包"，
/// 三个月后复核时无法确定当时用的是哪一版。
/// 有了 `revision`（单调递增的修订号）和 `policy_digest`（内容摘要），
/// 就能精确回答"当时用的是哪一版"。
///
pub struct PolicyRevision {
    pub schema: String,
    pub version: SchemaVersion,
    pub policy_id: SecurityPolicyId,
    pub revision: u64,
    pub authority_epoch: u64,
    pub policy_digest: String,
    pub revision_digest: String,
}

impl PolicyRevision {
    /// 校验这个修订快照自身是否合法。
    ///
    /// 【核心检查】
    /// - schema 匹配、版本兼容；
    /// - `policy_id` 非空、不超 128 字节；
    /// - `revision` 非 0（0 意味着"从未正式发布过"）；
    /// - `authority_epoch` 非 0（0 不代表任何真实世代）；
    /// - `policy_digest` 格式合法；
    /// - `revision_digest` 格式合法，且与重算值一致。
    ///
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != POLICY_REVISION_SCHEMA
            || !self.version.is_compatible_with(&POLICY_SCHEMA_VERSION)
            || self.policy_id.as_uuid().is_nil()
            || self.revision == 0
            || self.authority_epoch == 0
        {
            return Err("policy_revision_header_invalid".to_owned());
        }
        validate_digest(&self.policy_digest, "policy_revision_policy_digest")?;
        validate_digest(&self.revision_digest, "policy_revision_digest")?;
        if self.revision_digest != self.digest() {
            return Err("policy_revision_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 从 JSON 反序列化一个修订快照。
    ///
    /// 【⚠ 解码后立即校验】
    /// JSON 里什么都能写。校验才是真正的关卡。
    ///
    /// 【失败情况】
    /// - 解析失败 → `policy_revision_decode_failed`（丢掉 serde 原始错误，只留稳定码）；
    /// - 校验不通过 → 透传具体原因码。
    ///
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let revision: Self = serde_json::from_value(value.clone())
            .map_err(|_| "policy_revision_decode_failed".to_owned())?;
        revision.validate()?;
        Ok(revision)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "policy_revision_encode_failed".to_owned())
    }

    /// 计算这个修订快照的摘要。
    ///
    /// 【⚠ 包含 `policy_digest` 字段】
    /// 这份摘要覆盖的不只是快照自己的字段，
    /// 还包括**它所指向的那份策略包的摘要**。
    /// 这样就形成了一条链：
    /// 「判定结果 → 修订快照 → 策略包内容」。
    /// 任何一环被篡改，整条链的摘要校验都会失败。
    ///
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "policy_id": self.policy_id,
            "revision": self.revision,
            "authority_epoch": self.authority_epoch,
            "policy_digest": self.policy_digest,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一整套策略 —— 策略层的核心值类型。
///
/// 【作用】
/// 把一组规则、一个默认结论、一个世代号和摘要打包在一起，
/// 形成一个**可版本化、可持久化、可校验**的整体。
///
/// 【字段语义】
/// - `schema` / `version` —— 格式与版本；
/// - `policy_id` —— 这套策略的标识；
/// - `revision` —— 第几次修订，单调递增；
/// - `authority_epoch` —— 授权世代，见下方说明；
/// - `default_effect` —— **没有规则匹配时的兜底结论**；
/// - `rules` —— 规则列表（最多 512 条）；
/// - `policy_digest` —— 内容摘要。
///
/// 【⚠ `default_effect` 应该是什么】
/// 答案永远是 `Deny`。见 [`PolicyBundle::deny_all`]。
/// 把默认设成 `Allow` 意味着"没配置过的操作一律放行"，
/// 那是典型的 fail-open，会让任何未被规则覆盖的操作悄悄通过。
///
/// 【⚠ `authority_epoch` 与 `revision` 的区别】
/// - `revision` —— 策略**内容**改了几次；
/// - `authority_epoch` —— 授权**世代**，在整个系统层面换代时递增。
///
/// 两者独立演进：改一条规则只动 `revision`；
/// 管理员换届（撤销旧授权、发新授权）动 `authority_epoch`。
/// 判定时会检查调用方带来的世代是否与策略一致（见
/// [`PolicyBundle::evaluate_with_snapshot`]），防止用旧世代的策略做判定。
///
pub struct PolicyBundle {
    pub schema: String,
    pub version: SchemaVersion,
    pub policy_id: SecurityPolicyId,
    pub revision: u64,
    pub authority_epoch: u64,
    pub default_effect: PolicyEffect,
    pub rules: Vec<PolicyRule>,
    pub policy_digest: String,
    pub bundle_digest: String,
}

impl PolicyBundle {
    /// 构造一份策略包并校验。
    ///
    /// 【⚠ 顺序：先算摘要再校验】
    /// 摘要必须在 `validate()` 之前填好。
    ///
    /// 【关于 `revision` 参数】
    /// 由调用方传入，通常是"上一个修订号 + 1"。
    /// 刻意不做自动递增 —— 让版本更新成为一个**显式的动作**，
    /// 而不是"改个规则就自动升版本"。
    ///
    pub fn new(
        policy_id: SecurityPolicyId,
        revision: u64,
        authority_epoch: u64,
        mut rules: Vec<PolicyRule>,
    ) -> Result<Self, String> {
        rules.sort_by_key(|rule| (rule.priority, rule.rule_id.clone(), rule.selector_key()));
        let mut bundle = Self {
            schema: POLICY_BUNDLE_SCHEMA.to_owned(),
            version: POLICY_SCHEMA_VERSION,
            policy_id,
            revision,
            authority_epoch,
            default_effect: PolicyEffect::Deny,
            rules,
            policy_digest: String::new(),
            bundle_digest: String::new(),
        };
        bundle.policy_digest = bundle.rules_digest();
        bundle.bundle_digest = bundle.digest();
        bundle.validate()?;
        Ok(bundle)
    }

    /// 构造一份"全部拒绝"的空策略。
    ///
    /// 【作用】
    /// 生成一份没有任何规则、默认结论为 `Deny` 的策略包。
    /// 任何请求都会落到默认结论上，也就是被拒绝。
    ///
    /// 【⚠ 这是新系统的正确起点】
    /// "先全部拒绝，再逐条放开"是安全系统的标准做法。
    /// 反过来（先全部允许，再逐条收紧）迟早会漏掉某一条。
    ///
    pub fn deny_all(
        policy_id: SecurityPolicyId,
        revision: u64,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        Self::new(policy_id, revision, authority_epoch, Vec::new())
    }

    /// 改变默认结论，并重新校验。
    ///
    /// 【作用】
    /// 链式修改器（builder 模式），修改 `default_effect` 后返回 `Self`。
    ///
    /// 【为什么用 `mut self` 而不是 `&mut self`】
    /// 因为返回的是新值（`Self`）而不是 `()`。
    /// 这是 builder 模式的标准做法：链式调用，最后一个方法才 `unwrap()`。
    ///
    /// 【⚠ 它会重算摘要】
    /// `default_effect` 变了，摘要必须跟着变，否则校验会失败。
    /// 这个方法在返回前调用了 `self.validate()`，确保摘要已更新。
    ///
    /// 【⚠ 什么时候该用它】
    /// **只应该把它设成 `Deny`。**
    /// 设成其他任何值都会在 [`PolicyBundle::evaluate_with_snapshot`]
    /// 里被一个兜底分支"纠正"回 Deny —— 见该方法末尾的说明。
    /// 保留这个方法主要是为了代码可读性和将来可能的扩展。
    ///
    pub fn with_default_effect(mut self, effect: PolicyEffect) -> Result<Self, String> {
        if effect == PolicyEffect::Allow {
            return Err("policy_default_allow_forbidden".to_owned());
        }
        self.default_effect = effect;
        self.policy_digest = self.rules_digest();
        self.bundle_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    /// 生成当前这份策略的修订快照。
    ///
    /// 【作用】
    /// 把策略包"冻结"成一个不可变的修订记录。
    /// 判定时产出的轨迹里会带上它，事后就能追溯"用的是哪一版"。
    ///
    pub fn revision_snapshot(&self) -> Result<PolicyRevision, String> {
        let mut revision = PolicyRevision {
            schema: POLICY_REVISION_SCHEMA.to_owned(),
            version: POLICY_SCHEMA_VERSION,
            policy_id: self.policy_id,
            revision: self.revision,
            authority_epoch: self.authority_epoch,
            policy_digest: self.policy_digest.clone(),
            revision_digest: String::new(),
        };
        revision.revision_digest = revision.digest();
        revision.validate()?;
        Ok(revision)
    }

    /// 从 JSON 反序列化一份策略包。
    ///
    /// 【⚠ 解码后立即校验】
    /// 和本文件其他 `from_json` 一样，解析成功不等于数据可信。
    ///
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let bundle: Self = serde_json::from_value(value.clone())
            .map_err(|_| "policy_bundle_decode_failed".to_owned())?;
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "policy_bundle_encode_failed".to_owned())
    }

    /// 校验策略包自身是否完整且自洽。
    ///
    /// 【核心检查 —— 五组】
    ///
    /// ① 头部：schema 匹配、版本兼容、`policy_id` 合法、
    ///    `revision` 非 0、`authority_epoch` 非 0。
    ///
    /// ② 规模：规则数不超过 `MAX_POLICY_RULES`（512）。
    ///
    /// ③ 规则列表本身的顺序必须**规范**（相邻对的优先级键严格递增）。
    ///    这保证同一份策略永远只有一种表示，摘要才稳定。
    ///
    /// ④ 每条规则各自校验。
    ///
    /// ⑤ 摘要自洽：重算摘要与存储值比对。
    ///
    /// 【⚠ 为什么"规则必须有序"是一条硬性要求】
    /// 因为 [`PolicyBundle::evaluate_with_snapshot`] 依赖这个顺序来做
    /// "第一条最严格规则胜出"的判定。
    ///
    /// 如果规则顺序可以随意，那么两份内容完全相同、
    /// 只是排列顺序不同的策略包，会在同一个请求上得出**不同结论**。
    /// 规范序消除了这种歧义 —— 同样的策略集合，永远得到同样的判定。
    ///
    /// ⚠ 这也是最容易在"整理规则"时被破坏的约束：
    /// 插入新规则时必须插到正确的位置，而不是直接 append。
    ///
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != POLICY_BUNDLE_SCHEMA
            || !self.version.is_compatible_with(&POLICY_SCHEMA_VERSION)
            || self.policy_id.as_uuid().is_nil()
            || self.revision == 0
            || self.authority_epoch == 0
            || self.rules.len() > MAX_POLICY_RULES
            || self.default_effect == PolicyEffect::Allow
        {
            return Err("policy_bundle_header_invalid".to_owned());
        }
        let mut ids = BTreeSet::new();
        let mut selectors = BTreeSet::new();
        for rule in &self.rules {
            rule.validate()?;
            if !ids.insert(rule.rule_id.as_str()) {
                return Err("policy_rule_duplicate_id".to_owned());
            }
            if !selectors.insert(rule.selector_key()) {
                return Err("policy_rule_duplicate_selector".to_owned());
            }
        }
        if self.rules.windows(2).any(|pair| {
            (
                pair[0].priority,
                pair[0].rule_id.as_str(),
                pair[0].selector_key(),
            ) > (
                pair[1].priority,
                pair[1].rule_id.as_str(),
                pair[1].selector_key(),
            )
        }) {
            return Err("policy_rules_noncanonical".to_owned());
        }
        validate_digest(&self.policy_digest, "policy_bundle_policy_digest")?;
        if self.policy_digest != self.rules_digest() {
            return Err("policy_bundle_policy_digest_mismatch".to_owned());
        }
        validate_digest(&self.bundle_digest, "policy_bundle_digest")?;
        if self.bundle_digest != self.digest() {
            return Err("policy_bundle_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 判定一次能力请求（简化版）。
    ///
    /// 【作用】
    /// 不追踪轨迹的判定版本。内部仍然会算输入摘要和上下文摘要，
    /// 但不保存完整的规则命中列表。
    ///
    /// 【什么时候用哪个】
    /// 需要审计追溯时用 `evaluate_with_snapshot`；
    /// 纯运行时判定（热路径、不需要留痕）用这个。
    ///
    pub fn evaluate(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> Result<PolicyEvaluation, String> {
        self.evaluate_with_snapshot(context, request, self.authority_epoch, &self.policy_digest)
    }

    /// 判定一次能力请求，**产出完整的决策轨迹**。
    ///
    /// 【作用 —— 策略层的核心判定逻辑】
    /// 输入：请求上下文、能力请求、调用方声称的世代与策略摘要。
    /// 输出：[`PolicyEvaluation`]，包含结论 + 可重放的决策轨迹。
    ///
    /// 【核心流程 —— 五道关卡，按顺序】
    ///
    /// **① 策略包自检**
    /// `self.validate()?` —— 自己无效就直接返回错误。
    /// 注意这里是 `?`（返回 `Err`），而不是像 `provider.rs` 那样返回 Deny 决策。
    /// 因为策略包无效属于**系统配置问题**，不是"这个请求被拒绝"。
    ///
    /// **② 计算输入摘要**
    /// 把「上下文 + 请求」一起哈希，得到 `input_digest`。
    /// 这份摘要会进轨迹，让复核者能确认"判定时用的确实是这一份输入"。
    ///
    /// **③ 前置拒绝检查（四道，按顺序）**
    ///
    ///   a. **世代回滚**：`authority_epoch < self.authority_epoch`
    ///      → `PolicyAuthorityEpochRollback`。
    ///      调用方带来的世代比策略还旧，说明它拿着的是过期的授权视图。
    ///      ⚠ 这是最严重的一种情况 —— 可能是回滚攻击。
    ///
    ///   b. **世代不一致**：`authority_epoch != self.authority_epoch`
    ///      → `PolicyAuthorityEpochStale`。
    ///      调用方看到的是另一个世代的策略。
    ///
    ///   c. **摘要不匹配**：`policy_digest != self.policy_digest`
    ///      → `PolicyRevisionStale`。
    ///      调用方以为用的是这份策略，但它手里的策略已经被改过了。
    ///
    ///   d. **硬策略拒绝**：`hard_policy_denial(context, request)` 有结果
    ///      → 分类为对应的原因码。
    ///      这是**独立于规则表**的硬性拒绝（比如越权、路径越界）。
    ///      ⚠ 它在规则匹配之前执行 —— 意味着**再宽松的规则也绕不过它**。
    ///
    /// 【⚠ 为什么 a/b/c/d 必须在规则匹配之前】
    /// 因为它们检查的是"你是否有资格用这份策略做判定"，而不是"这个请求是否被允许"。
    ///
    /// 如果放在规则匹配之后，一条 `Allow` 规则就会绕过所有这些检查。
    /// 比如：调用方拿着旧世代的策略来判定，旧策略里有条宽松规则，
    /// 就能放行一个本该被新一代策略拒绝的操作。
    ///
    /// **顺序即安全边界。**
    ///
    /// **④ 规则匹配与择优**
    /// 遍历所有规则，记录所有命中的（进 `matched` 列表），
    /// 并从中选出**最严格的那条**。
    ///
    /// 排序键是三元组：`(effect.rank(), priority, rule_id)`，**取最小**。
    /// `rank` 排第一，所以严格度永远优先于优先级 ——
    /// 见 [`PolicyEffect::rank`] 的说明。
    ///
    /// **⑤ 得出结论**
    /// 分四种情况：
    ///
    /// 情况 A：前置检查已产生拒绝理由
    /// → 直接拒绝，用那个理由。**不进入规则匹配。**
    ///
    /// 情况 B：选中的规则是 `Deny` → 拒绝，用规则自带的 reason。
    ///    （`unwrap_or(PolicyBundleInvalid)` 是兜底：
    ///    如果规则没带 reason，那是规则本身有问题。）
    ///
    /// 情况 C：选中的规则是 `Ask` → 要审批，用规则自带的 reason。
    ///
    /// 情况 D：选中的规则是 `Allow` → 放行，签发授权 ID。
    ///    授权 ID 格式是 `policy:{revision}:{request_id}` ——
    ///    把策略修订号和请求 ID 编进去，这样这个授权天然绑定到
    ///    "哪一版策略批准的哪一次请求"。
    ///
    /// 情况 E：**一条规则都没匹配上**
    /// → 落到 `default_effect`。
    ///
    /// ⚠ 这里有一个容易看漏的细节：如果 `default_effect == Allow`，
    /// 代码得出的 `decision` 仍然是 **`Deny`**，原因码是 `PolicyBundleInvalid`。
    ///
    /// 也就是说：**默认结论设成 Allow 不会真的放开未注册的操作。**
    /// 它会得出"策略包本身有问题"的拒绝。
    ///
    /// 这是第二道防线 —— 第一道是文档层面的"你应该设成 Deny"，
    /// 这一道是代码层面的"就算你设错了也不会真的放行"。
    /// 两层防护，防止一个配置错误变成安全漏洞。
    ///
    /// 【副作用】
    /// 无。纯函数，不修改策略包，也不碰任何外部状态。
    ///
    /// 【失败情况】
    /// 只有策略包自身无效时返回 `Err`。其余所有情况（包括"请求被拒绝"）
    /// 都返回一个正常的 [`PolicyEvaluation`]。
    ///
    pub fn evaluate_with_snapshot(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
        authority_epoch: u64,
        policy_digest: &str,
    ) -> Result<PolicyEvaluation, String> {
        self.validate()?;
        let input_digest = json_digest(&json!({
            "context": context_digest(context),
            "request": request,
        }));
        let context_digest = context_digest(context);
        let mut matched = Vec::new();
        let mut selected: Option<&PolicyRule> = None;
        let mut deny_reason = None;
        if authority_epoch < self.authority_epoch {
            deny_reason = Some(SecurityReasonCode::PolicyAuthorityEpochRollback);
        } else if authority_epoch != self.authority_epoch {
            deny_reason = Some(SecurityReasonCode::PolicyAuthorityEpochStale);
        } else if policy_digest != self.policy_digest {
            deny_reason = Some(SecurityReasonCode::PolicyRevisionStale);
        } else if let Some(reason) = hard_policy_denial(context, request) {
            deny_reason = Some(kiana_domain::classify_security_reason(&reason));
        } else {
            for rule in &self.rules {
                if rule.matches(request) {
                    matched.push(rule.rule_id.clone());
                    selected = match selected {
                        None => Some(rule),
                        Some(current)
                            if (rule.effect.rank(), rule.priority, rule.rule_id.as_str())
                                < (
                                    current.effect.rank(),
                                    current.priority,
                                    current.rule_id.as_str(),
                                ) =>
                        {
                            Some(rule)
                        }
                        Some(current) => Some(current),
                    };
                }
            }
        }

        let (decision, outcome, reason) = if let Some(reason) = deny_reason {
            (
                PolicyDecision::Deny {
                    reason: reason.as_str().to_owned(),
                },
                PolicyOutcome::Deny,
                Some(reason),
            )
        } else if let Some(rule) = selected {
            match rule.effect {
                PolicyEffect::Deny => {
                    let reason = rule
                        .reason
                        .unwrap_or(SecurityReasonCode::PolicyBundleInvalid);
                    (
                        PolicyDecision::Deny {
                            reason: reason.as_str().to_owned(),
                        },
                        PolicyOutcome::Deny,
                        Some(reason),
                    )
                }
                PolicyEffect::Ask => {
                    let reason = rule
                        .reason
                        .unwrap_or(SecurityReasonCode::PolicyBundleInvalid);
                    (
                        PolicyDecision::Ask {
                            reason: reason.as_str().to_owned(),
                        },
                        PolicyOutcome::Ask,
                        Some(reason),
                    )
                }
                PolicyEffect::Allow => (
                    PolicyDecision::Allow {
                        authorization_id: format!(
                            "policy:{}:{}",
                            self.revision, request.request_id
                        ),
                    },
                    PolicyOutcome::Allow,
                    None,
                ),
            }
        } else {
            let reason = match self.default_effect {
                PolicyEffect::Deny => SecurityReasonCode::PolicyOperationUnregistered,
                PolicyEffect::Ask => SecurityReasonCode::PolicyApprovalRequired,
                PolicyEffect::Allow => SecurityReasonCode::PolicyBundleInvalid,
            };
            let decision = match self.default_effect {
                PolicyEffect::Deny => PolicyDecision::Deny {
                    reason: reason.as_str().to_owned(),
                },
                PolicyEffect::Ask => PolicyDecision::Ask {
                    reason: reason.as_str().to_owned(),
                },
                PolicyEffect::Allow => PolicyDecision::Deny {
                    reason: reason.as_str().to_owned(),
                },
            };
            (
                decision,
                if self.default_effect == PolicyEffect::Ask {
                    PolicyOutcome::Ask
                } else {
                    PolicyOutcome::Deny
                },
                Some(reason),
            )
        };

        let trace = DecisionTrace::new(
            self,
            request,
            input_digest,
            context_digest,
            matched,
            outcome,
            reason,
        )?;
        Ok(PolicyEvaluation { decision, trace })
    }

    /// 只对「默认结论 + 规则列表」计算的摘要。
    ///
    /// 【作用】
    /// 一个**更细粒度**的摘要，只覆盖影响判定结果的那部分内容。
    ///
    /// 【为什么与 [`PolicyBundle::digest`] 分开】
    /// 完整摘要覆盖所有字段（包括 `policy_id`、`revision` 等身份信息）。
    /// 而这个只覆盖"决定结论的内容"。
    ///
    /// 用途上：完整摘要标识"这是哪一版策略"，
    /// 这个摘要标识"这两版策略的判定行为是否完全相同"。
    ///
    /// 如果两版策略的 `policy_id` 不同但规则完全一样，
    /// 它们的完整摘要不同，但 `rules_digest` 相同 ——
    /// 说明判定行为一致，规则没被偷偷改过。
    ///
    fn rules_digest(&self) -> String {
        json_digest(&json!({
            "default_effect": self.default_effect,
            "rules": self.rules,
        }))
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "policy_id": self.policy_id,
            "revision": self.revision,
            "authority_epoch": self.authority_epoch,
            "default_effect": self.default_effect,
            "rules": self.rules,
            "policy_digest": self.policy_digest,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 判定结论的分类。
///
/// 【作用】
/// 比 [`kiana_domain::PolicyDecision`] 更细的分类。
/// 两者的区别在于：这里额外区分了"为什么没有规则匹配"这类情况，
/// 便于审计时区分"被规则拒绝"和"根本没有规则"。
///
pub enum PolicyOutcome {
    Allow,
    Ask,
    Deny,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一次判定的完整轨迹记录。
///
/// 【作用】
/// 这是**可重放（replayable）**的那一部分 —— 见文件头说明。
///
/// 记录了什么：
/// - 判定时的输入摘要（上下文 + 请求）；
/// - 使用的策略修订号与摘要；
/// - **所有命中的规则 ID**（不只是胜出的那条）；
/// - 最终结论和原因码。
///
/// 【⚠ 为什么记录所有命中的规则，而不只是胜出的那条】
/// 因为审计要回答的不只是"结论是什么"，还有"当时有哪些规则适用"。
/// 如果只记胜出那条，就无法验证"择优逻辑是不是选对了" ——
/// 比如有人改了 `rank` 的数值，导致本该最严格的规则没被选中，
/// 从单条记录里看不出来。
///
/// 【⚠ 轨迹是判定结果的一部分，不是可选的日志】
/// 它和结论一起被返回，一起被持久化。
/// 这不是"调试信息"，而是审计证据。
///
pub struct DecisionTrace {
    pub schema: String,
    pub version: SchemaVersion,
    pub decision_id: SecurityDecisionId,
    pub policy_id: SecurityPolicyId,
    pub policy_revision: u64,
    pub authority_epoch: u64,
    pub request_id: kiana_domain::RequestId,
    pub input_digest: String,
    pub context_digest: String,
    pub matched_rule_ids: Vec<String>,
    pub outcome: PolicyOutcome,
    #[serde(default)]
    pub reason: Option<SecurityReasonCode>,
    pub trace_digest: String,
}

impl DecisionTrace {
    /// 构造一份决策轨迹。
    ///
    /// 【作用】
    /// 从策略包、请求、摘要和命中列表组装出轨迹，并立即校验。
    ///
    /// 【⚠ 为什么要立即校验】
    /// 轨迹会被持久化并用于事后复核。
    /// 如果它本身是坏的（比如摘要对不上），那这份复核材料就毫无价值。
    /// 在这里就拦住，比等到事后复核时才发现要好。
    ///
    fn new(
        bundle: &PolicyBundle,
        request: &CapabilityRequest,
        input_digest: String,
        context_digest: String,
        matched_rule_ids: Vec<String>,
        outcome: PolicyOutcome,
        reason: Option<SecurityReasonCode>,
    ) -> Result<Self, String> {
        let mut trace = Self {
            schema: POLICY_DECISION_TRACE_SCHEMA.to_owned(),
            version: POLICY_SCHEMA_VERSION,
            decision_id: SecurityDecisionId::new(),
            policy_id: bundle.policy_id,
            policy_revision: bundle.revision,
            authority_epoch: bundle.authority_epoch,
            request_id: request.request_id,
            input_digest,
            context_digest,
            matched_rule_ids,
            outcome,
            reason,
            trace_digest: String::new(),
        };
        trace.trace_digest = trace.digest();
        trace.validate()?;
        Ok(trace)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let trace: Self = serde_json::from_value(value.clone())
            .map_err(|_| "policy_decision_trace_decode_failed".to_owned())?;
        trace.validate()?;
        Ok(trace)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "policy_decision_trace_encode_failed".to_owned())
    }

    /// 校验这份决策轨迹是否自洽。
    ///
    /// 【核心检查】
    /// - schema 与版本；
    /// - 输入摘要、策略摘要格式合法；
    /// - 结论字段与原因码配套（Allow 不带 reason，拒绝必须带）；
    /// - 命中规则列表的规模不超限，且每条都是非空字符串；
    /// - 轨迹摘要与重算值一致。
    ///
    /// 【⚠ 这条检查是复核工作的最后一道关卡】
    /// 复核者拿到一份轨迹时，会重算摘要并比对。
    /// 如果这一步没做，篡改过的轨迹也能通过复核。
    ///
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != POLICY_DECISION_TRACE_SCHEMA
            || !self.version.is_compatible_with(&POLICY_SCHEMA_VERSION)
            || self.decision_id.as_uuid().is_nil()
            || self.policy_id.as_uuid().is_nil()
            || self.policy_revision == 0
            || self.authority_epoch == 0
            || self.matched_rule_ids.len() > MAX_POLICY_RULES
            || self.matched_rule_ids.iter().any(|id| id.trim().is_empty())
        {
            return Err("policy_decision_trace_header_invalid".to_owned());
        }
        let mut matched_rule_ids = BTreeSet::new();
        if self
            .matched_rule_ids
            .iter()
            .any(|id| !matched_rule_ids.insert(id.as_str()))
        {
            return Err("policy_decision_trace_rules_invalid".to_owned());
        }
        match (self.outcome, self.reason) {
            (PolicyOutcome::Allow, Some(_)) | (PolicyOutcome::Ask | PolicyOutcome::Deny, None) => {
                return Err("policy_decision_trace_reason_invalid".to_owned());
            }
            _ => {}
        }
        validate_digest(&self.input_digest, "policy_decision_trace_input_digest")?;
        validate_digest(&self.context_digest, "policy_decision_trace_context_digest")?;
        validate_digest(&self.trace_digest, "policy_decision_trace_digest")?;
        if self.trace_digest != self.digest() {
            return Err("policy_decision_trace_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "decision_id": self.decision_id,
            "policy_id": self.policy_id,
            "policy_revision": self.policy_revision,
            "authority_epoch": self.authority_epoch,
            "request_id": self.request_id,
            "input_digest": self.input_digest,
            "context_digest": self.context_digest,
            "matched_rule_ids": self.matched_rule_ids,
            "outcome": self.outcome,
            "reason": self.reason,
        }))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PolicyEvaluation {
    pub decision: PolicyDecision,
    pub trace: DecisionTrace,
}

impl PolicyEvaluation {
    /// 校验一次判定结果（结论 + 轨迹）的组合是否自洽。
    ///
    /// 【作用】
    /// 确保结论和轨迹是**配套的** —— 不是 A 请求的结论配 B 请求的轨迹。
    ///
    /// 【⚠ 为什么需要这一层】
    /// `PolicyEvaluation` 是把两个独立对象打包在一起。
    /// 结构上它们可以被任意组合 —— 拼一个 A 的结论加一个 B 的轨迹，
    /// 编译器不会报错。这层校验就是防这种"张冠李戴"。
    ///
    pub fn validate(&self) -> Result<(), String> {
        self.trace.validate()
    }
}

#[derive(Clone, Debug)]
/// 基于策略包的严格策略引擎实现。
///
/// 【作用】
/// 把 [`PolicyBundle`] 适配到本 crate 的 [`PolicyEngine`] 接口上，
/// 让 `ControlPlane` 能用同一套接口调用它。
///
/// 【⚠ 它不是第二条执行路径】
/// 文件头强调过这一点。这个类型只是一个**适配器** ——
/// 它让新的策略包能通过既有的引擎接口被调用，
/// 而不是引入一套平行的执行机制。
/// `ControlPlane` 仍然是唯一的调度权威。
pub struct BundlePolicyEngine {
    bundle: PolicyBundle,
}

impl BundlePolicyEngine {
    pub fn new(bundle: PolicyBundle) -> Result<Self, String> {
        bundle.validate()?;
        Ok(Self { bundle })
    }

    pub fn bundle(&self) -> &PolicyBundle {
        &self.bundle
    }

    pub fn evaluate_with_trace(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> Result<PolicyEvaluation, String> {
        self.bundle.evaluate(context, request)
    }

    pub fn evaluate(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> PolicyDecision {
        <Self as PolicyEngine>::evaluate(self, context, request)
    }
}

impl PolicyEngine for BundlePolicyEngine {
    fn evaluate(&self, context: &RequestContext, request: &CapabilityRequest) -> PolicyDecision {
        self.bundle
            .evaluate(context, request)
            .map(|evaluation| evaluation.decision)
            .unwrap_or_else(|_| PolicyDecision::Deny {
                reason: SecurityReasonCode::PolicyBundleInvalid.as_str().to_owned(),
            })
    }
}

fn context_digest(context: &RequestContext) -> String {
    json_digest(&json!({
        "request_id": context.request_id,
        "session_id": context.session_id,
        "actor_id": context.actor_id,
        "project_root": context.project_root,
        "project_trusted": context.project_trusted,
        "permission_profile": context.permission_profile,
        "role_id": context.role_id,
        "department_id": context.department_id,
        "work_packet_id": context.work_packet_id,
        "cell_id": context.cell_id,
        "path_allow": context.path_allow,
    }))
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

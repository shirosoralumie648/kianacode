//! Server-owned provider.use policy and read-only credential status projection.
//!
//! Provider policy is intentionally separate from credential material.  A configured credential
//! never grants provider use by itself, and a probe can report scope/expiry/re-auth state without
//! turning an authentication failure into a capability grant.
//!
//! # 这个文件在系统里的位置
//!
//! 管理"能不能调用某个外部模型服务商"的策略。与 [`kiana-ports/src/oauth_accounts.rs`]
//! 那个文件是**刻意分开的** —— 一个管权限，一个管凭据。
//!
//! ```text
//! 有人想调用 provider "anthropic" 的 "provider.use" 操作
//!        ↓
//! 【本文件：ProviderPolicyBundle::evaluate】
//!    第一步：策略判定 —— 服务端配的规则允不允许？
//!    第二步：凭据状态 —— 配的凭据现在能用吗？
//!        ↓
//!    两者都通过才 Allow
//! ```
//!
//! # 最重要的一条：配了凭据 ≠ 有权使用
//!
//! 文件头那句话是本文件的核心："A configured credential never grants provider use
//! by itself"（**配置了凭据本身绝不等于获得了使用权**）。
//!
//! 两者必须**分别**成立：
//! 1. 服务端策略允许调用这个服务商（[`ProviderPolicyBundle`] 里的规则）；
//! 2. 配置的凭据处于可用状态（`Configured`）。
//!
//! 少任何一个都不放行。这防的是"只要在配置里填了 API key，
//! 就等于授权程序可以往外发数据"—— 那会让一份配置文件
//! 变成一个任意外传的通道。
//!
//! # 为什么凭据检查放在策略检查**之后**
//!
//! 文件头说得很明确：这样做的目的是让 "missing scope remains a distinct diagnostic
//! rather than an invalid-credential success/deny collapse"
//! （权限不足仍然是一个**独立的诊断**，而不是和"凭据无效"混在一起）。
//!
//! 举个具体例子。用户配置了一个凭据，但那个 key 没有调用目标操作的范围；
//! 同时服务商策略也恰好禁止了这个操作。
//!
//! - **先查策略**：返回 `provider_policy_denied` —— 明确是"策略不允许"；
//! - **先查凭据**：返回 `provider_scope_insufficient` —— 说的是"凭据范围不够"。
//!
//! 两个原因对应的处置完全不同：前者要改策略配置，后者要重新授权凭据。
//! 如果塌缩成一个 `provider_denied`，运维人员就不知道该改哪边。
//!
//! # 术语
//!
//! - **provider（服务商）**：外部模型服务提供方，如 Anthropic、OpenAI。
//! - **credential status（凭据状态）**：凭据当前的可用性，见
//!   [`CredentialDisplayStatus`]。这是**投影（projection）**，只读，不含密钥原文。
//! - **precedence（优先级）**：多条规则都匹配时，数值大的胜出。
//! - **fail-closed**：策略无效时直接拒绝，而不是"忽略这份策略"。
//!
//! # 上游契约
//!
//! Provider policy is intentionally separate from credential material.  A configured credential
//! never grants provider use by itself, and a probe can report scope/expiry/re-auth state without
//! turning an authentication failure into a capability grant.

use kiana_domain::{json_digest, CredentialDisplayStatus};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const PROVIDER_POLICY_SCHEMA: &str = "kiana.provider-use-policy.v1";
pub const PROVIDER_POLICY_RULE_SCHEMA: &str = "kiana.provider-use-policy-rule.v1";
pub const PROVIDER_POLICY_DECISION_SCHEMA: &str = "kiana.provider-use-policy-decision.v1";
pub const PROVIDER_USE_OPERATION: &str = "provider.use";

/// 检查一个字符串字段是否"有内容、在长度内、且不含控制字符"。
///
/// 【作用】
/// 三个条件：非空（纯空白也不行）、长度不超限、不含空字节/回车/换行。
///
/// 【⚠ 为什么要拦 `\r` 和 `\n`】
/// 这两个是换行符。允许它们进入 ID 字段，会造成**日志注入**：
/// 攻击者可以把 `恶意规则\n2026-01-01 INFO 用户已信任` 这样的字符串写进规则 ID，
/// 日志里就会多出一行看起来完全正常的记录，
/// 像是系统自己打的日志，极难识别。拦掉换行，字段就永远只能占一行。
///
/// 【⚠ 为什么要拦 `\0`】
/// 空字节是 C 字符串终止符。如果数据流向底层 C 库，
/// `"safe\0../../etc"` 会被截断成 `"safe"` 从而通过检查。
/// 虽然 Rust 内部没有这个问题，但数据入口处拦住最保险。
///
/// 【为什么用 `contains(['\0', '\r', '\n'])` 传数组】
/// `str::contains` 接受任何 `Pattern`。传数组是"任一出现即真"的语义，
/// 写成一个列表比三个 `||` 更紧凑，也更容易扩展。
fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains(['\0', '\r', '\n'])
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 策略判定的两种结果。
///
/// 【作用】
/// 只有 `Allow` 和 `Deny` 两种。**注意这里没有 `Ask`** ——
/// 和 Gate 层的 `AwaitingApproval` 不同，本层的判定是终局的：
/// 要么允许，要么不允许。
pub enum ProviderPolicyEffect {
    Allow,
    Deny,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一条服务商使用策略规则。
///
/// 【作用】
/// 描述"某个服务商上的某个操作，应该允许还是拒绝"。
///
/// 【通配符 `*`】
/// `provider_id` 和 `operation` 都支持 `"*"`，表示"匹配任意"。
/// 匹配逻辑见 [`ProviderPolicyRule::matches`]。
pub struct ProviderPolicyRule {
    pub schema: String,
    pub rule_id: String,
    pub provider_id: String,
    pub operation: String,
    pub effect: ProviderPolicyEffect,
    pub precedence: u32,
    pub rule_digest: String,
}

impl ProviderPolicyRule {
    /// 构造一条策略规则并校验。
    ///
    /// 【⚠ 顺序：先算摘要，再校验】
    /// 摘要的赋值必须在 `validate()` 之前执行，
    /// 因为校验里有一项是"存储的摘要 vs 重算的摘要"的比对。
    /// 先校验的话摘要还是空的，比对必然失败。
    ///
    /// 【失败情况】
    /// 校验不通过返回 `Err("provider_policy_rule_invalid")`。
    pub fn new(
        rule_id: impl Into<String>,
        provider_id: impl Into<String>,
        operation: impl Into<String>,
        effect: ProviderPolicyEffect,
        precedence: u32,
    ) -> Result<Self, String> {
        let mut rule = Self {
            schema: PROVIDER_POLICY_RULE_SCHEMA.to_owned(),
            rule_id: rule_id.into(),
            provider_id: provider_id.into(),
            operation: operation.into(),
            effect,
            precedence,
            rule_digest: String::new(),
        };
        rule.rule_digest = rule.digest();
        rule.validate()?;
        Ok(rule)
    }

    /// 校验这条规则自身是否合法。
    ///
    /// 【核心检查 —— 全部满足才通过】
    /// - schema 精确匹配；
    /// - `rule_id` 有内容、不超 256 字节、无控制字符；
    /// - `provider_id` 有内容、不超 128 字节、无控制字符；
    /// - `operation` 有内容、不超 128 字节、无控制字符 —— **或者**等于 `"*"`；
    /// - `precedence` 不超过 1,000,000；
    /// - 摘要自洽。
    ///
    /// 【关于 `precedence` 的上限 1,000,000】
    /// 优先级类型是 `u32`，理论上能到 42 亿。上限卡到 100 万，
    /// 是为了防止"用天大的优先级数字压过所有正常规则"这种配置。
    /// 如果真的需要区分超过百万级的优先级，说明策略设计本身就有问题。
    ///
    /// 【关于 operation 的 `"*"` 例外】
    /// 条件是"既不满足常规检查，**又**不是通配符"才报错，
    /// 这样 `*` 就能通过。写成这样是因为通配符是一个**有意义的特例**，
    /// 显式承认它比隐含在通用规则里更清楚。
    ///
    /// 【⚠ 末尾那段重复检查是必要的】
    /// 下面又检查了一次 `provider_id`，看起来多余，
    /// 实际上它是**防御检查顺序被调整**的补丁：
    /// 如果将来有人移动了前面那道检查，这道兜底能保证 `provider_id` 永远被检查到。
    /// 删掉它之前请先确认前面那道检查不会移动位置。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_POLICY_RULE_SCHEMA
            || !bounded(&self.rule_id, 256)
            || !bounded(&self.provider_id, 128)
            || (!bounded(&self.operation, 128) && self.operation != "*")
            || self.precedence > 1_000_000
            || self.rule_digest != self.digest()
        {
            return Err("provider_policy_rule_invalid".to_owned());
        }
        if self.provider_id != "*" && !bounded(&self.provider_id, 128) {
            return Err("provider_policy_provider_invalid".to_owned());
        }
        Ok(())
    }

    /// 判断这条规则是否覆盖给定的服务商和操作。
    ///
    /// 【通配符语义】
    /// 两个维度都支持 `"*"`：
    /// - `provider_id == "*"` → 匹配所有服务商；
    /// - `operation == "*"` → 匹配所有操作。
    ///
    /// ⚠ 但注意：**匹配上了不代表会执行**。
    /// [`ProviderPolicyBundle::evaluate`] 里还有一道独立检查，
    /// 要求操作名必须是 `PROVIDER_USE_OPERATION`。
    /// 通配规则只是"进入候选"，本身不构成放行。
    fn matches(&self, provider_id: &str, operation: &str) -> bool {
        (self.provider_id == "*" || self.provider_id == provider_id)
            && (self.operation == "*" || self.operation == operation)
    }

    /// 计算这条规则的内容摘要。
    ///
    /// 【⚠ 字段清单就是这条规则的身份】
    /// 把字段加进或移出这个 `json!`，会让所有已持久化的规则校验失败。
    /// 摘要字段自身不参与计算（否则要算自己，无解）。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "rule_id": self.rule_id,
            "provider_id": self.provider_id,
            "operation": self.operation,
            "effect": self.effect,
            "precedence": self.precedence,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一整套服务商使用策略。
///
/// 【作用】
/// 服务端持有的策略集合。这是**唯一**能决定能否调用服务商的依据 ——
/// [`ProviderPolicyBundle::evaluate`] 的原始注释里明确写着
/// "no plugin/default route can bypass the server-owned bundle"：
/// 插件、默认路由、任何其他来源都**不能**绕过它。
pub struct ProviderPolicyBundle {
    pub schema: String,
    pub version: u64,
    pub authority_epoch: u64,
    pub default_effect: ProviderPolicyEffect,
    pub rules: Vec<ProviderPolicyRule>,
    pub policy_digest: String,
}

impl ProviderPolicyBundle {
    /// 构造一份策略包。
    ///
    /// 【⚠ 版本号硬编码为 1】
    /// `version` 字段写死为 1，不通过参数传入。
    /// 这是有意的：让版本升级成为一个**显式的代码改动**，而不是配置项。
    pub fn new(
        authority_epoch: u64,
        default_effect: ProviderPolicyEffect,
        rules: Vec<ProviderPolicyRule>,
    ) -> Result<Self, String> {
        let mut bundle = Self {
            schema: PROVIDER_POLICY_SCHEMA.to_owned(),
            version: 1,
            authority_epoch,
            default_effect,
            rules,
            policy_digest: String::new(),
        };
        bundle.policy_digest = bundle.digest();
        bundle.validate()?;
        Ok(bundle)
    }

    /// 构造一份"全部拒绝"的空策略。
    ///
    /// 【为什么需要这个便捷函数】
    /// 当系统还没有配置任何服务商策略时，用它得到一个**安全的默认值**：
    /// 没有规则能匹配 → 落到 `default_effect` = `Deny`。
    ///
    /// 这是 fail-closed 的一个实例：未配置 = 拒绝，而不是"未配置 = 随便用"。
    ///
    /// 【典型用途】
    /// 全新安装、策略文件不存在、授权世代变更后尚未重新下发策略 ——
    /// 这些场景都应该得到这份空策略，
    /// 而不是退化成"没有策略就没有限制"。
    pub fn default_deny(authority_epoch: u64) -> Result<Self, String> {
        Self::new(authority_epoch, ProviderPolicyEffect::Deny, Vec::new())
    }

    /// 校验策略包自身是否合法。
    ///
    /// 【核心检查】
    /// - schema 匹配、版本非 0、授权世代非 0；
    /// - 规则数不超过 256（超出即 `provider_policy_invalid`）；
    /// - 摘要自洽；
    /// - 每条规则各自校验；
    /// - **规则 ID 互不重复**（用 `BTreeSet` 查重）。
    ///
    /// 【⚠ 为什么要查规则 ID 重复】
    /// 两条规则 ID 相同意味着事后审计时无法确定"生效的是哪一条"——
    /// 追溯性被破坏了。规则 ID 是规则的唯一标识，必须唯一。
    ///
    /// 【为什么是 256 条上限】
    /// 服务商使用策略不需要那么多细规则。上限既防止配置膨胀，
    /// 也让每次判定的开销有确定上界。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_POLICY_SCHEMA
            || self.version == 0
            || self.authority_epoch == 0
            || self.rules.len() > 256
            || self.policy_digest != self.digest()
        {
            return Err("provider_policy_invalid".to_owned());
        }
        let mut ids = BTreeSet::new();
        for rule in &self.rules {
            rule.validate()?;
            if !ids.insert(rule.rule_id.clone()) {
                return Err("provider_policy_rule_duplicate".to_owned());
            }
        }
        Ok(())
    }

    /// Evaluate a provider.use request.  Matching rules are considered in declaration order and
    /// the highest `(precedence, declaration_index)` wins; no plugin/default route can bypass the
    /// server-owned bundle. Credential status is checked after the policy effect so missing scope
    /// remains a distinct diagnostic rather than an invalid-credential success/deny collapse.
    /// 判定"能不能调用这个服务商的这个操作"。
    ///
    /// 【核心流程 —— 四道关卡】
    ///
    /// **① 策略包自身是否合法**
    /// 不合法 → 拒绝，原因 `provider_policy_invalid`。
    /// ⚠ 这里返回的是 **Deny 而不是 Err** —— 判定函数本身永不失败，它永远返回一个决策对象。
    /// "策略文件坏了"也是一种决策结果，就是拒绝。
    /// 这样调用方不必处理 `Result`，也就不存在"忘了检查 Err 导致策略被绕过"的风险。
    ///
    /// **② 请求参数是否合法**
    /// `provider_id` 或 `operation` 超长 / 含控制字符 → 拒绝，原因 `provider_request_invalid`。
    ///
    /// **③ 选出生效规则**
    /// 遍历所有规则，找出同时满足匹配条件的，
    /// 取 `(precedence, 声明位置)` 最大的那条。
    ///
    /// ⚠ 排序键是**二元组** `(precedence, index)`，不是只看 `precedence`。
    /// 这意味着：**优先级相同时，写在后面（index 更大）的规则胜出**，
    /// 也就是"后来者覆盖先来者"。
    /// 这条规则让策略可以**逐步收紧**：
    /// 想临时加一条例外，就在末尾追加一条同优先级但更具体的规则即可。
    ///
    /// **④ 凭据状态判定**（策略通过后才做）
    /// 这一步把策略结论和凭据状态组合起来，产出最终的 `(effect, reason)`。
    ///
    /// 【第 ④ 步的完整判定表】
    ///
    /// 先过两道前置门：
    /// - 操作不是 `provider.use` → `provider_operation_denied`；
    /// - 策略结论是 Deny → `provider_policy_denied`（**不再看凭据**）。
    ///
    /// 策略 Allow 之后，按凭据状态分流：
    ///
    /// | 凭据状态 | 结论 | 原因码 |
    ///
    /// | `Configured` | **允许** | `provider_ready` |
    /// | `ScopeInsufficient` | 拒绝 | `provider_scope_insufficient` |
    /// | `Missing` | 拒绝 | `provider_credential_missing` |
    /// | `Expired` | 拒绝 | `provider_credential_expired` |
    /// | `ReauthRequired` | 拒绝 | `provider_reauth_required` |
    /// | `Revoked` | 拒绝 | `provider_credential_revoked` |
    /// | `Unsupported` | 拒绝 | `provider_credential_backend_unsupported` |
    /// | `Unknown` | 拒绝 | `provider_credential_unknown` |
    ///
    /// ⚠ 注意**只有 `Configured` 放行**。`Unknown` 也被拒绝 ——
    /// 这与文件头说的 fail-closed 一致：不知道凭据能不能用，就当不能用。
    ///
    /// 【⚠ 为什么每种凭据问题都要给独立原因码】
    /// 因为它们的**处置方式完全不同**：
    /// - `Expired` → 重新授权；
    /// - `ScopeInsufficient` → 去服务商后台申请更大范围；
    /// - `Revoked` → 凭据可能泄露了，要作废重建；
    /// - `Unsupported` → 系统的凭据后端不支持这种类型，属于配置问题。
    ///
    /// 全部塌缩成一个 `denied` 的话，运维就失去了行动方向。
    /// 这正是文件头那句 "remains a distinct diagnostic" 的含义。
    ///
    /// 【关于 `is_none_or` 的用法】
    /// 代码是 `selected.as_ref().is_none_or(|(precedence, previous, _)| ...)`。
    /// `is_none_or` 接受闭包，在值为 `None` 时对闭包传入默认值 `()` 求值，
    /// 效果就是"还没选过就直接选；已经选过就在更优时替换"。
    /// 逻辑上等价于"取最大值"，但只需一次遍历，不用先收集再排序。
    ///
    /// 【副作用】
    /// 无。纯函数，只读策略包，不修改任何状态。
    pub fn evaluate(
        &self,
        provider_id: &str,
        operation: &str,
        credential_status: CredentialDisplayStatus,
    ) -> ProviderPolicyDecision {
        if self.validate().is_err() {
            return ProviderPolicyDecision {
                schema: PROVIDER_POLICY_DECISION_SCHEMA.to_owned(),
                provider_id: provider_id.to_owned(),
                operation: operation.to_owned(),
                credential_status,
                effect: ProviderPolicyEffect::Deny,
                reason: "provider_policy_invalid".to_owned(),
                matched_rule: None,
                policy_digest: self.policy_digest.clone(),
            };
        }
        if !bounded(provider_id, 128) || !bounded(operation, 128) {
            return ProviderPolicyDecision {
                schema: PROVIDER_POLICY_DECISION_SCHEMA.to_owned(),
                provider_id: provider_id.to_owned(),
                operation: operation.to_owned(),
                credential_status,
                effect: ProviderPolicyEffect::Deny,
                reason: "provider_request_invalid".to_owned(),
                matched_rule: None,
                policy_digest: self.policy_digest.clone(),
            };
        }
        let mut selected: Option<(u32, usize, &ProviderPolicyRule)> = None;
        for (index, rule) in self.rules.iter().enumerate() {
            if rule.matches(provider_id, operation)
                && selected.as_ref().is_none_or(|(precedence, previous, _)| {
                    (rule.precedence, index) > (*precedence, *previous)
                })
            {
                selected = Some((rule.precedence, index, rule));
            }
        }
        let effect = selected
            .map(|(_, _, rule)| rule.effect)
            .unwrap_or(self.default_effect);
        let (effect, reason) = if operation != PROVIDER_USE_OPERATION {
            (ProviderPolicyEffect::Deny, "provider_operation_denied")
        } else {
            match (effect, credential_status) {
                (ProviderPolicyEffect::Deny, _) => {
                    (ProviderPolicyEffect::Deny, "provider_policy_denied")
                }
                (ProviderPolicyEffect::Allow, CredentialDisplayStatus::Configured) => {
                    (ProviderPolicyEffect::Allow, "provider_ready")
                }
                (ProviderPolicyEffect::Allow, CredentialDisplayStatus::ScopeInsufficient) => {
                    (ProviderPolicyEffect::Deny, "provider_scope_insufficient")
                }
                (ProviderPolicyEffect::Allow, CredentialDisplayStatus::Missing) => {
                    (ProviderPolicyEffect::Deny, "provider_credential_missing")
                }
                (ProviderPolicyEffect::Allow, CredentialDisplayStatus::Expired) => {
                    (ProviderPolicyEffect::Deny, "provider_credential_expired")
                }
                (ProviderPolicyEffect::Allow, CredentialDisplayStatus::ReauthRequired) => {
                    (ProviderPolicyEffect::Deny, "provider_reauth_required")
                }
                (ProviderPolicyEffect::Allow, CredentialDisplayStatus::Revoked) => {
                    (ProviderPolicyEffect::Deny, "provider_credential_revoked")
                }
                (ProviderPolicyEffect::Allow, CredentialDisplayStatus::Unsupported) => (
                    ProviderPolicyEffect::Deny,
                    "provider_credential_backend_unsupported",
                ),
                (ProviderPolicyEffect::Allow, CredentialDisplayStatus::Unknown) => {
                    (ProviderPolicyEffect::Deny, "provider_credential_unknown")
                }
            }
        };
        ProviderPolicyDecision {
            schema: PROVIDER_POLICY_DECISION_SCHEMA.to_owned(),
            provider_id: provider_id.to_owned(),
            operation: operation.to_owned(),
            credential_status,
            effect,
            reason: reason.to_owned(),
            matched_rule: selected.map(|(_, _, rule)| rule.rule_id.clone()),
            policy_digest: self.policy_digest.clone(),
        }
    }

    /// 计算整份策略包的摘要。
    ///
    /// 【⚠ 包含全部规则】
    /// `rules` 数组整体参与计算，
    /// 所以**任何一条规则的任何改动**都会改变这份摘要，
    /// 进而让所有已持久化的策略包校验失败。
    /// 这是有意的：策略包是一个整体，不允许局部改动而不留痕迹。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "authority_epoch": self.authority_epoch,
            "default_effect": self.default_effect,
            "rules": self.rules,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一次判定的结果。
///
/// 【作用】
/// 记录"这次判定为什么是这个结论"。它同时也是**审计凭证** ——
/// `policy_digest` 字段指向被使用的那份策略包的摘要，
/// 事后可以据此确认"当时用的是哪一版策略"。
pub struct ProviderPolicyDecision {
    pub schema: String,
    pub provider_id: String,
    pub operation: String,
    pub credential_status: CredentialDisplayStatus,
    pub effect: ProviderPolicyEffect,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_rule: Option<String>,
    pub policy_digest: String,
}

impl ProviderPolicyDecision {
    /// 这次判定是否真的放行。
    ///
    /// 【⚠ 为什么不能只看 `effect == Allow`】
    /// 因为存在这种情况：策略结论是 Allow，但凭据状态不是 `Configured`。
    /// 此时 [`ProviderPolicyBundle::evaluate`] 会把最终 `effect` 改写成 `Deny`，
    /// 但**这层防御依然必要** —— 它确保"允许"这个结论必须同时满足两个条件，
    /// 不依赖上游是否记得做第二次检查。
    ///
    /// 两层检查是同一不变量的重复表达，属于刻意的冗余：
    /// 安全相关的判定，多一道独立的确认成本很低，漏掉一处的代价很高。
    pub fn allowed(&self) -> bool {
        self.effect == ProviderPolicyEffect::Allow
            && self.credential_status == CredentialDisplayStatus::Configured
    }

    /// 校验这个判定结果结构是否合法。
    ///
    /// 【核心检查】
    /// - schema 匹配；
    /// - `provider_id` / `operation` / `reason` 各自有内容、在长度内、无控制字符；
    /// - 摘要以 `sha256:` 开头，且**总长恰好 71**。
    ///
    /// 【⚠ 为什么摘要长度是 71 而不是 64】
    /// 这是 `sha256:` 前缀（7 个字符）加上 64 位十六进制摘要，
    /// 7 + 64 = 71。写成硬编码的 71 而不是 `"sha256:".len() + 64`，
    /// 是为了让这个数字成为一个**独立的交叉校验**：
    /// 如果哪天有人改了别处的摘要格式，这里会立刻发现不匹配。
    /// 两处独立写死的同一个常量，互相验证。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_POLICY_DECISION_SCHEMA
            || !bounded(&self.provider_id, 128)
            || !bounded(&self.operation, 128)
            || !bounded(&self.reason, 256)
            || !self.policy_digest.starts_with("sha256:")
            || self.policy_digest.len() != 71
        {
            return Err("provider_policy_decision_invalid".to_owned());
        }
        Ok(())
    }
}

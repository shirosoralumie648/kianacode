//! Monotonic capability grant scopes for parent/child Cell inheritance.
//!
//! A `GrantScope` is a policy value, not an authorization token. Child scopes are formed only by
//! intersecting server-owned layers; capability, secret and external-effect dimensions are
//! independent so a broad read scope cannot accidentally become a write/network/secret grant.
//!
//! # 这个文件在系统里的位置
//!
//! Kiana 里"谁能让谁做什么"由一条**单向收缩**的链条决定。本文件是那条链的核心。
//!
//! ```text
//! 项目模板（scope）
//!        ↓ intersect（取交集，只能更小）
//! 部门角色（scope）
//!        ↓ intersect
//! 工作包 / WorkPacket（scope）
//!        ↓ intersect
//! 审批 Approval（scope）
//!        ↓ intersect
//!    子 Cell（scope）        ← 交集的结果，永远不大于任何一层
//! ```
//!
//! # 最重要的一条不变量：权限只能缩小，绝不能扩大
//!
//! 文件头那句 "Child scopes are formed only by intersecting server-owned layers"
//! （子作用域只能由服务端持有的层取交集产生）是本文件的全部意义所在。
//!
//! **任何一次派生，结果都不可能比父级更大。** 这不是靠约定，是靠
//! [`GrantScope::intersect`] 的实现方式强制的 —— 它用的是交集运算：
//!
//! - 能力（capabilities）：两边**都有**才保留
//! - 密钥访问（allow_secret）：两边**都允许**才允许
//! - 外部副作用（allow_external）：两边**都允许**才允许
//! - 委派（delegation_allowed）：两边**都允许**才允许
//! - 过期时间：取**较早**的那个
//!
//! ⚠ 这意味着：如果你想给子级更多权限，**正确做法不是在这里放宽**，
//! 而是去修改某一层的上游来源。交集运算本身没有"放宽"这个操作。
//!
//! # 为什么三个危险维度要独立
//!
//! 文件头说 "capability, secret and external-effect dimensions are independent
//! so a broad read scope cannot accidentally become a write/network/secret grant"
//! （能力、密钥、外部副作用三个维度相互独立，
//! 所以一个宽泛的读作用域不会意外变成写/网络/密钥授权）。
//!
//! 举例说明为什么必须独立。如果只有一个"权限等级"字段：
//!
//! ```text
//! 父级："可读任意路径"       →  等级 = 高
//! 子级需要的：读 src/ 目录     →  等级 = 低
//!
//! 单一等级模型下，"低"覆盖不了"高"之外的语义 —— 你无法表达
//! "可以读文件但不能访问密钥"这种组合。
//! ```
//!
//! 拆成独立维度后，子级可以同时是"路径受限"+"不允许密钥"+"不允许外部副作用"，
//! 而父级的"可读"完全不影响这几个判断。
//!
//! # 术语
//!
//! - **GrantScope（授权作用域）**：一份权限的**值**，描述"允许什么"。
//!   ⚠ 它**不是**授权凭证（token）。凭证是可以出示的东西，作用域是描述性的数据。
//! - **intersect（取交集）**：本文件的中心操作，产出更窄的子作用域。
//! - **principal（主体）**：被授权的那个人/角色，即 [`PrincipalId`]。
//! - **authority epoch（授权世代）**：一个单调递增的整数，用来区分"不同代的授权"。
//!   世代不同的两层**不允许**求交集 —— 那是两代不同的授权，混在一起没有意义。
//! - **CapabilityGrant（能力授权）**：本文件出现之前的老格式。
//!   [`GrantScope::from_capability_grant`] 负责从它升级，但**只升不降**。
//! - **digest（摘要）**：内容的哈希值，见 [`GrantScope::digest`]。
//!
//! # 上游契约
//!
//! A `GrantScope` is a policy value, not an authorization token. Child scopes are formed only by
//! intersecting server-owned layers; capability, secret and external-effect dimensions are
//! independent so a broad read scope cannot accidentally become a write/network/secret grant.

use kiana_domain::{
    json_digest, CapabilityGrant, CapabilityGrantId, CapabilityKind, CapabilityRequest, GrantId,
    PrincipalId, ProjectId, RiskLevel, SchemaVersion, ScopeDimension, ScopeLimit, ScopeSet,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// 授权作用域的 schema 标识符。
pub const GRANT_SCOPE_SCHEMA: &str = "kiana.grant-scope.v1";
/// 授权作用域的版本号。`SchemaVersion::new(1, 0)` 表示主版本 1、次版本 0。
pub const GRANT_SCOPE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 一份授权最多能带多少种能力。
///
/// 【为什么是 16】
/// 这是一个**结构性上限**，不是性能优化。它防止一份授权膨胀成一张能力清单：
/// 如果某个角色被授予了十几种能力，那说明权限设计出了问题 ——
/// 正确的做法是拆成多个角色，每个角色职责明确。
/// 上限在这里被强制执行，意味着"权限清单越滚越长"这类问题会在构造时就被拦住。
pub const MAX_GRANT_CAPABILITIES: usize = 16;

/// 一份能力授权作用域。
///
/// 【作用】
/// 描述"某个主体在这个项目里被允许做什么"。它是纯数据 —— 判定权在
/// [`GrantScope::allows_request`]，而这个类型本身只是被判定的那份声明。
///
/// 【⚠ 它不是授权凭证】
/// 文件头特意强调 "a policy value, not an authorization token"。
/// 区别在于：凭证可以**出示**给别人用来证明身份，作用域只是**描述**权限内容。
/// 即使拿到这份 JSON，也不能凭它直接执行任何操作 —— 还要经过控制面校验。
///
/// 【字段分组】
///
/// - **身份**：`grant_id`（本份授权的唯一 ID）、`parent_grant_id`（从哪一层派生的）、
///   `principal_id`（授权给谁）、`project_id`（在哪个项目内有效）。
/// - **权限内容**：`scope`（操作/路径/网络/额度限制）、`capabilities`（能力清单）。
/// - **三个危险开关**：`allow_secret`、`allow_external`、`delegation_allowed`。
///   这三个必须与 `capabilities` 里的对应项一致，见 [`GrantScope::validate`]。
/// - **有效期与防伪**：`authority_epoch`、`expires_at_unix_ms`、`grant_digest`。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantScope {
    pub schema: String,
    pub version: SchemaVersion,
    pub grant_id: GrantId,
    #[serde(default)]
    pub parent_grant_id: Option<GrantId>,
    pub principal_id: PrincipalId,
    pub project_id: ProjectId,
    pub scope: ScopeSet,
    pub capabilities: Vec<CapabilityKind>,
    pub allow_secret: bool,
    pub allow_external: bool,
    pub delegation_allowed: bool,
    pub authority_epoch: u64,
    pub expires_at_unix_ms: u64,
    pub grant_digest: String,
}

impl GrantScope {
    /// 构造一份授权作用域。
    ///
    /// 【作用】
    /// 校验并创建一份新的授权作用域。**这是唯一的正规构造入口** ——
    /// 字段是 `pub` 的，理论上可以绕过它直接用结构体字面量构造，
    /// 但那样就跳过了排序、摘要计算和校验。
    ///
    /// 【核心流程】
    /// 1. 把能力清单**排序**（`sort_by_key(capability_key)`）；
    /// 2. 组装结构体，摘要字段先留空；
    /// 3. 计算摘要填入（`grant.grant_digest = grant.digest()`）；
    /// 4. 校验。
    ///
    /// 【⚠ 为什么第 1 步的排序是必需的，不是优化】
    /// 因为 [`GrantScope::validate`] 里有一条**规范序（canonical order）**检查：
    /// 能力清单必须严格递增，否则报 `grant_scope_capabilities_noncanonical`。
    ///
    /// 规范序带来的好处是**同一个授权永远只有一种表示**。
    /// 如果 `[Read, Write]` 和 `[Write, Read]` 都合法，它们会算出两个不同的
    /// `grant_digest` —— 明明是同一份权限，摘要却不同。
    /// 这会让审计时无法判断"这两份授权是不是同一份"，也会让去重失效。
    /// 强制排序消除了这种歧义。
    ///
    /// 【⚠ 为什么必须先算摘要再校验】
    /// 因为校验函数里会检查 `grant_digest == self.digest()`。
    /// 如果先校验再填摘要，校验时摘要还是空的，必然失败。
    /// 顺序是：填内容 → 算摘要 → 校验摘要自洽。
    ///
    /// 【参数说明】
    /// - `parent_grant_id`：从哪一层派生而来。根层为 `None`。
    /// - `capabilities`：能力清单，**会被就地排序**，所以调用方传入后
    ///   这个 Vec 的顺序变了。这是 `mut` 参数存在的原因。
    /// - `authority_epoch`：授权世代。0 会被校验拒绝 —— 因为 0 不代表任何真实世代。
    ///
    /// 【失败情况】
    /// 任一校验不通过即返回 `Err`。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        grant_id: GrantId,
        parent_grant_id: Option<GrantId>,
        principal_id: PrincipalId,
        project_id: ProjectId,
        scope: ScopeSet,
        mut capabilities: Vec<CapabilityKind>,
        allow_secret: bool,
        allow_external: bool,
        delegation_allowed: bool,
        authority_epoch: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        capabilities.sort_by_key(capability_key);
        let mut grant = Self {
            schema: GRANT_SCOPE_SCHEMA.to_owned(),
            version: GRANT_SCOPE_VERSION,
            grant_id,
            parent_grant_id,
            principal_id,
            project_id,
            scope,
            capabilities,
            allow_secret,
            allow_external,
            delegation_allowed,
            authority_epoch,
            expires_at_unix_ms,
            grant_digest: String::new(),
        };
        grant.grant_digest = grant.digest();
        grant.validate()?;
        Ok(grant)
    }

    /// Adapt the historical CapabilityGrant into an explicit scope without widening it.
    ///
    /// 【作用】
    /// 把老格式的 [`CapabilityGrant`] **升级**成本文件的 [`GrantScope`] 格式。
    ///
    /// 【⚠ 核心约束：只升不降】
    /// 方法名里的 "without widening it"（不放宽它）是硬性要求。
    /// 升级后的作用域必须**恰好等价**或**更严格**于原始授权。
    /// 任何"顺手多给一点"的做法都是越权 —— 比如给一个非网络能力
    /// 补上 `allow_external = true`，就是凭空扩大了权限。
    ///
    /// 【核心流程】
    /// 1. 先校验原始授权（无效则不升级）；
    /// 2. 把老格式的各个字段翻译成新的多维作用域：
    ///    - `operation` → 操作维度，限一个值；
    ///    - `paths` → 路径维度，空则标为"不适用"；
    ///    - 能力是 Network → 网络维度限 "network"，否则不适用；
    ///    - 额度两个维度都标为不适用（老格式没有这个概念）。
    /// 3. 构造新的作用域。
    ///
    /// 【⚠ 两个 `NotApplicable` 的翻译为什么正确】
    /// `ScopeDimension::NotApplicable` 的含义是"这一维度不参与限制"，
    /// 而不是"这一维度被禁止"。老的 `CapabilityGrant` 根本没有
    /// 额度控制的概念，所以标为不适用是准确的翻译。
    ///
    /// 但要注意：**"不适用"和"允许任意"在后续求交集时行为不同**，
    /// 详见 [`ScopeSet::intersect`] 的语义。
    ///
    /// 【两个危险开关的翻译】
    /// `allow_secret` 只在能力**恰好是** Secret 时才为真；
    /// `allow_external` 只在能力**恰好是** Network 时才为真。
    ///
    /// 为什么用 `==` 而不是"能力里包含 Secret"？因为老格式一个授权
    /// **只有一种**能力。如果用 `contains`，一个 Network 授权也会
    /// 被打上 `allow_secret`，那凭空多给了密钥权限。
    /// 这里必须精确相等。
    ///
    /// 【失败情况】
    /// 原始授权无效 → 转换错误；作用域构造失败 → 构造错误。
    pub fn from_capability_grant(
        grant: &CapabilityGrant,
        principal_id: PrincipalId,
        project_id: ProjectId,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        grant.validate().map_err(|error| error.to_owned())?;
        let paths = if grant.paths.is_empty() {
            ScopeDimension::NotApplicable
        } else {
            ScopeDimension::Restricted(grant.paths.clone())
        };
        let scope = ScopeSet::new(
            ScopeDimension::Restricted(vec![grant.operation.clone()]),
            paths,
            ScopeDimension::NotApplicable,
            if grant.capability == CapabilityKind::Network {
                ScopeDimension::Restricted(vec!["network".to_owned()])
            } else {
                ScopeDimension::NotApplicable
            },
            ScopeLimit::NotApplicable,
            ScopeLimit::NotApplicable,
        )?;
        Self::new(
            GrantId::new(),
            None,
            principal_id,
            project_id,
            scope,
            vec![grant.capability.clone()],
            grant.capability == CapabilityKind::Secret,
            grant.capability == CapabilityKind::Network,
            grant.delegation_allowed,
            authority_epoch,
            grant.expires_at_unix_ms,
        )
    }

    /// Intersect two grant layers. The resulting grant is a fresh server-owned identity and can
    /// never be used to transfer a scope to another principal/project.
    ///
    /// 【作用 —— 本文件最重要的方法】
    /// 把两层授权求交集，产出一个**必然更窄**的子授权。这是权限收缩的唯一实现。
    ///
    /// 【⚠ 三条前置检查：先确认"能比"，再求交集】
    ///
    /// 1. **主体必须相同** —— 否则 `grant_scope_principal_mismatch`。
    /// 2. **项目必须相同** —— 否则 `grant_scope_project_mismatch`。
    /// 3. **授权世代必须相同** —— 否则 `grant_scope_authority_epoch_mismatch`。
    ///
    /// 为什么第 3 条这么重要？代际不同的两份授权，是**两次独立的授权行为**。
    /// 比如管理员在世代 5 撤回了某人的权限，那份"世代 3 的旧授权"就应当彻底失效。
    /// 如果允许跨代求交，旧的宽权限会和新世代的窄权限混合，
    /// 产生一份"看起来是新版、实际含旧权限"的授权 —— 撤销就失效了。
    ///
    /// 【⚠ 结果是一份全新的身份，不是任何一方的副本】
    /// 方法返回的授权带 `GrantId::new()`（全新 ID）和 `parent_grant_id = Some(self.grant_id)`。
    ///
    /// 注释里那句 "a fresh server-owned identity and can never be used to transfer
    /// a scope to another principal/project"（一个全新的、服务端持有的身份，
    /// 绝不可能被用来把作用域转移给别的主体/项目）是关键。
    ///
    /// 为什么要强调这一点？因为"权限转移"是最危险的越权形式。
    /// 如果交集结果复用了某一方的 ID，那么持有那个 ID 的一方
    /// 就能通过某种途径把权限"传递"出去。用全新 ID 从根上堵死这条路。
    ///
    /// 【⚠ 交集为空时必须报错，而不是返回空授权】
    /// 能力交集为空 → `grant_scope_capability_intersection_empty`。
    ///
    /// 为什么不能返回一个"什么都不允许"的空授权？因为空授权看起来是合法的，
    /// 可以被继续传递、持久化、写进事件。报错则让调用方明确知道
    /// "这两层授权完全不兼容"，可以走人工介入或重新申请。
    /// **静默的空权限会让问题延后暴露。**
    ///
    /// 【各字段的合并规则 —— 全部是"与"或"取小"】
    ///
    /// | 字段 | 规则 | 含义 |
    /// |---|---|---|
    /// | `scope` | `self.scope.intersect(other)` | 逐维度取交集 |
    /// | `capabilities` | 两边都有的才保留 | 能力不放大 |
    /// | `allow_secret` | `self && other` | 密钥权限不放大 |
    /// | `allow_external` | `self && other` | 外部副作用权限不放大 |
    /// | `delegation_allowed` | `self && other` | 委派权不放大 |
    /// | `expires_at_unix_ms` | `.min()` | 有效期取较早者 |
    ///
    /// `expires_at_unix_ms` 用 `.min()` 而不是 `.max()`：有效期**取短**，
    /// 因为更长的有效期意味着权限存续更久，那是不安全的方向。
    ///
    /// 【⚠ 常见错误：把某一项写成 `||`】
    /// 本方法里**每一个**合并都是 `&&` 或 `.min()`。
    /// 任何一处写成 `||` 或 `.max()`，就是一处越权漏洞。
    /// 修改时请逐行核对这张表。
    ///
    /// 【副作用】
    /// 无。不修改 `self` 也不修改 `other` —— 产出的是全新对象。
    pub fn intersect(&self, other: &Self) -> Result<Self, String> {
        self.validate()?;
        other.validate()?;
        if self.principal_id != other.principal_id {
            return Err("grant_scope_principal_mismatch".to_owned());
        }
        if self.project_id != other.project_id {
            return Err("grant_scope_project_mismatch".to_owned());
        }
        if self.authority_epoch != other.authority_epoch {
            return Err("grant_scope_authority_epoch_mismatch".to_owned());
        }
        let scope = self.scope.intersect(&other.scope)?;
        let capabilities = self
            .capabilities
            .iter()
            .filter(|left| other.capabilities.iter().any(|right| right == *left))
            .cloned()
            .collect::<Vec<_>>();
        if capabilities.is_empty() {
            return Err("grant_scope_capability_intersection_empty".to_owned());
        }
        Self::new(
            GrantId::new(),
            Some(self.grant_id),
            self.principal_id,
            self.project_id,
            scope,
            capabilities,
            self.allow_secret && other.allow_secret,
            self.allow_external && other.allow_external,
            self.delegation_allowed && other.delegation_allowed,
            self.authority_epoch,
            self.expires_at_unix_ms.min(other.expires_at_unix_ms),
        )
    }

    /// 把多层授权一次性求交集。
    ///
    /// 【作用】
    /// [`GrantScope::intersect`] 的批量版本，用于"模板 → 部门 → 包 → 审批"这种
    /// 多层同时收敛的场景。
    ///
    /// 【⚠ 空输入必须报错】
    /// `split_first()` 对空切片返回 `None`，此时报 `grant_scope_layers_required`。
    ///
    /// 为什么不能把空输入当作"全集"（返回一份无限制的授权）？
    /// 因为那会是最危险的默认值 —— 一个 bug 导致层列表意外为空时，
    /// 系统会静默地授予**全部权限**。fail-closed 在这里就是拒绝。
    ///
    /// 【核心流程】
    /// `try_fold` 从第一份开始，逐个与后面的求交。
    /// 任何一层失败立即返回该错误（`try_fold` 的短路语义）。
    ///
    /// 【为什么用 `try_fold` 而不是 `for` 循环】
    /// 效果等价，但 `try_fold` 天然短路，且避免了 `?` 写在循环里的
    /// 所有权处理样板。这是惯用写法，不是性能考量。
    ///
    /// 【为什么拿 `first.clone()` 作为起点】
    /// 因为 [`GrantScope::intersect`] 取 `&self` 而返回新值。
    /// 如果直接用 `first`（引用），会因为后续 `current` 的类型不匹配而编译不过。
    /// 这一行的克隆是"用第一层作为累加器初始值"的标准写法。
    ///
    /// 【⚠ 求交集的顺序不影响结果】
    /// 交集运算是可交换的（`A ∩ B = B ∩ A`），所以层序无关。
    /// 但要注意：**某一层的错误会先暴露**。
    /// 传进来的层若有一份本身无效，`intersect` 开头的 `validate()` 会立即拒绝。
    pub fn intersect_all(layers: &[Self]) -> Result<Self, String> {
        let Some((first, rest)) = layers.split_first() else {
            return Err("grant_scope_layers_required".to_owned());
        };
        rest.iter()
            .try_fold(first.clone(), |current, next| current.intersect(next))
    }

    /// 判断 `child` 是否完全落在 `self` 的范围内。
    ///
    /// 【作用】
    /// 包含关系判定：子授权是否**确实是**父授权的一个更窄版本。
    ///
    /// 【与 `intersect` 的区别 —— 重要】
    /// 两者方向相反：
    /// - [`GrantScope::intersect`] **计算**交集，需要产出一份新授权；
    /// - 本方法只**检查**包含关系，不产出任何东西。
    ///
    /// 典型用途：验证"这个请求用的授权，是不是恰好由我这条链派生的"，
    /// 防止用一个来路不明的授权冒充。
    ///
    /// 【核心流程】
    /// 1. 双方各自校验；
    /// 2. 主体或项目不同 → 直接 `Ok(false)`（**不是错误**）。
    ///    不同主体的话，"包含"这个问题本身就没有意义。
    /// 3. 逐项检查子级是否都不超过父级。
    ///
    /// 【⚠ 第 2 步为什么返回 `Ok(false)` 而不是 `Err`】
    /// 因为"主体不同"是一个**正常且安全**的答案 —— 这个授权本来就不该被用在这里。
    /// 报成错误会让上层难以区分"安全地拒绝了"和"系统出故障了"。
    /// 这是 fail-closed 的另一种表达：**不确定就说不允许，但不要说系统坏了**。
    ///
    /// 【⚠ 委派检查的方向容易搞反】
    /// 代码是 `!child.delegation_allowed || self.delegation_allowed`，
    /// 读作"子级要委派，父级必须也允许委派"。
    ///
    /// 展开成布尔：`如果子级不需要委派，条件为真（通过）`；
    /// `否则检查父级是否允许委派`。
    /// 对应的语义是：**子级的委派权只能来自父级的委派权**。
    /// 父级不允许委派，子级就不可能自己获得委派权。
    pub fn contains(&self, child: &Self) -> Result<bool, String> {
        self.validate()?;
        child.validate()?;
        if self.principal_id != child.principal_id || self.project_id != child.project_id {
            return Ok(false);
        }
        Ok(child.authority_epoch == self.authority_epoch
            && child.expires_at_unix_ms <= self.expires_at_unix_ms
            && (!child.delegation_allowed || self.delegation_allowed)
            && child
                .capabilities
                .iter()
                .all(|capability| self.capabilities.contains(capability))
            && (!child.allow_secret || self.allow_secret)
            && (!child.allow_external || self.allow_external)
            && child.scope.is_subset_of(&self.scope)?)
    }

    /// 判断这份授权是否允许某个具体的能力请求。
    ///
    /// 【作用】
    /// 运行时判定的入口：给定一次实际请求，这份授权放不放行。
    ///
    /// 【核心流程 —— 五道检查，任何一道不过都返回 `Ok(false)`】
    ///
    /// 1. **有效期**：`now >= expires_at_unix_ms` → 拒绝。
    ///    ⚠ 用 `>=` 而非 `>`：到期时刻的**那一毫秒**就已经失效了。
    ///    写成 `>` 会让授权多活 1 毫秒，属于边界错误。
    /// 2. **能力**：请求的能力不在清单里 → 拒绝。
    /// 3. **操作**：操作不在 scope 允许范围 → 拒绝。
    /// 4. **危险维度**：
    ///    - 请求 Secret 能力但 `allow_secret` 为假 → 拒绝；
    ///    - 请求的风险等级是"外部副作用"或"关键"，但 `allow_external` 为假 → 拒绝。
    /// 5. **路径**：如果请求参数里带了 `path` 字段，路径必须落在允许范围内。
    ///
    /// 【⚠ 关于第 4 步用风险等级而非能力来判断】
    /// 密钥权限看的是能力（必须是 Secret）；
    /// 外部副作用权限看的是**风险等级**。
    /// 两者判断依据不同，因为"会产生外部副作用"这件事可以由多种能力触发
    /// （比如一次写文件在某些配置下也可能触及外部），
    /// 而"访问密钥"是明确的单一能力。
    ///
    /// 【⚠ 关于第 5 步：为什么用 `if let` 而不是要求必须有 path】
    /// 路径检查是**可选的**，因为不是所有请求都涉及文件操作。
    /// `if let Some(path) = ...` 意味着"没传路径就跳过这项检查"。
    ///
    /// ⚠ 这里有一个隐含约定：请求的 `arguments` 里如果要用文件路径，
    /// 必须以 `path` 这个键名传递。否则这项检查会被跳过。
    /// 这一点在修改请求构造逻辑时需要留意。
    ///
    /// 【⚠ 返回 `Ok(false)` 而不是 `Err`】
    /// 授权不允许是**正常的业务结果**，不是系统故障。
    /// 上层需要能区分"这次请求不合规"（拒绝并继续）和"授权数据损坏了"（报错）。
    /// 但注意：内部 `validate()` 失败**会**返回 `Err` —— 因为那说明
    /// 授权数据本身有问题，属于故障而非业务判断。
    ///
    /// 【副作用】
    /// 无。纯判定。
    pub fn allows_request(
        &self,
        request: &CapabilityRequest,
        now_unix_ms: u64,
    ) -> Result<bool, String> {
        self.validate()?;
        if now_unix_ms >= self.expires_at_unix_ms
            || !self.capabilities.contains(&request.capability)
            || !self.scope.allows_operation(&request.operation)
        {
            return Ok(false);
        }
        if request.capability == CapabilityKind::Secret && !self.allow_secret {
            return Ok(false);
        }
        if matches!(
            request.risk,
            RiskLevel::ExternalSideEffect | RiskLevel::Critical
        ) && !self.allow_external
        {
            return Ok(false);
        }
        if let Some(path) = request.arguments.get("path").and_then(Value::as_str) {
            if !self.scope.allows_path(path) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Materialize one legacy `CapabilityGrant` from this already-intersected scope.  The
    /// operation and capability are checked against the scope before any ID is minted, so a
    /// caller cannot turn a broad parent scope into a secret/network/provider grant by choosing a
    /// different operation at the conversion boundary.
    ///
    /// 【作用】
    /// 把本文件的新格式**降级**回老格式 [`CapabilityGrant`]，
    /// 供还没迁移到 `GrantScope` 的老代码使用。
    ///
    /// 【⚠ 为什么这是"降级"而不是"转换"】
    /// 新格式能表达的东西比老格式多（多维 scope、独立的额度控制）。
    /// 降级时这些信息必然丢失。所以使用降级结果的地方，
    /// 实际拿到的权限可能比原始作用域更模糊。
    /// 这也是为什么这个方向需要严格检查，而升级方向（`from_capability_grant`）
    /// 相对宽松 —— 降级有丢失信息的风险，升级没有。
    ///
    /// 【⚠ 核心安全设计：先检查，再铸 ID】
    /// 注释里那句 "checked against the scope **before any ID is minted**"
    /// （在铸造任何 ID **之前**检查）点出了本方法的关键。
    ///
    /// 如果顺序反过来 —— 先铸 ID 再检查，检查失败时 ID 已经发出去了 ——
    /// 那么攻击者就可以不断尝试不同的操作组合，拿到一堆"看起来存在但其实无效"的授权 ID。
    /// 这些 ID 可能被记录、传播，产生困惑。
    ///
    /// 现在的顺序保证了：**检查不通过，一个 ID 都不会产生。**
    ///
    /// 【⚠ 注释指出的具体攻击手法】
    /// "a caller cannot turn a broad parent scope into a secret/network/provider grant
    /// by choosing a different operation at the conversion boundary"
    /// —— 调用方不能通过"在转换边界上选一个不同的操作"，
    /// 把一份宽泛的父级作用域变成密钥/网络/服务商授权。
    ///
    /// 通俗说：如果父级只允许"读文件"，那在这一步无论调用方填什么操作名，
    /// 都不可能拿到一份 Secret 授权。因为下面紧接的能力检查
    /// `!self.capabilities.contains(&capability)` 会拦下来 ——
    /// 前提是 `capability` 参数被如实传入。
    ///
    /// 【⚠ 四项检查，缺一不可】
    /// 1. 能力在授权清单里；
    /// 2. 操作在 scope 允许范围内；
    /// 3. Secret 能力需要 `allow_secret`；
    /// 4. Network 能力需要 `allow_external`。
    ///
    /// 第 3、4 条是容易被漏掉的 —— 只查能力清单是不够的，
    /// 因为一份授权可以**列出** Secret 能力但 `allow_secret` 为假
    /// （这是合法状态，见 [`GrantScope::validate`] 的维度一致性检查）。
    /// 三个开关必须一起看。
    ///
    /// 【路径的三种情况】
    /// - `NotApplicable` → 用 `["."]`（当前目录）兜底。
    ///   ⚠ 这是老格式没有"不限制路径"这个概念的妥协：写一个具体路径
    ///   比写空数组更安全。
    /// - `Restricted` 且为空 → 报错。空列表意味着"交集算出来是空"，
    ///   这不应该发生，遇到了说明上游有 bug。
    /// - `Restricted` 且非空 → 直接沿用。
    ///
    /// 【失败情况】
    /// 四项检查任一不过 → `grant_scope_capability_materialization_denied`。
    /// 路径为空 → `grant_scope_path_intersection_empty`。
    /// 最后还会用老格式自己的 `validate()` 再校验一次产物 ——
    /// 确保降级结果是**合法的老格式**，而不是"差不多能用"。
    pub fn to_capability_grant(
        &self,
        capability: CapabilityKind,
        operation: impl Into<String>,
        resources: Vec<String>,
        approval_id: Option<kiana_domain::ApprovalId>,
    ) -> Result<CapabilityGrant, String> {
        self.validate()?;
        let operation = operation.into();
        if !self.capabilities.contains(&capability)
            || !self.scope.allows_operation(&operation)
            || (capability == CapabilityKind::Secret && !self.allow_secret)
            || (capability == CapabilityKind::Network && !self.allow_external)
        {
            return Err("grant_scope_capability_materialization_denied".to_owned());
        }
        let paths = match &self.scope.paths {
            ScopeDimension::NotApplicable => vec![".".to_owned()],
            ScopeDimension::Restricted(paths) if paths.is_empty() => {
                return Err("grant_scope_path_intersection_empty".to_owned())
            }
            ScopeDimension::Restricted(paths) => paths.clone(),
        };
        let grant = CapabilityGrant {
            schema: kiana_domain::CAPABILITY_GRANT_SCHEMA.to_owned(),
            grant_id: CapabilityGrantId::new(),
            capability,
            operation,
            resources,
            paths,
            expires_at_unix_ms: self.expires_at_unix_ms,
            approval_id,
            delegation_allowed: self.delegation_allowed,
        };
        grant
            .validate()
            .map_err(|error| error.to_owned())
            .map(|_| grant)
    }

    /// 校验这份授权自身是否自洽。
    ///
    /// 【作用】
    /// 检查授权**内部**的一致性。**这是所有操作的必经关口** ——
    /// [`GrantScope::intersect`]、[`GrantScope::contains`]、
    /// [`GrantScope::allows_request`] 全都在开头调用它。
    ///
    /// 【为什么每个入口都要重新校验，而不是只在构造时校验一次】
    /// 因为这份结构可以经过序列化、跨进程传递、长期存储。
    /// 从别处读回来的数据可能：被手工改过、来自旧版本、字段缺失、
    /// 或者被恶意构造。**"我构造时校验过"不能作为"它现在仍然有效"的依据。**
    ///
    /// 【核心检查 —— 六组】
    ///
    /// **① 头部字段（任一不满足 → `grant_scope_header_invalid`）**
    /// - schema 字符串必须精确匹配；
    /// - 版本必须**兼容**（不是相等）—— 见下方说明；
    /// - 三个 UUID 都不能是 nil（全零 UUID，通常意味着"未初始化"）；
    /// - `authority_epoch` 不能为 0（0 不代表任何真实世代）；
    /// - `expires_at_unix_ms` 不能为 0（否则永远过期或永不合理的值）；
    /// - 能力清单不能为空，也不能超过 16 项。
    ///
    /// **② 父子关系：不能自己指向自己（→ `grant_scope_parent_self`）**
    /// 如果 `parent_grant_id == grant_id`，那这份授权声称自己是自己的父级。
    /// 沿着派生链回溯会陷入死循环。
    ///
    /// **③ 能力清单：无重复、无未知、且有序**
    /// - 重复 → `grant_scope_capabilities_duplicate`（用 `BTreeSet` 查重）；
    /// - `Other("")` 空字符串 → `grant_scope_capabilities_unknown`（未知能力）；
    /// - **非规范序 → `grant_scope_capabilities_noncanonical`**。
    ///
    ///   最后这条最容易让人困惑。代码用 `windows(2).any(|pair| k0 >= k1)`，
    ///   即检查每一对相邻元素是否**严格递增**。
    ///   为什么用 `>=` 而不是 `>`？因为相等的情况已经被 ③ 的查重拦住了，
    ///   这里只需要确认"递增"。写成 `>=` 是为了让这个函数自身也是完备的 ——
    ///   万一将来去重逻辑改了，这里的检查依然能独立发现非递增的情况。
    ///
    /// **④ 三个开关必须与能力清单一致**
    /// - `allow_secret` 为真但清单里没有 Secret → `grant_scope_secret_dimension_mismatch`；
    /// - `allow_external` 为真但清单里没有 Network → `grant_scope_external_dimension_mismatch`。
    ///
    ///   为什么这是矛盾状态？因为这两个开关的作用是"在已有能力之上，
    ///   再解锁这个更危险的维度"。如果清单里根本没有这个能力，
    ///   那这个开关就是凭空多出来的权限。必须拒绝。
    ///
    /// **⑤ 摘要格式合法（→ `grant_scope_digest_invalid`）**
    /// 见 [`validate_digest`]。
    ///
    /// **⑥ 摘要自洽（→ `grant_scope_digest_mismatch`）**
    /// 重算摘要与存储的摘要比对。这一步防的是**内容被改**：
    /// 有人拿到一份合法授权，把 `capabilities` 从 `[Read]` 改成
    /// `[Read, Secret]`，但没动摘要 —— 这时摘要对不上，授权被拒。
    ///
    /// ⚠ 如果你修改了 `digest()` 里 `json!` 的字段清单，
    /// 所有已持久化的旧授权都会校验失败。改这个函数要格外小心。
    ///
    /// 【关于版本"兼容"而非"相等"】
    /// `is_compatible_with` 允许**次版本**不同。含义是：
    /// 主版本必须一致（有不兼容的语义变化），次版本可以更高（只有向前兼容的新增）。
    /// 这让 `1.0` 的授权能被 `1.1` 的代码读懂。
    ///
    /// 【⚠ 不要给这里的检查"优化"掉】
    /// 这个函数看起来很长、很多分支，容易让人想精简。
    /// 但每一组检查都对应一类真实的安全问题或数据损坏场景。
    /// 删掉任何一组都会开出一个静默的漏洞。
    ///
    /// 【失败情况】
    /// 返回 `Err(String)`，字符串是稳定的原因码（snake_case）。
    /// 这些码会进入日志和事件账本，属于**对外契约**，不要随意改写。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != GRANT_SCOPE_SCHEMA
            || !self.version.is_compatible_with(&GRANT_SCOPE_VERSION)
            || self.grant_id.as_uuid().is_nil()
            || self.principal_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.expires_at_unix_ms == 0
            || self.capabilities.is_empty()
            || self.capabilities.len() > MAX_GRANT_CAPABILITIES
        {
            return Err("grant_scope_header_invalid".to_owned());
        }
        if self.parent_grant_id == Some(self.grant_id) {
            return Err("grant_scope_parent_self".to_owned());
        }
        self.scope.validate()?;
        let mut seen = BTreeSet::new();
        for capability in &self.capabilities {
            if !seen.insert(capability_key(capability)) {
                return Err("grant_scope_capabilities_duplicate".to_owned());
            }
            if *capability == CapabilityKind::Other(String::new()) {
                return Err("grant_scope_capability_unknown".to_owned());
            }
        }
        if self
            .capabilities
            .windows(2)
            .any(|pair| capability_key(&pair[0]) >= capability_key(&pair[1]))
        {
            return Err("grant_scope_capabilities_noncanonical".to_owned());
        }
        if self.allow_secret && !self.capabilities.contains(&CapabilityKind::Secret) {
            return Err("grant_scope_secret_dimension_mismatch".to_owned());
        }
        if self.allow_external && !self.capabilities.contains(&CapabilityKind::Network) {
            return Err("grant_scope_external_dimension_mismatch".to_owned());
        }
        validate_digest(&self.grant_digest, "grant_scope_digest")?;
        if self.grant_digest != self.digest() {
            return Err("grant_scope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 从 JSON 值反序列化一份授权。
    ///
    /// 【作用】
    /// 反序列化入口。因为结构体上标了 `#[serde(deny_unknown_fields)]`，
    /// 遇到未识别字段会直接失败而不是忽略 —— 这在安全数据上很重要：
    /// 忽略未知字段意味着一个拼写错误的字段名会被静默丢弃，
    /// 而发送方以为它生效了。
    ///
    /// 【⚠ 解码后立即校验**
    /// `grant.validate()?` 不可省略。JSON 里什么都能写 ——
    /// 空的能力清单、矛盾的开关、伪造的摘要，都能通过反序列化。
    /// 校验才是真正的关卡。
    ///
    /// 【失败情况】
    /// - JSON 解析失败 → `grant_scope_decode_failed`（丢掉 serde 的原始错误，
    ///   只留稳定码，理由和 `connector_https.rs` 里一样：不让上层依赖
    ///   会随 serde 版本变化的消息）；
    /// - 校验不通过 → 透传具体原因码。
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let grant: Self = serde_json::from_value(value.clone())
            .map_err(|_| "grant_scope_decode_failed".to_owned())?;
        grant.validate()?;
        Ok(grant)
    }

    /// 序列化成 JSON 值。
    ///
    /// 【作用】
    /// 序列化出口。**不调用 `validate()`** —— 序列化不改变数据，
    /// 一个无效的对象序列化出来还是无效的，由读取方负责校验。
    ///
    /// （对比 [`GrantScope::from_json`] 会校验：因为那个方向是"外部数据进来"，
    /// 必须把关。而这个方向是"内部数据出去"，把关没有意义。）
    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "grant_scope_encode_failed".to_owned())
    }

    /// 计算这份授权的内容摘要。
    ///
    /// 【作用】
    /// 对除 `grant_digest` 自身之外的所有字段做哈希，得到一个固定长度的指纹。
    ///
    /// 【⚠ 摘要字段本身不参与计算】
    /// `json!` 里刻意**没有** `grant_digest`。这是必须的 ——
    /// 否则摘要要算自己，无解。
    ///
    /// 【⚠ 这个字段列表就是"授权的身份"】
    /// 任何一个字段被加进或移出这个 `json!`，都会导致所有历史授权的摘要
    /// 校验失败。要改动这里，等于换了授权的身份定义。
    ///
    /// 【⚠ 顺序无关】
    /// `json!` 产出的是 `serde_json::Map`（内部有序映射），
    /// 而 `json_digest` 内部会做规范化排序，所以这里的键顺序不影响结果。
    /// 但 `capabilities` 是**数组**，顺序会影响摘要 —— 这正是
    /// [`GrantScope::new`] 要强制排序的原因。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "grant_id": self.grant_id,
            "parent_grant_id": self.parent_grant_id,
            "principal_id": self.principal_id,
            "project_id": self.project_id,
            "scope": self.scope,
            "capabilities": self.capabilities,
            "allow_secret": self.allow_secret,
            "allow_external": self.allow_external,
            "delegation_allowed": self.delegation_allowed,
            "authority_epoch": self.authority_epoch,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

/// 生成一个能力的**规范排序键**。
///
/// 【作用】
/// 把一个 [`CapabilityKind`] 转换成用于排序和去重的字符串键。
///
/// 【为什么需要它，而不是直接用枚举的 `Ord`】
/// 因为 [`CapabilityKind`] 里有一个 `Other(String)` 变体 ——
/// 它携带任意字符串，**没有内在的排序规则**。
/// 如果直接对枚举排序，`Other` 的比较结果会退化成"随便哪个"，
/// 导致同样的能力集合在不同构造顺序下排出不同结果，
/// 摘要也就跟着不稳定。
///
/// 加上 `other:` 前缀就是把这个变体纳入一套确定的规则里。
///
/// 【⚠ 前缀的必要性】
/// 假设没有前缀，`Other("read")` 会生成键 `"read"`，
/// 而内置的 `Read` 变体经 `format!("{capability:?}")` 也可能得到类似 `"read"`。
/// 两者会撞键 → 去重时误判为重复，或排序时顺序不确定。
/// `other:` 前缀把这个命名空间隔开了。
///
/// 【⚠ 排序键只用于排序/去重，不是身份标识】
/// 它不参与 [`GrantScope::digest`] 的语义，只在
/// [`GrantScope::new`] 排序和 [`GrantScope::validate`] 查重/查序时使用。
fn capability_key(capability: &CapabilityKind) -> String {
    match capability {
        // 未知能力：加前缀纳入确定的排序规则。
        CapabilityKind::Other(value) => format!("other:{value}"),
        // 内置能力：用 Debug 格式（变体名）再转小写，
        // 这样 "Shell"、"shell" 之类的拼写差异被归一化。
        _ => format!("{capability:?}").to_ascii_lowercase(),
    }
}

/// 校验一个摘要字符串的**格式**是否合法。
///
/// 【作用】
/// 只检查格式，不检查内容 —— 内容的比对在
/// [`GrantScope::validate`] 里做（重算摘要再比对）。
///
/// 【校验规则】
/// 1. 必须以 `sha256:` 开头 —— 固定前缀，表明用的是哪种算法。
///    ⚠ 加上算法名是为了将来能平滑升级到 sha512 等，
///    旧数据仍能靠前缀区分。
/// 2. 去掉前缀后必须**正好 64 个字符** —— SHA-256 输出 32 字节，
///    十六进制表示就是 64 个字符。
/// 3. 每个字符都必须是十六进制数字。
///
/// 【⚠ 为什么用 `hex.bytes().all(...)` 而不是 `chars()`】
/// `bytes()` 直接在字节层面检查，字符数与字节数在 UTF-8 下可能不同。
/// 用字节检查杜绝了"一个多字节字符算成 1 个"的漏洞。
///
/// 【失败情况】
/// 格式不对 → 返回 `{field}_invalid`，其中 `field` 是调用方传入的字段名。
/// 这样错误信息能指出是哪个字段的摘要出了问题。
fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

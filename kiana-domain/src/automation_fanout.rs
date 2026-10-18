//! 本文件定义 AUT-19 的父子 FanOut/FanIn 计划、真实 scope 校验与不执行副作用的扇出账本。
//!
//! # 在系统中的位置
//!
//! ```text
//! 调用方准备 AutomationFanoutParent + 每个 child 的 AutomationFanoutChildPlan
//!     -> ChildPlan::admit() 用 ScopeSet::intersect / is_subset_of 对照真实父 scope，
//!        拒绝 scope/budget/role/TTL 超父级与 parent 循环
//!     -> AutomationFanoutPlanV2::new()/validate() 检查唯一 child、预算求和、并发上限与摘要
//!     -> AutomationFanoutLedger::admit() 仅登记事实（不执行任何 child）
//!     -> start_child / observe_child 记录 started/observed/failed，fail-fast 把未启动 child
//!        标为 Skipped 并拒绝其后续 start
//!     -> merge(inputs) 按 (child_id, sequence) 稳定合并结果/证据，拒绝缺失/未知/重复输入
//!     -> parent_state() 只由已记录事实推导，没有调用方可设置的 status
//! ```
//!
//! # 边界与诚实口径
//!
//! 当前 workspace 的生产 Rust 源码中没有找到本文件 v2 计划或账本的实际调用者；`kiana-core`
//! 只提供只读门面。本文件不创建 workflow instance、不调度/并发执行 child、不分配真实
//! budget lease、不写 EventLog、不调用 Broker 或 handler。它证明的是“这些声明在领域层面被
//! 拒绝/接受”，不是运行时 fan-out 已接入产品；`parent_state()` 也只是事实折叠，不是持久状态。
//!
//! 角色边界使用父计划显式声明的 `allowed_child_roles` 允许列表；这是可检查的合同，不是
//! `RoleSpec` 权限层级的完整推导，真实授权仍必须由 ControlPlane 重新执行。
//!
//! 摘要均为无密钥 SHA-256 内容摘要，只能发现载荷变化，不是签名、批准或执行许可。
//!
//! # 本切片的拒绝语义（先拒绝，后成功）
//!
//! * scope 扩大：child scope 与 parent scope 求交后不等于 child 本身
//!   （`automation_fanout_scope_widening`），scope 自身无效时为 `automation_fanout_scope_invalid`；
//! * 预算超父级：单 child 或全部 child 求和超过父预算（`automation_fanout_budget_exceeded`，
//!   求和用 `checked_add`，溢出也拒绝）；
//! * role 超父级：child role 不在父声明的允许列表（`automation_fanout_role_widening`）；
//! * 循环 parent：child ID 等于 parent ID（`automation_fanout_cycle_refused`）；
//! * fail-fast 后启动：已标 Skipped 的 child 再次 `start_child` 被拒
//!   （`automation_fanout_child_skipped`）；
//! * fan-in 缺输入/未知/重复：`automation_fanout_fan_in_missing_child` /
//!   `_unknown_child` / `_duplicate`，child 尚未终态时 `_incomplete`。
//!
//! 旧版 `AutomationFanoutPlan` DTO 保持原样以兼容既有调用方；新增
//! `validate_with_scopes()` / `derive_declarations()` 让布尔声明与实际 scope/ID 对照，而不是继续
//! 盲信调用方。

use crate::{json_digest, ScopeSet};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// 旧版 FanOut 计划 DTO 的固定 wire schema 名称（保持向后兼容）。
pub const AUTOMATION_FANOUT_SCHEMA: &str = "kiana.automation-fanout.v1";

/// v2 父子扇出计划的固定 wire schema 名称。
pub const AUTOMATION_FANOUT_PLAN_SCHEMA: &str = "kiana.automation-fanout.v2";

/// 单个父计划允许的直接 child 上限；与 `kiana-workflow` 的 FanOut 分支上限一致。
pub const AUTOMATION_FANOUT_MAX_CHILDREN: usize = 32;

/// child scope 与 parent scope 求交后不相等时返回的稳定拒绝码。
pub const AUTOMATION_FANOUT_SCOPE_WIDENING: &str = "automation_fanout_scope_widening";
/// ScopeSet 自身无法通过校验（摘要/规范序/字段损坏）时的稳定拒绝码。
pub const AUTOMATION_FANOUT_SCOPE_INVALID: &str = "automation_fanout_scope_invalid";
/// 单 child 预算或全部 child 预算之和超过父预算时的稳定拒绝码。
pub const AUTOMATION_FANOUT_BUDGET_EXCEEDED: &str = "automation_fanout_budget_exceeded";
/// child role 不在父计划允许列表时的稳定拒绝码。
pub const AUTOMATION_FANOUT_ROLE_WIDENING: &str = "automation_fanout_role_widening";
/// child ID 等于 parent ID（parent 出现在 children 中）时的稳定拒绝码。
pub const AUTOMATION_FANOUT_CYCLE_REFUSED: &str = "automation_fanout_cycle_refused";
/// 父计划自身字段无效时的稳定拒绝码。
pub const AUTOMATION_FANOUT_PARENT_INVALID: &str = "automation_fanout_parent_invalid";
/// child 计划字段无效时的稳定拒绝码。
pub const AUTOMATION_FANOUT_CHILD_INVALID: &str = "automation_fanout_child_invalid";
/// child ID 列表为空或超过上限时的稳定拒绝码。
pub const AUTOMATION_FANOUT_CHILD_COUNT_INVALID: &str = "automation_fanout_child_count_invalid";
/// child ID 重复时的稳定拒绝码。
pub const AUTOMATION_FANOUT_CHILD_DUPLICATE: &str = "automation_fanout_child_duplicate";
/// child TTL 超过父剩余 TTL 时的稳定拒绝码。
pub const AUTOMATION_FANOUT_TTL_EXCEEDED: &str = "automation_fanout_ttl_exceeded";
/// 父计划深度已到上限时的稳定拒绝码。
pub const AUTOMATION_FANOUT_DEPTH_EXCEEDED: &str = "automation_fanout_depth_exceeded";
/// 并发的 child 数超过父计划上限时的稳定拒绝码。
pub const AUTOMATION_FANOUT_CONCURRENCY_EXCEEDED: &str = "automation_fanout_concurrency_exceeded";
/// 账本中不存在的 child ID 的稳定拒绝码。
pub const AUTOMATION_FANOUT_CHILD_UNKNOWN: &str = "automation_fanout_child_unknown";
/// 已因 fail-fast 标为 Skipped 的 child 被再次启动时的稳定拒绝码。
pub const AUTOMATION_FANOUT_CHILD_SKIPPED: &str = "automation_fanout_child_skipped";
/// child 已经启动过、不能再次启动时的稳定拒绝码。
pub const AUTOMATION_FANOUT_CHILD_ALREADY_STARTED: &str = "automation_fanout_child_already_started";
/// 尚未 started 的 child 被 observe 时的稳定拒绝码。
pub const AUTOMATION_FANOUT_CHILD_NOT_STARTED: &str = "automation_fanout_child_not_started";
/// 证据/输出摘要形状无效时的稳定拒绝码。
pub const AUTOMATION_FANOUT_EVIDENCE_INVALID: &str = "automation_fanout_evidence_invalid";
/// fan-in 输入中 (child_id, sequence) 重复时的稳定拒绝码。
pub const AUTOMATION_FANOUT_FAN_IN_DUPLICATE: &str = "automation_fanout_fan_in_duplicate";
/// fan-in 输入引用账本外 child 时的稳定拒绝码。
pub const AUTOMATION_FANOUT_FAN_IN_UNKNOWN_CHILD: &str = "automation_fanout_fan_in_unknown_child";
/// 已接纳的 child 没有出现在 fan-in 输入中时的稳定拒绝码。
pub const AUTOMATION_FANOUT_FAN_IN_MISSING_CHILD: &str = "automation_fanout_fan_in_missing_child";
/// child 尚未进入终态就请求 fan-in 时的稳定拒绝码。
pub const AUTOMATION_FANOUT_FAN_IN_INCOMPLETE: &str = "automation_fanout_fan_in_incomplete";
/// fan-in 输入 sequence 为 0 时的稳定拒绝码。
pub const AUTOMATION_FANOUT_FAN_IN_SEQUENCE_INVALID: &str =
    "automation_fanout_fan_in_sequence_invalid";
/// 旧版计划的 scope 布尔声明与真实 scope 不一致时的稳定拒绝码。
pub const AUTOMATION_FANOUT_SCOPE_DECLARATION_MISMATCH: &str =
    "automation_fanout_scope_declaration_mismatch";
/// 旧版计划的 cycle 布尔声明与真实 child ID 列表不一致时的稳定拒绝码。
pub const AUTOMATION_FANOUT_CYCLE_DECLARATION_MISMATCH: &str =
    "automation_fanout_cycle_declaration_mismatch";

/// 一份父执行展开为多个子执行的静态计划及其有限约束声明（旧版 v1，保持向后兼容）。
///
/// 结构体使用 `deny_unknown_fields`，SerDe 会拒绝多余 JSON 字段。字段经过 validator 检查后仍是
/// 请求方提供的声明；`child_scope_is_subset` 与 `parent_id_in_children` 不会被 `validate()`
/// 重新推导。需要真实 scope 对照时使用 [`AutomationFanoutPlan::validate_with_scopes`] 或直接改用
/// [`AutomationFanoutPlanV2`]。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationFanoutPlan {
    /// 必须等于 `AUTOMATION_FANOUT_SCHEMA`。
    pub schema: String,
    /// 扇出的父执行身份；只检查文本格式，不查找真实运行记录。
    pub parent_execution_id: String,
    /// 本计划要创建的子执行 ID 列表；必须非空、有界且不重复。
    pub child_execution_ids: Vec<String>,
    /// 允许的最大嵌套深度。
    pub max_depth: u32,
    /// 当前计划深度；必须严格小于 max_depth。
    pub depth: u32,
    /// 父执行声明允许的最大并发数。
    pub max_concurrency: u32,
    /// 本次申请的并发数；此处只检查数值不越界，不会创建或限制真实并发任务。
    pub requested_concurrency: u32,
    /// 父级声明可用总预算单位。
    pub parent_budget_units: u64,
    /// 每个 child 声明的预算单位；validator 将其乘以 child 数目与 parent budget 比较。
    pub child_budget_units: u64,
    /// 子执行允许使用的 TTL，单位毫秒。
    pub ttl_ms: u64,
    /// 父执行剩余 TTL，单位毫秒；child TTL 不得超过它。
    pub parent_ttl_remaining_ms: u64,
    /// 调用方声称每个 child scope 是 parent scope 的子集；`validate()` 不核对真实 scope。
    pub child_scope_is_subset: bool,
    /// 调用方声称 child ID 中不包含 parent ID；`validate()` 只检查此标记。
    pub parent_id_in_children: bool,
    /// 计划是否要求子任务失败时 fail-fast；此值只进入摘要，本文件不停止运行中的 siblings。
    pub fail_fast: bool,
    /// 必须为 true，表示调用方要求汇总 child 结果；本 validator 不实际执行 fan-in。
    pub fan_in_required: bool,
    /// 对其余计划字段的 SHA-256 内容摘要；可以被提供者自行重算，不是执行许可。
    pub plan_digest: String,
}

impl AutomationFanoutPlan {
    /// 校验 v1 计划的结构和声明边界，不调度任务、不分配真实 scope/budget。
    ///
    /// 子 ID 数量为 1..=32 且必须唯一；depth/max_depth、并发申请、预算、TTL 必须满足范围；还要求
    /// child_scope_is_subset=true、parent_id_in_children=false、fan_in_required=true 和摘要自洽。
    /// 文本 ID 上限为 256 UTF-8 字节，拒绝 NUL/CR/LF，但不拒绝所有内部空白。
    /// 任一检查失败都返回 `automation_fanout_plan_invalid`；成功只表示输入对象满足本函数规则。
    ///
    /// 注意：预算总量用 `saturating_mul()` 计算后再比较。若乘法溢出饱和为 `u64::MAX`，且父预算
    /// 也恰为 `u64::MAX`，当前 `>` 比较不会拒绝数学上超出表示范围的总额；这是 v1 静态边界问题，
    /// 为保持向后兼容未改变算术行为。v2 计划改用 `checked_add` 并单独拒绝溢出。
    pub fn validate(&self) -> Result<(), &'static str> {
        self.validate_structure()?;
        // parent 出现在 child 列表里就是循环，不再采信调用方的布尔声明。
        if self.actual_parent_id_in_children() || self.parent_id_in_children {
            return Err(AUTOMATION_FANOUT_CYCLE_REFUSED);
        }
        if !self.child_scope_is_subset || !self.fan_in_required {
            return Err("automation_fanout_plan_invalid");
        }
        Ok(())
    }

    /// 只校验 v1 计划的结构与数值边界，不检查 scope/cycle 声明布尔值本身。
    ///
    /// `validate_with_scopes()` 先做这一步，再用真实值判定声明是否成立，因此声明不实会得到
    /// 专门的 mismatch 码，而不是笼统的 `automation_fanout_plan_invalid`。
    fn validate_structure(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_FANOUT_SCHEMA
            || !valid_text(&self.parent_execution_id)
            || self.child_execution_ids.is_empty()
            || self.child_execution_ids.len() > AUTOMATION_FANOUT_MAX_CHILDREN
            || self.child_execution_ids.iter().any(|v| !valid_text(v))
            || self
                .child_execution_ids
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != self.child_execution_ids.len()
            || self.max_depth == 0
            || self.depth >= self.max_depth
            || self.max_concurrency == 0
            || self.requested_concurrency == 0
            || self.requested_concurrency > self.max_concurrency
            || self.requested_concurrency > self.child_execution_ids.len() as u32
            || self.child_budget_units == 0
            || self
                .child_budget_units
                .saturating_mul(self.child_execution_ids.len() as u64)
                > self.parent_budget_units
            || self.ttl_ms == 0
            || self.ttl_ms > self.parent_ttl_remaining_ms
            || !valid_digest(&self.plan_digest)
            || self.plan_digest != self.digest()
        {
            return Err("automation_fanout_plan_invalid");
        }
        Ok(())
    }

    /// 用真实 ScopeSet 核对 v1 计划里的 scope 与 cycle 布尔声明。
    ///
    /// 【输入】`parent_scope` 与 `child_scope` 是服务端掌握的实际范围，不是计划自报文本。
    /// 【输出】声明与实际一致返回 `Ok(())`；scope 实际更宽返回
    /// `automation_fanout_scope_widening`，scope 无效返回 `automation_fanout_scope_invalid`，
    /// 声明与事实不符返回 `automation_fanout_scope_declaration_mismatch` /
    /// `automation_fanout_cycle_declaration_mismatch`，真实 child 列表含 parent 返回
    /// `automation_fanout_cycle_refused`。
    /// 【边界】仍只比较一份 child scope；v1 计划不为每个 child 保存独立 scope，逐 child 绑定请用
    /// [`AutomationFanoutPlanV2`]。
    pub fn validate_with_scopes(
        &self,
        parent_scope: &ScopeSet,
        child_scope: &ScopeSet,
    ) -> Result<(), &'static str> {
        self.validate_structure()?;
        let actual_subset = child_scope_within_parent(parent_scope, child_scope)?;
        if !actual_subset {
            return Err(AUTOMATION_FANOUT_SCOPE_WIDENING);
        }
        if !self.child_scope_is_subset {
            return Err(AUTOMATION_FANOUT_SCOPE_DECLARATION_MISMATCH);
        }
        let actual_cycle = self.actual_parent_id_in_children();
        if self.parent_id_in_children != actual_cycle {
            return Err(AUTOMATION_FANOUT_CYCLE_DECLARATION_MISMATCH);
        }
        if actual_cycle || !self.fan_in_required {
            return Err(AUTOMATION_FANOUT_CYCLE_REFUSED);
        }
        Ok(())
    }

    /// 从真实 scope 与 child ID 列表重算两个声明布尔值，并重算 `plan_digest`。
    ///
    /// 【调用者】持有真实 scope 的调用方在提交计划前用它替换自报布尔值。
    /// 【输出】成功时 `child_scope_is_subset` 等于实际求交结果，`parent_id_in_children` 等于
    /// child 列表是否真的包含 parent ID；随后仍应调用 `validate()`。
    /// 【失败】scope 无效/更宽时拒绝且不修改结构体。
    pub fn derive_declarations(
        &mut self,
        parent_scope: &ScopeSet,
        child_scope: &ScopeSet,
    ) -> Result<(), &'static str> {
        let subset = child_scope_within_parent(parent_scope, child_scope)?;
        if !subset {
            return Err(AUTOMATION_FANOUT_SCOPE_WIDENING);
        }
        self.child_scope_is_subset = true;
        self.parent_id_in_children = self.actual_parent_id_in_children();
        self.plan_digest = self.digest();
        Ok(())
    }

    /// 实际检查 child ID 列表是否包含 parent ID；不依赖调用方布尔声明。
    fn actual_parent_id_in_children(&self) -> bool {
        self.child_execution_ids
            .iter()
            .any(|child| child == &self.parent_execution_id)
    }

    /// 对除 plan_digest 外的全部计划字段计算内容摘要；不包含签名或可信来源证明。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "parent_execution_id": self.parent_execution_id,
            "child_execution_ids": self.child_execution_ids,
            "max_depth": self.max_depth,
            "depth": self.depth,
            "max_concurrency": self.max_concurrency,
            "requested_concurrency": self.requested_concurrency,
            "parent_budget_units": self.parent_budget_units,
            "child_budget_units": self.child_budget_units,
            "ttl_ms": self.ttl_ms,
            "parent_ttl_remaining_ms": self.parent_ttl_remaining_ms,
            "child_scope_is_subset": self.child_scope_is_subset,
            "parent_id_in_children": self.parent_id_in_children,
            "fail_fast": self.fail_fast,
            "fan_in_required": self.fan_in_required,
        }))
    }
}

/// 对外领域入口，仅转调 v1 DTO 校验器；当前没有生产调度路径调用它。
pub fn validate_automation_fanout(plan: &AutomationFanoutPlan) -> Result<(), &'static str> {
    plan.validate()
}

/// 父执行在 v2 计划中的真实边界：scope、预算、TTL、深度、并发与允许的 child role。
///
/// 这些字段仍由调用方声明，但 child 构造器会拿它们与每个 child 的真实 ScopeSet 比较；它们不是
/// ControlPlane 的授权决定，也不分配预算租约。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationFanoutParent {
    /// 父执行 ID；只检查文本形状，不查找真实运行记录。
    pub parent_execution_id: String,
    /// 父执行自身的角色文本；只用于计划记录，不参与权限推导。
    pub role: String,
    /// 允许 child 使用的角色允许列表；child role 不在其中即拒绝。
    pub allowed_child_roles: Vec<String>,
    /// 父执行真实 ScopeSet；child scope 必须与它求交后保持不变。
    pub scope: ScopeSet,
    /// 父计划可分配的总预算单位；必须大于 0。
    pub budget_units: u64,
    /// 父执行剩余 TTL（毫秒）；child TTL 不得超过它。
    pub ttl_remaining_ms: u64,
    /// 当前嵌套深度。
    pub depth: u32,
    /// 允许的最大嵌套深度；`depth` 必须严格小于它。
    pub max_depth: u32,
    /// 允许同时处于 started 状态的 child 上限。
    pub max_concurrency: u32,
    /// 是否在任一 child 失败后把未启动 child 标为 Skipped 并拒绝其启动。
    pub fail_fast: bool,
}

impl AutomationFanoutParent {
    /// 校验父计划字段与 ScopeSet 自洽性，不查询运行记录。
    ///
    /// 失败统一返回 `automation_fanout_parent_invalid`，深度已到上限返回
    /// `automation_fanout_depth_exceeded`，scope 损坏返回 `automation_fanout_scope_invalid`。
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_text(&self.parent_execution_id)
            || !valid_text(&self.role)
            || self.allowed_child_roles.is_empty()
            || self.allowed_child_roles.len() > AUTOMATION_FANOUT_MAX_CHILDREN
            || self
                .allowed_child_roles
                .iter()
                .any(|role| !valid_text(role))
            || self
                .allowed_child_roles
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != self.allowed_child_roles.len()
            || self.budget_units == 0
            || self.ttl_remaining_ms == 0
            || self.max_depth == 0
            || self.max_concurrency == 0
        {
            return Err(AUTOMATION_FANOUT_PARENT_INVALID);
        }
        self.scope
            .validate()
            .map_err(|_| AUTOMATION_FANOUT_SCOPE_INVALID)?;
        if self.depth >= self.max_depth {
            return Err(AUTOMATION_FANOUT_DEPTH_EXCEEDED);
        }
        Ok(())
    }
}

/// 一个 child 在 v2 计划中的真实绑定：身份、角色、ScopeSet、预算与 TTL。
///
/// 推荐通过 [`AutomationFanoutChildPlan::admit`] 构造；结构体字段公开以便 SerDe 往返，因此
/// [`AutomationFanoutPlanV2::validate`] 会重新执行同一组检查，不信任构造路径。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationFanoutChildPlan {
    /// child 执行 ID；必须唯一且不等于 parent ID。
    pub child_execution_id: String,
    /// child 角色；必须落在父计划允许列表内。
    pub role: String,
    /// child 真实 ScopeSet；必须不宽于父 scope。
    pub scope: ScopeSet,
    /// 该 child 申请的预算单位；必须大于 0 且不超过父预算。
    pub budget_units: u64,
    /// 该 child 申请的 TTL（毫秒）；必须大于 0 且不超过父剩余 TTL。
    pub ttl_ms: u64,
}

impl AutomationFanoutChildPlan {
    /// 用真实父 scope/预算/role/TTL 检查并构造一个 child 计划。
    ///
    /// 【核心流程】父计划自检 -> child ID 形状与 parent 循环 -> role 允许列表 -> 用
    /// `ScopeSet::intersect` 证明 child = child ∩ parent（等价于 `is_subset_of`）-> 单 child
    /// 预算与 TTL 上界。
    ///
    /// 【失败码】`automation_fanout_parent_invalid`、`automation_fanout_child_invalid`、
    /// `automation_fanout_cycle_refused`、`automation_fanout_role_widening`、
    /// `automation_fanout_scope_invalid`、`automation_fanout_scope_widening`、
    /// `automation_fanout_budget_exceeded`、`automation_fanout_ttl_exceeded`。
    /// 任一失败都不构造对象，也不产生任何副作用。
    pub fn admit(
        parent: &AutomationFanoutParent,
        child_execution_id: &str,
        role: &str,
        scope: ScopeSet,
        budget_units: u64,
        ttl_ms: u64,
    ) -> Result<Self, &'static str> {
        parent.validate()?;
        if !valid_text(child_execution_id) || !valid_text(role) {
            return Err(AUTOMATION_FANOUT_CHILD_INVALID);
        }
        if child_execution_id == parent.parent_execution_id {
            return Err(AUTOMATION_FANOUT_CYCLE_REFUSED);
        }
        if !parent
            .allowed_child_roles
            .iter()
            .any(|allowed| allowed == role)
        {
            return Err(AUTOMATION_FANOUT_ROLE_WIDENING);
        }
        if !child_scope_within_parent(&parent.scope, &scope)? {
            return Err(AUTOMATION_FANOUT_SCOPE_WIDENING);
        }
        if budget_units == 0 || budget_units > parent.budget_units {
            return Err(AUTOMATION_FANOUT_BUDGET_EXCEEDED);
        }
        if ttl_ms == 0 || ttl_ms > parent.ttl_remaining_ms {
            return Err(AUTOMATION_FANOUT_TTL_EXCEEDED);
        }
        Ok(Self {
            child_execution_id: child_execution_id.to_owned(),
            role: role.to_owned(),
            scope,
            budget_units,
            ttl_ms,
        })
    }
}

/// v2 扇出计划：一个父边界加一组已绑定真实 scope 的 child。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationFanoutPlanV2 {
    /// 必须等于 `AUTOMATION_FANOUT_PLAN_SCHEMA`。
    pub schema: String,
    /// 父执行边界。
    pub parent: AutomationFanoutParent,
    /// child 列表；1..=32、ID 唯一、不含 parent ID。
    pub children: Vec<AutomationFanoutChildPlan>,
    /// 对 schema/parent/children 的内容摘要；不是批准。
    pub plan_digest: String,
}

impl AutomationFanoutPlanV2 {
    /// 构造并校验 v2 计划；成功只表示领域边界成立，不创建或执行 child。
    pub fn new(
        parent: AutomationFanoutParent,
        children: Vec<AutomationFanoutChildPlan>,
    ) -> Result<Self, &'static str> {
        let mut plan = Self {
            schema: AUTOMATION_FANOUT_PLAN_SCHEMA.to_owned(),
            parent,
            children,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    /// 从头重新校验计划：父边界、child 数量/唯一性/循环、role、真实 scope 子集、预算求和
    /// （`checked_add`，溢出拒绝）与 TTL。
    ///
    /// 不信任构造路径，反序列化得到的计划同样必须通过。并发上限不在此处拒绝：计划可以接纳多于
    /// 并发上限的 child，由账本 `start_child` 在运行时把同时 Running 的数量限制在
    /// `parent.max_concurrency` 内，这样“全部接纳 + 有界执行 + fail-fast 跳过”可以同时成立。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_FANOUT_PLAN_SCHEMA {
            return Err(AUTOMATION_FANOUT_PARENT_INVALID);
        }
        self.parent.validate()?;
        if self.children.is_empty() || self.children.len() > AUTOMATION_FANOUT_MAX_CHILDREN {
            return Err(AUTOMATION_FANOUT_CHILD_COUNT_INVALID);
        }
        if self
            .children
            .iter()
            .map(|child| &child.child_execution_id)
            .collect::<BTreeSet<_>>()
            .len()
            != self.children.len()
        {
            return Err(AUTOMATION_FANOUT_CHILD_DUPLICATE);
        }
        let mut budget_total = 0u64;
        for child in &self.children {
            if !valid_text(&child.child_execution_id) || !valid_text(&child.role) {
                return Err(AUTOMATION_FANOUT_CHILD_INVALID);
            }
            if child.child_execution_id == self.parent.parent_execution_id {
                return Err(AUTOMATION_FANOUT_CYCLE_REFUSED);
            }
            if !self
                .parent
                .allowed_child_roles
                .iter()
                .any(|allowed| allowed == &child.role)
            {
                return Err(AUTOMATION_FANOUT_ROLE_WIDENING);
            }
            if !child_scope_within_parent(&self.parent.scope, &child.scope)? {
                return Err(AUTOMATION_FANOUT_SCOPE_WIDENING);
            }
            if child.budget_units == 0 {
                return Err(AUTOMATION_FANOUT_BUDGET_EXCEEDED);
            }
            budget_total = budget_total
                .checked_add(child.budget_units)
                .ok_or(AUTOMATION_FANOUT_BUDGET_EXCEEDED)?;
            if budget_total > self.parent.budget_units {
                return Err(AUTOMATION_FANOUT_BUDGET_EXCEEDED);
            }
            if child.ttl_ms == 0 || child.ttl_ms > self.parent.ttl_remaining_ms {
                return Err(AUTOMATION_FANOUT_TTL_EXCEEDED);
            }
        }
        if !valid_digest(&self.plan_digest) || self.plan_digest != self.digest() {
            return Err(AUTOMATION_FANOUT_CHILD_INVALID);
        }
        Ok(())
    }

    /// 计算 schema/parent/children 的内容摘要。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "parent": self.parent,
            "children": self.children,
        }))
    }

    /// 仅登记计划事实，返回一个所有 child 均为 Pending 的账本；不执行任何 child。
    pub fn admit(&self) -> Result<AutomationFanoutLedger, &'static str> {
        AutomationFanoutLedger::admit(self)
    }
}

/// child 在账本中的状态；只由账本内部转移，调用方不能直接设置。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationFanoutChildState {
    /// 已接纳但尚未启动。
    Pending,
    /// 已记录 started 事实。
    Running,
    /// 已记录成功结果。
    Succeeded,
    /// 已记录失败结果。
    Failed,
    /// fail-fast 后未启动即被跳过；永远不能再次启动。
    Skipped,
}

impl AutomationFanoutChildState {
    /// 是否为终态；Pending/Running 返回 false。
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Skipped)
    }
}

/// 账本记录的有序事实；sequence 单调递增，用于稳定合并。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationFanoutFact {
    /// child 已被启动。
    Started { child_id: String, sequence: u64 },
    /// child 已观察到成功结果。
    Succeeded {
        child_id: String,
        sequence: u64,
        output_digest: String,
    },
    /// child 已观察到失败结果。
    Failed {
        child_id: String,
        sequence: u64,
        failure_code: String,
    },
    /// child 未启动即被 fail-fast 跳过。
    Skipped {
        child_id: String,
        sequence: u64,
        reason: String,
    },
}

impl AutomationFanoutFact {
    /// 该事实的单调序号。
    pub fn sequence(&self) -> u64 {
        match self {
            Self::Started { sequence, .. }
            | Self::Succeeded { sequence, .. }
            | Self::Failed { sequence, .. }
            | Self::Skipped { sequence, .. } => *sequence,
        }
    }

    /// 该事实关联的 child ID。
    pub fn child_id(&self) -> &str {
        match self {
            Self::Started { child_id, .. }
            | Self::Succeeded { child_id, .. }
            | Self::Failed { child_id, .. }
            | Self::Skipped { child_id, .. } => child_id,
        }
    }
}

/// 一次 observe 的结果；只描述事实，不执行任何能力。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationFanoutChildOutcome {
    /// child 成功并给出输出摘要。
    Succeeded { output_digest: String },
    /// child 失败并给出稳定失败码。
    Failed { failure_code: String },
}

/// 父执行状态；每次由账本事实重新推导，没有可变 status 字段。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationFanoutParentState {
    /// 已接纳但没有任何 child 事实。
    Admitted,
    /// 有 child 已启动且尚无终态失败。
    Running,
    /// 至少一个 child 失败（含因此跳过的 child）。
    Failed,
    /// 全部 child 成功。
    Succeeded,
}

/// 从真实值推导出的接纳事实；两个布尔字段不是调用方输入。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationFanoutAdmission {
    /// 父执行 ID。
    pub parent_execution_id: String,
    /// 已接纳的 child ID 列表（保持计划顺序）。
    pub child_ids: Vec<String>,
    /// 每个 child scope 是否都被证明不宽于父 scope（由真实 ScopeSet 推导）。
    pub child_scope_is_subset: bool,
    /// child ID 列表是否实际包含 parent ID（由真实 ID 推导；接纳后必须为 false）。
    pub parent_id_in_children: bool,
    /// 计划内容摘要。
    pub plan_digest: String,
}

/// 一条 fan-in 证据输入；`(child_id, sequence)` 必须唯一。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationFanoutFanInInput {
    /// 引用的 child ID；必须已在账本中接纳。
    pub child_id: String,
    /// 该 child 内的证据序号；必须大于 0。
    pub sequence: u64,
    /// 证据摘要；只校验形状，不重建内容。
    pub evidence_digest: String,
}

/// 合并后的一条证据；输出按 `(child_id, sequence)` 字典序排列。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationFanoutMergeEntry {
    /// child ID。
    pub child_id: String,
    /// 证据序号。
    pub sequence: u64,
    /// 合并时该 child 的终态。
    pub state: AutomationFanoutChildState,
    /// 证据摘要。
    pub evidence_digest: String,
}

/// fan-in 的确定性结果与推导出的父状态。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationFanoutMerge {
    /// 父执行 ID。
    pub parent_execution_id: String,
    /// 由账本事实推导的父状态。
    pub parent_state: AutomationFanoutParentState,
    /// 稳定排序后的证据条目。
    pub entries: Vec<AutomationFanoutMergeEntry>,
    /// 对上述内容的摘要；不是执行/批准凭证。
    pub merge_digest: String,
}

#[derive(Clone, Debug)]
struct AutomationFanoutChildRecord {
    state: AutomationFanoutChildState,
    output_digest: Option<String>,
    failure_code: Option<String>,
}

/// 不执行任何 child 的扇出账本：只接纳计划、记录事实、推导父状态并做确定性合并。
///
/// 账本不持有 runtime、broker、clock 或事件存储；因此任何拒绝都只返回稳定错误码，不会产生
/// Broker/handler 副作用。它也不持久化；`parent_state()` 是当前内存事实的纯折叠。
#[derive(Clone, Debug)]
pub struct AutomationFanoutLedger {
    plan_digest: String,
    parent: AutomationFanoutParent,
    children: Vec<AutomationFanoutChildPlan>,
    records: BTreeMap<String, AutomationFanoutChildRecord>,
    facts: Vec<AutomationFanoutFact>,
    next_sequence: u64,
}

impl AutomationFanoutLedger {
    /// 校验计划并登记全部 child 为 Pending；不启动、不派发、不写事件。
    pub fn admit(plan: &AutomationFanoutPlanV2) -> Result<Self, &'static str> {
        plan.validate()?;
        let records = plan
            .children
            .iter()
            .map(|child| {
                (
                    child.child_execution_id.clone(),
                    AutomationFanoutChildRecord {
                        state: AutomationFanoutChildState::Pending,
                        output_digest: None,
                        failure_code: None,
                    },
                )
            })
            .collect();
        Ok(Self {
            plan_digest: plan.plan_digest.clone(),
            parent: plan.parent.clone(),
            children: plan.children.clone(),
            records,
            facts: Vec::new(),
            next_sequence: 1,
        })
    }

    /// 父计划边界。
    pub fn parent(&self) -> &AutomationFanoutParent {
        &self.parent
    }

    /// 已接纳的 child 计划（计划顺序）。
    pub fn children(&self) -> &[AutomationFanoutChildPlan] {
        &self.children
    }

    /// 计划摘要。
    pub fn plan_digest(&self) -> &str {
        &self.plan_digest
    }

    /// 已记录事实的只读视图。
    pub fn facts(&self) -> &[AutomationFanoutFact] {
        &self.facts
    }

    /// 查询 child 当前状态；未知 child 返回 None。
    pub fn child_state(&self, child_id: &str) -> Option<AutomationFanoutChildState> {
        self.records.get(child_id).map(|record| record.state)
    }

    /// 从真实值推导接纳事实：scope subset 由每个 child 与父 scope 求交得到，cycle 由真实 ID
    /// 列表得到。
    pub fn admission(&self) -> AutomationFanoutAdmission {
        let child_scope_is_subset = self.children.iter().all(|child| {
            matches!(
                child_scope_within_parent(&self.parent.scope, &child.scope),
                Ok(true)
            )
        });
        AutomationFanoutAdmission {
            parent_execution_id: self.parent.parent_execution_id.clone(),
            child_ids: self
                .children
                .iter()
                .map(|child| child.child_execution_id.clone())
                .collect(),
            child_scope_is_subset,
            parent_id_in_children: self
                .children
                .iter()
                .any(|child| child.child_execution_id == self.parent.parent_execution_id),
            plan_digest: self.plan_digest.clone(),
        }
    }

    /// 登记一个 child 的 started 事实并返回其 sequence。
    ///
    /// 拒绝未知 child（`_child_unknown`）、已终态/已启动（`_child_already_started`）、fail-fast
    /// 后跳过的 child（`_child_skipped`）以及超过父并发上限（`_concurrency_exceeded`）。
    /// 本函数不执行 child，只记录“已启动”事实。
    pub fn start_child(&mut self, child_id: &str) -> Result<u64, &'static str> {
        let Some(record) = self.records.get(child_id) else {
            return Err(AUTOMATION_FANOUT_CHILD_UNKNOWN);
        };
        match record.state {
            AutomationFanoutChildState::Skipped => return Err(AUTOMATION_FANOUT_CHILD_SKIPPED),
            AutomationFanoutChildState::Pending => {}
            _ => return Err(AUTOMATION_FANOUT_CHILD_ALREADY_STARTED),
        }
        let running = self
            .records
            .values()
            .filter(|record| record.state == AutomationFanoutChildState::Running)
            .count() as u64;
        if running >= u64::from(self.parent.max_concurrency) {
            return Err(AUTOMATION_FANOUT_CONCURRENCY_EXCEEDED);
        }
        if let Some(record) = self.records.get_mut(child_id) {
            record.state = AutomationFanoutChildState::Running;
        }
        let sequence = self.push_fact(|sequence| AutomationFanoutFact::Started {
            child_id: child_id.to_owned(),
            sequence,
        });
        Ok(sequence)
    }

    /// 记录一个已启动 child 的结果；fail-fast 时把其余 Pending child 按 ID 字典序标为 Skipped。
    ///
    /// 拒绝未知 child、未启动 child（`_child_not_started`）、重复 observe
    /// （`_child_already_started`）与无效摘要（`_evidence_invalid`）。Skipped 事实的 reason 固定为
    /// `automation_fanout_fail_fast_skip`，因此合并/回执可稳定复现。
    pub fn observe_child(
        &mut self,
        child_id: &str,
        outcome: AutomationFanoutChildOutcome,
    ) -> Result<(), &'static str> {
        let Some(record) = self.records.get(child_id) else {
            return Err(AUTOMATION_FANOUT_CHILD_UNKNOWN);
        };
        if record.state != AutomationFanoutChildState::Running {
            return Err(if record.state == AutomationFanoutChildState::Pending {
                AUTOMATION_FANOUT_CHILD_NOT_STARTED
            } else {
                AUTOMATION_FANOUT_CHILD_ALREADY_STARTED
            });
        }
        match &outcome {
            AutomationFanoutChildOutcome::Succeeded { output_digest } => {
                if !valid_digest(output_digest) {
                    return Err(AUTOMATION_FANOUT_EVIDENCE_INVALID);
                }
            }
            AutomationFanoutChildOutcome::Failed { failure_code } => {
                if !valid_text(failure_code) {
                    return Err(AUTOMATION_FANOUT_EVIDENCE_INVALID);
                }
            }
        }
        let failed = matches!(outcome, AutomationFanoutChildOutcome::Failed { .. });
        let record = self
            .records
            .get_mut(child_id)
            .expect("record checked above");
        match outcome {
            AutomationFanoutChildOutcome::Succeeded { output_digest } => {
                record.state = AutomationFanoutChildState::Succeeded;
                record.output_digest = Some(output_digest.clone());
                self.push_fact(|sequence| AutomationFanoutFact::Succeeded {
                    child_id: child_id.to_owned(),
                    sequence,
                    output_digest,
                });
            }
            AutomationFanoutChildOutcome::Failed { failure_code } => {
                record.state = AutomationFanoutChildState::Failed;
                record.failure_code = Some(failure_code.clone());
                self.push_fact(|sequence| AutomationFanoutFact::Failed {
                    child_id: child_id.to_owned(),
                    sequence,
                    failure_code,
                });
            }
        }
        if failed && self.parent.fail_fast {
            let skipped: Vec<String> = self
                .records
                .iter()
                .filter(|(_, record)| record.state == AutomationFanoutChildState::Pending)
                .map(|(child_id, _)| child_id.clone())
                .collect();
            for skipped_child in skipped {
                if let Some(record) = self.records.get_mut(&skipped_child) {
                    record.state = AutomationFanoutChildState::Skipped;
                }
                self.push_fact(|sequence| AutomationFanoutFact::Skipped {
                    child_id: skipped_child.clone(),
                    sequence,
                    reason: "automation_fanout_fail_fast_skip".to_owned(),
                });
            }
        }
        Ok(())
    }

    /// 由已记录事实推导父状态：无事实为 Admitted，任一 child Failed 为 Failed，全部 Succeeded
    /// 为 Succeeded，其余为 Running。
    ///
    /// 调用方不能设置或覆写该状态；它每次都是记录的纯函数。
    pub fn parent_state(&self) -> AutomationFanoutParentState {
        if self
            .records
            .values()
            .any(|record| record.state == AutomationFanoutChildState::Failed)
        {
            return AutomationFanoutParentState::Failed;
        }
        if self.facts.is_empty() {
            return AutomationFanoutParentState::Admitted;
        }
        if self
            .records
            .values()
            .all(|record| record.state == AutomationFanoutChildState::Succeeded)
        {
            return AutomationFanoutParentState::Succeeded;
        }
        AutomationFanoutParentState::Running
    }

    /// 按 `(child_id, sequence)` 字典序稳定合并 child 结果/证据。
    ///
    /// 【拒绝】输入 `(child_id, sequence)` 重复（`_fan_in_duplicate`）、引用未知 child
    /// （`_fan_in_unknown_child`）、已接纳 child 完全没有输入（`_fan_in_missing_child`）、child
    /// 尚未进入终态（`_fan_in_incomplete`）、sequence 为 0（`_fan_in_sequence_invalid`）或摘要
    /// 形状无效（`_evidence_invalid`）。任一拒绝都不产生合并结果，也没有外部副作用。
    ///
    /// 【输出】条目按 (child_id, sequence) 排序，父状态由事实推导；相同事实重复调用得到相同
    /// `merge_digest`。
    pub fn merge(
        &self,
        inputs: &[AutomationFanoutFanInInput],
    ) -> Result<AutomationFanoutMerge, &'static str> {
        let mut seen = BTreeSet::new();
        for input in inputs {
            if input.sequence == 0 {
                return Err(AUTOMATION_FANOUT_FAN_IN_SEQUENCE_INVALID);
            }
            if !valid_digest(&input.evidence_digest) {
                return Err(AUTOMATION_FANOUT_EVIDENCE_INVALID);
            }
            if !seen.insert((input.child_id.clone(), input.sequence)) {
                return Err(AUTOMATION_FANOUT_FAN_IN_DUPLICATE);
            }
            if !self.records.contains_key(&input.child_id) {
                return Err(AUTOMATION_FANOUT_FAN_IN_UNKNOWN_CHILD);
            }
        }
        for child in &self.children {
            let state = self
                .records
                .get(&child.child_execution_id)
                .map(|record| record.state)
                .ok_or(AUTOMATION_FANOUT_FAN_IN_MISSING_CHILD)?;
            if !inputs
                .iter()
                .any(|input| input.child_id == child.child_execution_id)
            {
                return Err(AUTOMATION_FANOUT_FAN_IN_MISSING_CHILD);
            }
            if !state.is_terminal() {
                return Err(AUTOMATION_FANOUT_FAN_IN_INCOMPLETE);
            }
        }
        let mut entries: Vec<AutomationFanoutMergeEntry> = inputs
            .iter()
            .map(|input| AutomationFanoutMergeEntry {
                child_id: input.child_id.clone(),
                sequence: input.sequence,
                state: self
                    .records
                    .get(&input.child_id)
                    .map(|record| record.state)
                    .unwrap_or(AutomationFanoutChildState::Pending),
                evidence_digest: input.evidence_digest.clone(),
            })
            .collect();
        entries.sort_by(|left, right| {
            left.child_id
                .cmp(&right.child_id)
                .then(left.sequence.cmp(&right.sequence))
        });
        let parent_state = self.parent_state();
        let merge_digest = json_digest(&serde_json::json!({
            "parent_execution_id": self.parent.parent_execution_id,
            "parent_state": parent_state,
            "entries": entries,
        }));
        Ok(AutomationFanoutMerge {
            parent_execution_id: self.parent.parent_execution_id.clone(),
            parent_state,
            entries,
            merge_digest,
        })
    }

    /// 追加一条事实并推进单调序号；sequence 从 1 开始。
    fn push_fact(&mut self, build: impl FnOnce(u64) -> AutomationFanoutFact) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.facts.push(build(sequence));
        sequence
    }
}

/// 用真实 ScopeSet 证明 child 不宽于 parent：求交结果必须与 child 完全一致。
///
/// `ScopeSet::intersect` 对空允许集返回 `scope_intersection_empty`；此时回退到
/// `is_subset_of`，因为它能正确表达“空集是任何集合的子集”，而不会把拒绝误算成授权。
fn child_scope_within_parent(
    parent_scope: &ScopeSet,
    child_scope: &ScopeSet,
) -> Result<bool, &'static str> {
    parent_scope
        .validate()
        .map_err(|_| AUTOMATION_FANOUT_SCOPE_INVALID)?;
    child_scope
        .validate()
        .map_err(|_| AUTOMATION_FANOUT_SCOPE_INVALID)?;
    match child_scope.intersect(parent_scope) {
        Ok(intersection) => Ok(&intersection == child_scope),
        Err(code) if code == "scope_intersection_empty" => child_scope
            .is_subset_of(parent_scope)
            .map_err(|_| AUTOMATION_FANOUT_SCOPE_INVALID),
        Err(_) => Err(AUTOMATION_FANOUT_SCOPE_INVALID),
    }
}

/// 检查文本非空、最长 256 UTF-8 字节且不含 NUL/CR/LF；不验证执行 ID 是否存在。
fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.contains(['\0', '\n', '\r'])
}

/// 检查摘要文本格式，不核对它是否来自受信任计划或真实执行数据。
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

//! 本文件定义 AUT-19 父子 FanOut/FanIn 计划 DTO，并校验计划字段之间的静态边界。
//!
//! # 在系统中的位置
//!
//! 调用方准备 AutomationFanoutPlan
//!     -> validate() 检查父子 ID / 深度 / 并发 / 预算 / TTL / scope 声明 / 摘要
//!     -> Core facade 原样转发结果
//!
//! 当前未找到生产 Rust 调用者；这个 validator 不创建子任务、不限制真实并发、不分配预算、不做
//! fail-fast，也不汇总子结果。它与 `kiana-workflow::durable` 已有的 `WorkflowNodeKind::FanOut`
//! / `FanIn` DAG 节点是不同契约，未发现 durable planner 调用本文件。因此通过校验不代表 AUT-19
//! 的运行时 fan-out 已接入产品。
//!
//! 计划里的 scope/cycle 等布尔值由构造者提供；没有对应父子 scope 或 ID 解析上下文时，本文件只
//! 能检查布尔声明，不能证明其与外部执行身份相符。`plan_digest` 是无密钥内容摘要，不是批准。

use crate::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// FanOut 计划 DTO 的固定 wire schema 名称。
pub const AUTOMATION_FANOUT_SCHEMA: &str = "kiana.automation-fanout.v1";

/// 一份父执行展开为多个子执行的静态计划及其有限约束声明。
///
/// 结构体使用 `deny_unknown_fields`，SerDe 会拒绝多余 JSON 字段。字段经过 validator 检查后仍是
/// 请求方提供的声明；真正的调度器、权限系统和 budget lease 尚未在本文件中实现。
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
    /// 调用方声称每个 child scope 是 parent scope 的子集；本 DTO 无实际 scope 可供比较。
    pub child_scope_is_subset: bool,
    /// 调用方声称 child ID 中不包含 parent ID；validator 检查此标记，不与 ID 列表比较。
    pub parent_id_in_children: bool,
    /// 计划是否要求子任务失败时 fail-fast；此值只进入摘要，本文件不停止运行中的 siblings。
    pub fail_fast: bool,
    /// 必须为 true，表示调用方要求汇总 child 结果；本 validator 不实际执行 fan-in。
    pub fan_in_required: bool,
    /// 对其余计划字段的 SHA-256 内容摘要；可以被提供者自行重算，不是执行许可。
    pub plan_digest: String,
}

impl AutomationFanoutPlan {
    /// 校验 fan-out 计划的结构和声明边界，不调度任务、不分配真实 scope/budget。
    ///
    /// 子 ID 数量为 1..=32 且必须唯一；depth/max_depth、并发申请、预算、TTL 必须满足范围；还要求
    /// child_scope_is_subset=true、parent_id_in_children=false、fan_in_required=true 和摘要自洽。
    /// 文本 ID 上限为 256 UTF-8 字节，拒绝 NUL/CR/LF，但不拒绝所有内部空白。
    /// 任一检查失败都返回 `automation_fanout_plan_invalid`；成功只表示输入对象满足本函数规则。
    ///
    /// 注意：预算总量用 `saturating_mul()` 计算后再比较。若乘法溢出饱和为 `u64::MAX`，且父预算
    /// 也恰为 `u64::MAX`，当前 `>` 比较不会拒绝数学上超出表示范围的总额；这是静态边界问题，本
    /// 注释任务不改变算术行为。并发、fail_fast、scope 和 cycle 标记同样依赖调用方事实。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_FANOUT_SCHEMA
            || !valid_text(&self.parent_execution_id)
            || self.child_execution_ids.is_empty()
            || self.child_execution_ids.len() > 32
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
            || !self.child_scope_is_subset
            || self.parent_id_in_children
            || !self.fan_in_required
            || !valid_digest(&self.plan_digest)
            || self.plan_digest != self.digest()
        {
            return Err("automation_fanout_plan_invalid");
        }
        Ok(())
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

/// 对外领域入口，仅转调 DTO 校验器；当前没有生产调度路径调用它。
pub fn validate_automation_fanout(plan: &AutomationFanoutPlan) -> Result<(), &'static str> {
    plan.validate()
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

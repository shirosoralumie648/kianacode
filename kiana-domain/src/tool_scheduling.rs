//! # 文件职责
//!
//! 本文件把一次模型响应中的多个工具调用按顺序整理为有界只读组（bounded parallel-read group）
//! 和独占屏障（exclusive barrier），输出可校验的批次计划 `ToolBatchPlan`。它在
//! `kiana-domain` 中负责纯数据规划，不发送请求、不启动任务，也不授予能力。
//!
//! # 数据从哪里来、去哪里
//!
//! ```text
//! 模型返回有序 ModelToolCall 列表
//!   -> Runner 先校验调用形状
//!   -> plan_tool_batch() 读取当前 ToolCatalogSnapshot 的调度/resource 元数据
//!   -> 返回有序 ToolBatchPlan，或返回稳定错误
//!   -> Runner 当前只用它检查批次是否合法，然后丢弃 plan
//!   -> pending_tools 仍逐个发给 Core；Core 对每次请求单独授权
//! ```
//!
//! ⚠ 当前 Runner 调用点使用 `plan_tool_batch(...).map(|_| ())`，不会把返回的分组计划交给
//! 有界并发工作池（bounded worker pool）；Harness 通过 `emit_tool_request()` 发出队首调用，
//! 并在 `on_capability_result()` 等前一个结果回来后再发下一项。因此这里的 `ParallelRead`
//! 表示规划合同，不是当前已经发生实际并发的证据。H16 baseline 也将真实工作池留作后续工作。
//!
//! # 为什么要分组
//!
//! 相邻、同资源的只读调用可以被标记为一个有界候选组；写入或其他副作用操作各自形成独占
//! 屏障，阻止它与两侧的读取组被合并。即使未来消费者执行这些组，每条 CapabilityRequest
//! 仍必须回到 Core 独立完成授权依据（authority）、策略、审批和执行许可（permit）检查；
//! 组级元数据不能扩张兄弟调用权限。

use crate::{current_tool_catalog, ModelToolCall, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const TOOL_BATCH_PLAN_SCHEMA: &str = "kiana.tool-batch-plan.v1";
/// 批次计划的合同版本；序列化消费者据此识别字段语义。
pub const TOOL_BATCH_PLAN_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 一个计划组最多允许的全局并行度上限；单工具 descriptor 可以声明更低上限。
pub const TOOL_BATCH_MAX_PARALLELISM: u32 = 8;

/// 调度分组模式；Serde（Rust 的序列化/反序列化框架）将枚举名编码为 `parallel_read` / `exclusive`。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSchedulingMode {
    /// 可放入有界候选并行组的只读调用（ParallelRead）。
    ParallelRead,
    /// 必须独占一个组、与前后候选组分开的调用（Exclusive）。
    Exclusive,
}

/// 并行额度上限的三个来源：run、部署（deployment）与角色（role）。
///
/// H16 要求“并行额度取 run、部署与角色上限的交集”。这里只承载三个来源的声明值，
/// 由 `effective()` 求交集；它不读取任何真实配额账本，也不授予执行权。任何来源为 0
/// 表示该来源“不允许并发”，交集的保守结果是退化为串行（宽度 1），而不是拒绝整批。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBatchParallelismCap {
    /// run 级并发上限。
    pub run: u32,
    /// 部署级并发上限。
    pub deployment: u32,
    /// 角色级并发上限。
    pub role: u32,
}

impl ToolBatchParallelismCap {
    /// 记录三个来源的上限；不做裁剪，交集在 `effective()` 中计算。
    pub fn new(run: u32, deployment: u32, role: u32) -> Self {
        Self {
            run,
            deployment,
            role,
        }
    }

    /// 生效并行度 = 三个来源逐项取最小，再与全局上限取最小；结果至少为 1。
    ///
    /// 【调用者】`ToolBatchGroup::dispatch_width()` 用它把“组自身上限”和“环境上限”取交集。
    /// 【为什么下限是 1】额度为 0 表示该来源不允许额外并发，但批次仍要串行推进；把它当成
    /// “不许派发”会让整批停滞，混淆“额度收窄”和“拒绝执行”。
    pub fn effective(&self) -> u32 {
        self.run
            .min(self.deployment)
            .min(self.role)
            .min(TOOL_BATCH_MAX_PARALLELISM)
            .max(1)
    }
}

/// 计划中按源顺序展开的单个待派发调用。
///
/// H16 的历史必须“按 ordinal 形成模型历史”，因此这里给每个调用一个从 0 开始、与
/// assistant 输出顺序一致的 `ordinal`，并保留它所属的组和调度模式。它是描述数据，
/// 不是已授权请求。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBatchDispatchUnit {
    /// 从 0 开始的源序号（assistant 输出顺序）；模型历史按它排序，与完成顺序无关。
    pub ordinal: usize,
    /// 该调用所属的组号，与 `ToolBatchGroup::group_id` 对应。
    pub group_id: u32,
    /// 该调用所属组的调度模式（只读候选并行组或独占屏障）。
    pub mode: ToolSchedulingMode,
    /// provider 调用 ID；组内/组间都保持源顺序。
    pub call_id: String,
}

/// 计划中的连续调用组，以及该组允许的调度属性。
///
/// 这是用于 Runner/Core 交接的描述数据，不包含已授权请求或执行结果。Serde 负责结构与外部
/// JSON 的读写；`deny_unknown_fields` 防止消费者静默忽略尚未认识的合同字段。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBatchGroup {
    /// 计划内从 1 开始连续递增的组序号，用于保留组之间的先后关系。
    pub group_id: u32,
    /// 该组是只读候选并行组，还是单调用独占屏障。
    pub mode: ToolSchedulingMode,
    /// 工具目录声明的粗粒度资源键（resource）；这里只用于判断相邻调用是否可放在同组，不是资源授权。
    pub resources: Vec<String>,
    /// 该组允许收纳的最多调用数，不能超过全局并行度上限（maximum parallelism）8。
    pub max_parallelism: u32,
    /// 按模型输出源顺序（source order）记录的调用 ID；规划不会在组内重排。
    pub call_ids: Vec<String>,
}

impl ToolBatchGroup {
    /// 检查单个分组的标识、资源、调用 ID 和调度上限。
    ///
    /// 【调用者】`ToolBatchPlan::validate()` 在校验整个计划时调用。
    /// 【输入/输出】读取当前 group；通过返回 `Ok(())`，否则返回稳定错误字符串。
    /// 【失败情况】组号为 0、资源/调用列表为空或含空白项、parallelism 越界、只读调用数
    /// 超限、独占组不止一条调用、resource 重复或 call ID 重复都会拒绝。
    /// 【副作用】只读内存并创建临时集合检查去重，不执行工具、不更改组。
    /// 这里不检查组号是否连续，也不证明声明的 resource 是当前请求真正访问的资源；前者由
    /// `ToolBatchPlan::validate()` 负责，后者必须由 Core/Broker 的真实请求授权与 containment 负责。
    pub fn validate(&self) -> Result<(), String> {
        // 这些条件合并在一起描述单组的形状不变量：只读组可包含多项但不能超过上限，
        // 独占屏障必须恰好是一项；外层 plan 再检查组序号和跨组 call ID 唯一性。
        if self.group_id == 0
            || self.resources.is_empty()
            || self
                .resources
                .iter()
                .any(|resource| resource.trim().is_empty())
            || self.call_ids.is_empty()
            || self
                .call_ids
                .iter()
                .any(|call_id| call_id.trim().is_empty())
            || self.max_parallelism == 0
            || self.max_parallelism > TOOL_BATCH_MAX_PARALLELISM
            || self.call_ids.len() as u32 > self.max_parallelism
                && self.mode == ToolSchedulingMode::ParallelRead
            || self.call_ids.len() != 1 && self.mode == ToolSchedulingMode::Exclusive
        {
            return Err("tool_batch_group_invalid".to_owned());
        }
        // 组内资源键必须唯一；重复键不会增加访问权，只会让计划的等价比较变得含糊。
        let mut resources = BTreeSet::new();
        if self
            .resources
            .iter()
            .any(|resource| !resources.insert(resource.clone()))
        {
            return Err("tool_batch_group_resources_duplicate".to_owned());
        }
        // call id 在同一组内只能出现一次，避免同一模型调用被计划执行两遍。
        let mut calls = BTreeSet::new();
        if self
            .call_ids
            .iter()
            .any(|call_id| !calls.insert(call_id.clone()))
        {
            return Err("tool_batch_group_calls_duplicate".to_owned());
        }
        Ok(())
    }

    /// 该组在给定额度下的实际派发宽度：取组自身上限与生效额度较小值，至少为 1。
    ///
    /// 【调用者】Runner 侧的有界并行执行器在展开一个只读组时用它决定同时可派发的调用数。
    /// 【为什么独占组也是 1】`exclusive` 组的 `max_parallelism` 恒为 1，所以无论环境额度多大，
    /// 独占调用都单独执行，前后自然形成屏障。
    /// 【边界】它只把两个数字取交集，不检查授权、不启动线程，也不是资源锁。
    pub fn dispatch_width(&self, cap: ToolBatchParallelismCap) -> u32 {
        self.max_parallelism.min(cap.effective()).max(1)
    }
}

/// 一次 assistant 工具调用批次（tool-call batch）的有序分组结果。
///
/// Runner 在模型输出后构造该值并校验，但当前只用校验结果决定是否接收 batch；执行顺序仍由
/// Harness 的 pending queue 与 Core 的逐请求授权/交付流程控制。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBatchPlan {
    /// 计划的稳定 schema 名称。
    pub schema: String,
    /// 计划合同版本。
    pub version: SchemaVersion,
    /// 顺序分组；独占调用会作为单独组隔开前后调用。
    pub groups: Vec<ToolBatchGroup>,
}

impl ToolBatchPlan {
    /// 校验 plan 的合同版本、非空分组、连续序号和跨组调用唯一性。
    ///
    /// 每组的详细资源/模式/上限检查委托给 `ToolBatchGroup::validate()`。失败时返回首个
    /// 稳定错误；Runner 当前把该错误转成 run 的 Failed 事件，而不是继续发送该批请求。
    /// 该方法只校验计划数据，不会检查 Core 授权，也不会启动并发执行。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TOOL_BATCH_PLAN_SCHEMA
            || self.version != TOOL_BATCH_PLAN_VERSION
            || self.groups.is_empty()
        {
            return Err("tool_batch_plan_invalid".to_owned());
        }
        // 组序号从 1 开始且必须连续；该值让消费者能识别缺组、乱序或重排后的计划。
        let mut expected = 1u32;
        let mut calls = BTreeSet::new();
        for group in &self.groups {
            if group.group_id != expected {
                return Err("tool_batch_plan_group_order_invalid".to_owned());
            }
            // 每组先验证自身，再检查 call ID 是否已在前面的组出现过。
            group.validate()?;
            if group
                .call_ids
                .iter()
                .any(|call_id| !calls.insert(call_id.clone()))
            {
                return Err("tool_batch_plan_call_duplicate".to_owned());
            }
            // 饱和递增避免整数回绕成 0；若组数到达 u32 上界，后续重复序号仍会被拒绝。
            expected = expected.saturating_add(1);
        }
        Ok(())
    }

    /// 只遍历可并行只读组，保持原有组顺序；独占屏障由调用者另行处理。
    pub fn parallel_groups(&self) -> impl Iterator<Item = &ToolBatchGroup> {
        self.groups
            .iter()
            .filter(|group| group.mode == ToolSchedulingMode::ParallelRead)
    }

    /// 按组顺序把计划展开为逐调用派发单元，`ordinal` 保持源顺序（0 起）。
    ///
    /// 【调用者】有界并行执行器用它在派发前把“组级计划”与“源序号”绑定，结果再按 ordinal
    /// 回填模型历史。独占屏障不会打断 ordinal 的连续性：屏障前后的调用仍按声明顺序编号。
    /// 【失败情况】先校验整个计划；不满足合同（空组、组号断裂、call ID 重复等）时返回稳定
    /// 错误，且不做部分展开。
    /// 【副作用】只读内存并构造新向量，不执行工具、不访问文件/网络。
    pub fn dispatch_units(&self) -> Result<Vec<ToolBatchDispatchUnit>, String> {
        self.validate()?;
        let mut units = Vec::new();
        for group in &self.groups {
            for call_id in &group.call_ids {
                units.push(ToolBatchDispatchUnit {
                    ordinal: units.len(),
                    group_id: group.group_id,
                    mode: group.mode,
                    call_id: call_id.clone(),
                });
            }
        }
        Ok(units)
    }
}

/// 按模型输出顺序，把工具调用规划为只读组和独占屏障。
///
/// 【调用者】Runner 在 `validate_model_calls()` 通过后、将模型返回值加入 `pending_tools` 前调用。
/// 【输入】有序的 `ModelToolCall` 切片；每项的名称必须能在当前工具目录中找到，ID 不能为空。
/// 【输出】带 schema/version 的 `ToolBatchPlan`，组内和组间都保留源顺序（source order）。
/// 【失败情况】空批次、空 ID、未知工具、目录校验失败或生成计划不满足合同都会返回错误；
/// 当前 Runner 将错误变成 Failed 事件，整批不会继续入队。
/// 【核心流程】读取并校验当前 catalog → 按 descriptor 选 `ParallelRead` / `Exclusive` →
/// 只把相邻、同资源、尚未达到 descriptor 上限的只读调用追加到当前组 → 独占调用新建单项组 →
/// 对最终计划执行完整校验。
/// 【副作用】只在内存中构造计划，不执行任何能力、不访问文件/网络，也不修改输入切片。
/// 【设计原因】把调度意图与 authority 分开，使 future consumer 能看到写操作前后的边界，
/// 同时由 Core 对每次真实调用独立授权。返回 ParallelRead 组并不等于当前系统已并发执行它们。
/// 文档守卫还依赖保留短语 `exclusive call always drains the preceding group`，其含义是
/// “独占调用前先结束前一只读组，再单独形成屏障”。
pub fn plan_tool_batch(calls: &[ModelToolCall]) -> Result<ToolBatchPlan, String> {
    if calls.is_empty() {
        return Err("tool_batch_empty".to_owned());
    }
    // 规划只能使用服务端当前目录中的调度声明；不从模型参数信任 scheduling/resource 值。
    let catalog = current_tool_catalog();
    catalog.validate()?;
    let mut groups = Vec::new();
    for call in calls {
        if call.id.trim().is_empty() {
            return Err("tool_batch_call_id_required".to_owned());
        }
        let descriptor = catalog
            .descriptor(&call.name)
            .ok_or_else(|| format!("tool_unsupported:{}", call.name))?;
        let mode = if descriptor.scheduling == "parallel_read" {
            ToolSchedulingMode::ParallelRead
        } else {
            ToolSchedulingMode::Exclusive
        };
        // 同时服从单工具 descriptor 上限与本 planner 的全局上限，采用两者中更小的值。
        let max_parallelism = descriptor.max_parallelism.min(TOOL_BATCH_MAX_PARALLELISM);
        // 只有和当前组相邻、同属只读模式、资源声明完全相同且未满员的调用才能合并。
        // 任何写入/副作用项都会走下面的新组分支，自然打断前后的只读组。
        let can_append = mode == ToolSchedulingMode::ParallelRead
            && groups.last().is_some_and(|group: &ToolBatchGroup| {
                group.mode == ToolSchedulingMode::ParallelRead
                    && group.call_ids.len() < group.max_parallelism as usize
                    && group.resources == descriptor.resources
            });
        if can_append {
            groups
                .last_mut()
                .expect("parallel group was checked above")
                .call_ids
                .push(call.id.clone());
        } else {
            // groups 长度从 0 开始，+1 生成 plan 约定的一基连续 group_id。
            let group_id = groups.len() as u32 + 1;
            groups.push(ToolBatchGroup {
                group_id,
                mode,
                resources: descriptor.resources.clone(),
                max_parallelism,
                call_ids: vec![call.id.clone()],
            });
        }
    }
    let plan = ToolBatchPlan {
        schema: TOOL_BATCH_PLAN_SCHEMA.to_owned(),
        version: TOOL_BATCH_PLAN_VERSION,
        groups,
    };
    plan.validate()?;
    Ok(plan)
}

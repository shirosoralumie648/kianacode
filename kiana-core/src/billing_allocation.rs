//! BQ-19 成本归属（cost allocation）的 ControlPlane 准入边界。
//!
//! # 文件职责与数据流
//!
//! 成本归属不是 UI 自己计算的一行金额，而是绑定到运行身份、项目范围、用量事实和授权的
//! source fact。本文件位于 Domain 合同与 EventStore 写入之间：
//!
//! ```text
//! BQ-13 usage / server-owned AllocationScope
//!        -> CostAllocation + trusted scope/source project
//!        -> 【当前文件】validate + server binding + SharingGrant 检查
//!        -> immutable RuntimeEvent(cost.allocation)
//!        -> 既有 ControlPlane/EventStore 事务提交
//!        -> billing/query projection 消费事实
//! ```
//!
//! `allocation` 中来自 wire 的项目、组织、workflow、cell、run 等字段只是调用方声明；真正
//! 可信的 scope 和 source project 必须由服务端上下文传入并在这里逐项比较。跨项目归属还要
//! 有与 allocation 精确匹配、未过期、包含 `COST_ALLOCATION_OPERATION` 的 SharingGrant。
//!
//! # 副作用与证明边界
//!
//! `authorize`/`authorize_wire` 只做内存校验，`allocation_event` 只序列化并构造不可变
//! `RuntimeEvent`。本文件不会 append EventStore、结算账单、调用 provider、提交预算或自动
//! 选择 grant；这些动作仍由唯一的 DaemonHost -> ControlPlane 主链负责。成功表示“当前输入
//! 可以进入事件提交阶段”，不表示成本已经 durable、已开票或已经影响财务权威。

use kiana_domain::{
    AllocationScope, CostAllocation, ProjectId, RuntimeEvent, SharingGrant, SharingGrantRef,
    COST_ALLOCATION_EVENT,
};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
/// 成本归属准入失败的稳定错误分类。
///
/// 错误分类区分了 allocation 自身形状、服务端绑定、wire 声明、跨项目 grant 和事件编码/
/// metadata 失败。上层可以据此决定拒绝请求、记录审计或返回用户可理解的原因；本模块不把
/// 所有拒绝压成通用 I/O 错误，也不自动重试。
pub enum CostAllocationAdmissionError {
    /// Domain allocation 字段、金额、摘要或范围本身无效。
    #[error("cost_allocation_invalid:{0}")]
    AllocationInvalid(String),
    /// allocation 与服务端可信 scope/source project 不一致。
    #[error("cost_allocation_server_binding_invalid:{0}")]
    ServerBindingInvalid(String),
    /// wire 报告的 scope 与已知服务端上下文不一致。
    #[error("cost_allocation_wire_scope_invalid:{0}")]
    WireScopeInvalid(String),
    /// 跨项目 grant 缺失、过期、代次不匹配或操作范围不正确。
    #[error("cost_allocation_sharing_grant_invalid:{0}")]
    SharingGrantInvalid(String),
    /// 将合法 allocation 编码成 RuntimeEvent 或写入 stream metadata 失败。
    #[error("cost_allocation_event_invalid:{0}")]
    EventInvalid(String),
}

/// BQ-19 成本归属准入的无状态门面。
///
/// 它把“是否允许把这一条 allocation 变成事件”的判断固定在 Core/ControlPlane 侧，但不
/// 保存账本，也不拥有 EventStore。这样相同的 Domain 校验可以被 deny-first fixture 复用，
/// 同时避免 UI/adapter 绕过服务端 scope 创建看似合法的归属事实。
pub struct CostAllocationAdmission;

impl CostAllocationAdmission {
    /// 校验服务端绑定的成本归属是否可以进入提交阶段。
    ///
    /// 【调用者】
    /// 通常由 `ControlPlane::admit_cost_allocation` 或相同授权事务在构造 allocation event
    /// 前调用。调用者应已经知道服务端解析出的 `trusted_scope` 和 source project，而不是把
    /// 用户请求中的 scope 当作可信值。
    ///
    /// 【检查顺序】
    /// 1. `allocation.validate()` 检查领域字段和 digest；
    /// 2. `validate_server_binding` 比较 allocation scope/source project 与服务端上下文；
    /// 3. `authorize_sharing_grant` 判断同项目不应带 grant、跨项目必须带精确 active grant。
    /// `now_unix_ms` 用于过期判断，`authority_epoch` 防止使用旧授权世代。
    ///
    /// 【输出/失败】
    /// `Ok(())` 只代表输入可以继续走正常事件提交；失败返回带分类的错误，不会选备用 grant、
    /// 修改 allocation、写 EventLog 或重试。
    pub fn authorize(
        allocation: &CostAllocation,
        trusted_scope: &AllocationScope,
        trusted_source_project: ProjectId,
        sharing_grant: Option<&SharingGrant>,
        now_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<(), CostAllocationAdmissionError> {
        allocation
            .validate()
            .map_err(CostAllocationAdmissionError::AllocationInvalid)?;
        allocation
            .validate_server_binding(trusted_scope, trusted_source_project)
            .map_err(CostAllocationAdmissionError::ServerBindingInvalid)?;
        Self::authorize_sharing_grant(allocation, sharing_grant, now_unix_ms, authority_epoch)?;
        Ok(())
    }

    /// 在校验 allocation 前，先拒绝 wire scope 对服务端 scope 的任何身份漂移。
    ///
    /// wire 字段是客户端/网络声明，不是 ownership authority。只要 project、organization、
    /// workflow、cell 或 run 任一维度与 `trusted_scope` 不同，就在事件构造前失败；通过 wire
    /// 比较后仍会调用完整的 `authorize`，所以不能把“scope 相等”误解成 grant、金额或摘要都
    /// 已经合法。
    ///
    /// 该分层是故意的：先拒绝伪造身份，再复用同一份 server binding/grant 规则，避免直接
    /// 信任序列化请求里的所有权字段。
    pub fn authorize_wire(
        allocation: &CostAllocation,
        reported_scope: &AllocationScope,
        trusted_scope: &AllocationScope,
        trusted_source_project: ProjectId,
        sharing_grant: Option<&SharingGrant>,
        now_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<(), CostAllocationAdmissionError> {
        allocation
            .validate_wire_scope(reported_scope)
            .map_err(CostAllocationAdmissionError::WireScopeInvalid)?;
        Self::authorize(
            allocation,
            trusted_scope,
            trusted_source_project,
            sharing_grant,
            now_unix_ms,
            authority_epoch,
        )
    }

    /// 拒绝 wire 自报的全部归属字段：scope、source project 与 sharing grant 引用。
    ///
    /// 这是 `authorize_wire` 的完整版本：wire 报告被当作断言逐字段与 allocation 事实比较，
    /// 任一漂移都在事件成形前返回分类错误；通过后仍复用同一份服务端 scope/source project/
    /// grant 规则。调用方不要把 wire 报告改写成 allocation 再去授权。
    #[allow(clippy::too_many_arguments)]
    pub fn authorize_wire_attribution(
        allocation: &CostAllocation,
        reported_scope: &AllocationScope,
        reported_source_project: ProjectId,
        reported_grant: Option<&SharingGrantRef>,
        trusted_scope: &AllocationScope,
        trusted_source_project: ProjectId,
        sharing_grant: Option<&SharingGrant>,
        now_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<(), CostAllocationAdmissionError> {
        allocation
            .validate_wire_attribution(reported_scope, reported_source_project, reported_grant)
            .map_err(CostAllocationAdmissionError::WireScopeInvalid)?;
        Self::authorize(
            allocation,
            trusted_scope,
            trusted_source_project,
            sharing_grant,
            now_unix_ms,
            authority_epoch,
        )
    }

    /// 校验 allocation 是否有权使用给定的 SharingGrant。
    ///
    /// 同项目归属不需要 grant，反而带 grant 会被拒绝，防止把不必要的跨项目授权混入事实；
    /// 跨项目归属必须同时满足：调用方提供 grant、grant 自身有效、allocation 引用精确匹配、
    /// grant 在 `now_unix_ms` 和 `authority_epoch` 下 active，并显式允许成本归属操作。
    /// 通过后由 Domain 的 `validate_grant_binding` 单一规则复核引用/时效/操作名，失败只返回
    /// 分类错误，不消费 grant、不延长有效期，也不创建事件。
    fn authorize_sharing_grant(
        allocation: &CostAllocation,
        grant: Option<&SharingGrant>,
        now_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<(), CostAllocationAdmissionError> {
        if allocation.source_project_id == allocation.scope.project_id {
            if grant.is_some() {
                return Err(CostAllocationAdmissionError::SharingGrantInvalid(
                    "same_project_grant_unexpected".to_owned(),
                ));
            }
            return Ok(());
        }
        let Some(grant) = grant else {
            return Err(CostAllocationAdmissionError::SharingGrantInvalid(
                "cross_project_sharing_grant_required".to_owned(),
            ));
        };
        allocation
            .validate_grant_binding(grant, now_unix_ms, authority_epoch)
            .map_err(CostAllocationAdmissionError::SharingGrantInvalid)
    }

    /// 把已验证的 allocation 封装成一条 `cost.allocation` RuntimeEvent。
    ///
    /// 【输入】
    /// `request_id` 把事件归入当前请求，`sequence` 是该请求事件流中的序号，`allocation`
    /// 是待写入的成本归属事实。
    ///
    /// 【处理】
    /// 先重新调用 Domain `validate`，再用 serde 转成 JSON，最后通过 `RuntimeEvent::new` 创建
    /// 事件，并把 aggregate type 固定为 `cost_allocation`、aggregate ID 固定为 allocation ID、
    /// revision 固定为 allocation revision。重复验证是有意的：即使调用者此前已经准入，
    /// 事件边界仍不能接受一个后来被修改或 digest 失配的值。
    ///
    /// 【输出与副作用】
    /// 成功返回内存中的不可变事件；失败返回 `EventInvalid`，不会 append EventStore、扣款、
    /// 结算或触发下游 projection。真正提交必须由上层 ControlPlane 事务完成。
    pub fn allocation_event(
        request_id: kiana_domain::RequestId,
        sequence: u64,
        allocation: &CostAllocation,
    ) -> Result<RuntimeEvent, CostAllocationAdmissionError> {
        allocation
            .validate()
            .map_err(CostAllocationAdmissionError::AllocationInvalid)?;
        let data = serde_json::to_value(allocation)
            .map_err(|_| CostAllocationAdmissionError::EventInvalid("encode_failed".to_owned()))?;
        RuntimeEvent::new(request_id, sequence, COST_ALLOCATION_EVENT, data)
            .map(|event| {
                event.with_stream_metadata(
                    "cost_allocation",
                    allocation.allocation_id.to_string(),
                    allocation.revision,
                )
            })
            .map_err(|error| CostAllocationAdmissionError::EventInvalid(error.to_string()))
    }
}

impl super::ControlPlane {
    /// 以 `ControlPlane` 方法形式暴露 BQ-19 allocation 准入。
    ///
    /// 这个方法是授权边界的入口，而不是财务写入入口：它只复用 `CostAllocationAdmission`
    /// 的 server-owned scope、跨项目 grant、时间和 authority epoch 检查。返回 `Ok(())` 后，
    /// 调用方仍须在正常事件事务中调用 `allocation_event` 并提交 EventStore；这里不创建第二
    /// 条写入路径，也不把成本归属直接算入 billing ledger。
    pub fn admit_cost_allocation(
        &self,
        allocation: &CostAllocation,
        trusted_scope: &AllocationScope,
        trusted_source_project: ProjectId,
        sharing_grant: Option<&SharingGrant>,
        now_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<(), CostAllocationAdmissionError> {
        CostAllocationAdmission::authorize(
            allocation,
            trusted_scope,
            trusted_source_project,
            sharing_grant,
            now_unix_ms,
            authority_epoch,
        )
    }

    /// 以 `ControlPlane` 方法形式暴露带 wire 归属校验的准入。
    ///
    /// 与 `admit_cost_allocation` 相同，只是先在事件成形前拒绝 wire 自报的 scope/source
    /// project/grant 漂移；它仍然只做内存校验，不写事件、不消费审批、不结算账单。
    #[allow(clippy::too_many_arguments)]
    pub fn admit_cost_allocation_wire(
        &self,
        allocation: &CostAllocation,
        reported_scope: &AllocationScope,
        reported_source_project: ProjectId,
        reported_grant: Option<&SharingGrantRef>,
        trusted_scope: &AllocationScope,
        trusted_source_project: ProjectId,
        sharing_grant: Option<&SharingGrant>,
        now_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<(), CostAllocationAdmissionError> {
        CostAllocationAdmission::authorize_wire_attribution(
            allocation,
            reported_scope,
            reported_source_project,
            reported_grant,
            trusted_scope,
            trusted_source_project,
            sharing_grant,
            now_unix_ms,
            authority_epoch,
        )
    }
}

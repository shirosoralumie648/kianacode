//! Kiana 的确定性工作流状态定义。
//!
//! 本 crate 只描述一个小型状态机：给定当前状态与目标状态，判断这条边是否合法。它不
//! 调用模型、不执行能力、不持有审批，也不写事件账本，因此不能成为绕过
//! `kiana-core::ControlPlane` 的第二条执行路径。上层若采用这些状态，仍必须由控制面完成
//! 授权、生命周期推进和事实记录。
//!
//! 当前产品主路径尚未直接消费本 crate 的状态类型；这里提供的是可复用的源码契约，
//! 不能仅凭类型存在就声称某个入口已经强制该工作流。

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 一次通用工作流的最小生命周期状态。
///
/// 序列化时使用稳定的 `snake_case` 名称，便于写入 JSON 产物。状态机刻意不提供任意
/// 跳转或原地重入：调用方必须通过 [`Self::transition`] 验证每一条边，不能直接把一个
/// 较早状态覆盖为“已完成”。
pub enum WorkflowState {
    /// 请求已登记，但实际执行尚未开始。
    Requested,
    /// 请求正在受控执行，可能继续完成、失败、转为结果未知或暂停等待审批。
    Running,
    /// 执行因缺少显式批准而暂停；此状态本身不授予继续执行的权限。
    AwaitingApproval,
    /// 请求被策略、审批结果或其他不可继续条件阻断；这是终态。
    Blocked,
    /// 工作流按自身完成条件结束；这不自动等同于现实业务结果已验证。
    Completed,
    /// 已明确观察到执行失败；这是终态。
    Failed,
    /// 无法可靠判断副作用是否发生，例如取消与外部执行完成产生竞态；这是终态。
    ResultUnknown,
}

impl WorkflowState {
    /// 校验并返回下一状态，不合法时保留原状态并返回结构化错误。
    ///
    /// 合法路径只有：请求开始运行；请求在开始前被阻断；运行中等待审批或进入三个执行
    /// 终态；等待审批后恢复运行或被阻断。终态没有出边，`Running` 也不能倒退回
    /// `Requested`。函数为 `const`，因为判断完全由固定转移图决定，不依赖外部状态。
    pub const fn transition(self, next: Self) -> Result<Self, WorkflowError> {
        // 将完整允许边集中列出，使新增状态默认没有权限转移，符合 fail-closed 原则。
        let allowed = matches!(
            (self, next),
            (Self::Requested, Self::Running)
                | (Self::Requested, Self::Blocked)
                | (Self::Running, Self::AwaitingApproval)
                | (Self::Running, Self::Completed)
                | (Self::Running, Self::Failed)
                | (Self::Running, Self::ResultUnknown)
                | (Self::AwaitingApproval, Self::Running)
                | (Self::AwaitingApproval, Self::Blocked)
        );
        if allowed {
            Ok(next)
        } else {
            Err(WorkflowError::InvalidTransition {
                from: self,
                to: next,
            })
        }
    }

    /// 判断状态是否已经结束，结束后所有后续转移都必须被拒绝。
    ///
    /// `ResultUnknown` 也被视为终态，因为“拿不准”不能自动重跑并冒险重复产生副作用；
    /// 是否补偿或重新发起必须由上层创建新的、可审计的决策流程。
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Blocked | Self::Completed | Self::Failed | Self::ResultUnknown
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
/// 工作流状态机拒绝转移时返回的稳定错误。
///
/// 错误保留起点与目标状态，便于上层记录冲突事实。它只说明状态边非法，不应被入口层
/// 改写成通用 I/O 错误，也不意味着可以跳过检查后强制赋值。
pub enum WorkflowError {
    /// `from -> to` 不在冻结的允许边集合中。
    #[error("invalid_workflow_transition:{from:?}->{to:?}")]
    InvalidTransition {
        /// 尝试转移前的权威状态。
        from: WorkflowState,
        /// 调用方请求但未被接受的目标状态。
        to: WorkflowState,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_state_machine_edges_are_allowed() {
        assert_eq!(
            WorkflowState::Requested.transition(WorkflowState::Running),
            Ok(WorkflowState::Running)
        );
        assert_eq!(
            WorkflowState::Running.transition(WorkflowState::AwaitingApproval),
            Ok(WorkflowState::AwaitingApproval)
        );
        assert_eq!(
            WorkflowState::AwaitingApproval.transition(WorkflowState::Running),
            Ok(WorkflowState::Running)
        );
    }

    #[test]
    fn terminal_states_reject_all_transitions() {
        for terminal in [
            WorkflowState::Blocked,
            WorkflowState::Completed,
            WorkflowState::Failed,
            WorkflowState::ResultUnknown,
        ] {
            assert!(terminal.is_terminal());
            assert!(terminal.transition(WorkflowState::Running).is_err());
        }
    }

    #[test]
    fn running_cannot_skip_back_to_requested() {
        assert!(WorkflowState::Running
            .transition(WorkflowState::Requested)
            .is_err());
    }
}

//! Pure workflow state transitions for Kiana.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowState {
    Requested,
    Running,
    AwaitingApproval,
    Blocked,
    Completed,
    Failed,
    ResultUnknown,
}

impl WorkflowState {
    pub const fn transition(self, next: Self) -> Result<Self, WorkflowError> {
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

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Blocked | Self::Completed | Self::Failed | Self::ResultUnknown
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum WorkflowError {
    #[error("invalid_workflow_transition:{from:?}->{to:?}")]
    InvalidTransition {
        from: WorkflowState,
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

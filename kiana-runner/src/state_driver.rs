//! Pure Harness lifecycle driver.
//!
//! The driver owns only bounded state transitions. It never calls a model, Broker, EventLog or
//! filesystem. External IDs, time and effect observations arrive as `DriverInput`; callers then
//! perform the existing I/O and persist the resulting RunnerEvent facts through ControlPlane.

use kiana_domain::{InteractionId, RunId, StepId, TurnId};
use serde::{Deserialize, Serialize};

pub const RUN_FRAME_SCHEMA: &str = "kiana.harness-run-frame.v1";
pub const TURN_FRAME_SCHEMA: &str = "kiana.harness-turn-frame.v1";
pub const DEFAULT_MAILBOX_CAPACITY: u32 = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessPhase {
    Idle,
    Queued,
    Preparing,
    ModelPending,
    ToolPending,
    AwaitingApproval,
    AwaitingInput,
    Compacting,
    Cancelling,
    TurnFinished,
    RecoveryRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverTerminal {
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnFrame {
    pub schema: String,
    pub turn_id: Option<TurnId>,
    pub run_id: RunId,
    pub phase: HarnessPhase,
    pub terminal: Option<DriverTerminal>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunFrame {
    pub schema: String,
    pub run_id: RunId,
    pub turn: TurnFrame,
    pub phase: HarnessPhase,
    pub step_id: Option<StepId>,
    pub step: u32,
    pub pending_tools: u32,
    /// A pending question is bound to one interaction and is never an approval handle.
    #[serde(default)]
    pub pending_interaction_id: Option<InteractionId>,
    pub driver_owner: Option<String>,
    pub mailbox_capacity: u32,
    pub accepted_inputs: u64,
    pub terminal: Option<DriverTerminal>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum DriverInput {
    QueueInput,
    BeginTurn,
    ClaimDriver { owner: String },
    ReleaseDriver { owner: String },
    BeginStep { step_id: StepId, step: u32 },
    ModelOutput { tool_count: u32, complete: bool },
    ToolResult { effect_known: bool },
    AwaitApproval,
    AwaitClarification { interaction_id: InteractionId },
    AnswerClarification { interaction_id: InteractionId },
    RequestCancel,
    StopConfirmed,
    RecoverUnknown,
    Terminal { outcome: DriverTerminal },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverIntent {
    AcknowledgeInput,
    StartModel,
    DispatchTools,
    WaitForToolResult,
    WaitForApproval,
    WaitForInput,
    CancelPendingTools,
    Reconcile,
    EmitTerminal,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DriverError {
    #[error("harness_driver_invalid_transition:{from:?}->{to:?}")]
    InvalidTransition {
        from: HarnessPhase,
        to: HarnessPhase,
    },
    #[error("harness_driver_second_owner")]
    SecondOwner,
    #[error("harness_driver_owner_mismatch")]
    OwnerMismatch,
    #[error("harness_driver_mailbox_full")]
    MailboxFull,
    #[error("harness_driver_terminal_conflict")]
    TerminalConflict,
    #[error("harness_driver_step_invalid")]
    StepInvalid,
    #[error("harness_driver_clarification_identity_mismatch")]
    ClarificationIdentityMismatch,
    #[error("harness_driver_clarification_not_pending")]
    ClarificationNotPending,
    #[error("harness_driver_frame_invalid")]
    InvalidFrame,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DriverTransition {
    pub frame: RunFrame,
    pub intents: Vec<DriverIntent>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunDriver {
    pub frame: RunFrame,
}

impl RunDriver {
    pub fn new(run_id: RunId, turn_id: Option<TurnId>) -> Self {
        let turn = TurnFrame {
            schema: TURN_FRAME_SCHEMA.to_owned(),
            turn_id,
            run_id,
            phase: HarnessPhase::Idle,
            terminal: None,
        };
        Self {
            frame: RunFrame {
                schema: RUN_FRAME_SCHEMA.to_owned(),
                run_id,
                turn,
                phase: HarnessPhase::Idle,
                step_id: None,
                step: 0,
                pending_tools: 0,
                pending_interaction_id: None,
                driver_owner: None,
                mailbox_capacity: DEFAULT_MAILBOX_CAPACITY,
                accepted_inputs: 0,
                terminal: None,
            },
        }
    }

    pub fn transition(&mut self, input: DriverInput) -> Result<Vec<DriverIntent>, DriverError> {
        let transition = transition(&self.frame, input)?;
        self.frame = transition.frame;
        Ok(transition.intents)
    }

    pub fn validate(&self) -> Result<(), DriverError> {
        if self.frame.schema != RUN_FRAME_SCHEMA
            || self.frame.turn.schema != TURN_FRAME_SCHEMA
            || self.frame.run_id != self.frame.turn.run_id
            || self.frame.step == 0 && self.frame.step_id.is_some()
            || self.frame.pending_tools > self.frame.mailbox_capacity
            || (self.frame.phase == HarnessPhase::AwaitingInput)
                != self.frame.pending_interaction_id.is_some()
            || (self.frame.phase != HarnessPhase::AwaitingInput
                && self.frame.pending_interaction_id.is_some())
        {
            return Err(DriverError::InvalidFrame);
        }
        Ok(())
    }

    pub fn queue_input(&mut self) -> Result<(), DriverError> {
        self.transition(DriverInput::QueueInput).map(|_| ())
    }

    pub fn begin_turn(&mut self) -> Result<(), DriverError> {
        self.transition(DriverInput::BeginTurn).map(|_| ())
    }

    pub fn begin_step(&mut self, step_id: StepId, step: u32) -> Result<(), DriverError> {
        self.transition(DriverInput::BeginStep { step_id, step })
            .map(|_| ())
    }

    pub fn model_output(&mut self, tool_count: u32, complete: bool) -> Result<(), DriverError> {
        self.transition(DriverInput::ModelOutput {
            tool_count,
            complete,
        })
        .map(|_| ())
    }

    pub fn tool_result(&mut self, effect_known: bool) -> Result<(), DriverError> {
        self.transition(DriverInput::ToolResult { effect_known })
            .map(|_| ())
    }

    pub fn await_clarification(
        &mut self,
        interaction_id: InteractionId,
    ) -> Result<(), DriverError> {
        self.transition(DriverInput::AwaitClarification { interaction_id })
            .map(|_| ())
    }

    pub fn answer_clarification(
        &mut self,
        interaction_id: InteractionId,
    ) -> Result<(), DriverError> {
        self.transition(DriverInput::AnswerClarification { interaction_id })
            .map(|_| ())
    }

    pub fn claim(&mut self, owner: impl Into<String>) -> Result<(), DriverError> {
        self.transition(DriverInput::ClaimDriver {
            owner: owner.into(),
        })
        .map(|_| ())
    }

    pub fn release(&mut self, owner: impl Into<String>) -> Result<(), DriverError> {
        self.transition(DriverInput::ReleaseDriver {
            owner: owner.into(),
        })
        .map(|_| ())
    }

    pub fn request_cancel(&mut self) -> Result<(), DriverError> {
        self.transition(DriverInput::RequestCancel).map(|_| ())
    }
}

fn move_phase(
    frame: &mut RunFrame,
    phase: HarnessPhase,
    intents: &mut Vec<DriverIntent>,
) -> Result<(), DriverError> {
    if frame.phase == phase {
        return Ok(());
    }
    let allowed = matches!(
        (frame.phase, phase),
        (
            HarnessPhase::Idle,
            HarnessPhase::Queued | HarnessPhase::Preparing | HarnessPhase::Cancelling
        ) | (HarnessPhase::Queued, HarnessPhase::Preparing)
            | (HarnessPhase::Queued, HarnessPhase::Cancelling)
            | (HarnessPhase::Preparing, HarnessPhase::ModelPending)
            | (HarnessPhase::Preparing, HarnessPhase::Cancelling)
            | (HarnessPhase::ModelPending, HarnessPhase::ToolPending)
            | (HarnessPhase::ModelPending, HarnessPhase::TurnFinished)
            | (HarnessPhase::ToolPending, HarnessPhase::ModelPending)
            | (HarnessPhase::ToolPending, HarnessPhase::AwaitingApproval)
            | (HarnessPhase::ModelPending, HarnessPhase::AwaitingInput)
            | (HarnessPhase::AwaitingApproval, HarnessPhase::ModelPending)
            | (HarnessPhase::AwaitingInput, HarnessPhase::ModelPending)
            | (HarnessPhase::AwaitingApproval, HarnessPhase::Cancelling)
            | (HarnessPhase::AwaitingInput, HarnessPhase::Cancelling)
            | (HarnessPhase::ModelPending, HarnessPhase::Cancelling)
            | (HarnessPhase::ToolPending, HarnessPhase::Cancelling)
            | (HarnessPhase::Cancelling, HarnessPhase::TurnFinished)
            | (HarnessPhase::Cancelling, HarnessPhase::RecoveryRequired)
            | (HarnessPhase::TurnFinished, HarnessPhase::Preparing)
            | (HarnessPhase::ModelPending, HarnessPhase::RecoveryRequired)
            | (HarnessPhase::ToolPending, HarnessPhase::RecoveryRequired)
            | (HarnessPhase::Preparing, HarnessPhase::RecoveryRequired)
    );
    if !allowed {
        return Err(DriverError::InvalidTransition {
            from: frame.phase,
            to: phase,
        });
    }
    frame.phase = phase;
    frame.turn.phase = phase;
    intents.push(match phase {
        HarnessPhase::Preparing => DriverIntent::StartModel,
        HarnessPhase::ModelPending => DriverIntent::StartModel,
        HarnessPhase::ToolPending => DriverIntent::DispatchTools,
        HarnessPhase::AwaitingApproval => DriverIntent::WaitForApproval,
        HarnessPhase::AwaitingInput => DriverIntent::WaitForInput,
        HarnessPhase::Cancelling => DriverIntent::CancelPendingTools,
        HarnessPhase::RecoveryRequired => DriverIntent::Reconcile,
        HarnessPhase::TurnFinished => DriverIntent::EmitTerminal,
        _ => DriverIntent::AcknowledgeInput,
    });
    Ok(())
}

/// Pure state transition function. No I/O, clock, ID allocation or side effects occur here.
pub fn transition(frame: &RunFrame, input: DriverInput) -> Result<DriverTransition, DriverError> {
    let mut next = frame.clone();
    let mut intents = Vec::new();
    match input {
        DriverInput::QueueInput => {
            if next.accepted_inputs >= u64::from(next.mailbox_capacity) {
                return Err(DriverError::MailboxFull);
            }
            next.accepted_inputs += 1;
            if next.phase == HarnessPhase::Idle {
                move_phase(&mut next, HarnessPhase::Queued, &mut intents)?;
            }
            intents.push(DriverIntent::AcknowledgeInput);
        }
        DriverInput::BeginTurn => {
            next.accepted_inputs = 0;
            next.pending_tools = 0;
            next.step_id = None;
            next.pending_interaction_id = None;
            next.terminal = None;
            next.turn.terminal = None;
            move_phase(&mut next, HarnessPhase::Preparing, &mut intents)?;
        }
        DriverInput::ClaimDriver { owner } => {
            if owner.trim().is_empty() {
                return Err(DriverError::SecondOwner);
            }
            if let Some(current) = &next.driver_owner {
                if current != &owner {
                    return Err(DriverError::SecondOwner);
                }
            } else {
                next.driver_owner = Some(owner);
            }
        }
        DriverInput::ReleaseDriver { owner } => {
            if next.driver_owner.as_deref() != Some(owner.as_str()) {
                return Err(DriverError::OwnerMismatch);
            }
            next.driver_owner = None;
        }
        DriverInput::BeginStep { step_id, step } => {
            if step == 0 || (next.step != 0 && step <= next.step) {
                return Err(DriverError::StepInvalid);
            }
            next.step_id = Some(step_id);
            next.step = step;
            move_phase(&mut next, HarnessPhase::ModelPending, &mut intents)?;
        }
        DriverInput::ModelOutput {
            tool_count,
            complete,
        } => {
            if complete && tool_count != 0 {
                return Err(DriverError::InvalidTransition {
                    from: next.phase,
                    to: HarnessPhase::TurnFinished,
                });
            }
            if tool_count > 0 {
                next.pending_tools = tool_count;
                move_phase(&mut next, HarnessPhase::ToolPending, &mut intents)?;
            } else if complete {
                next.pending_tools = 0;
                next.terminal = Some(DriverTerminal::Completed);
                next.turn.terminal = next.terminal;
                move_phase(&mut next, HarnessPhase::TurnFinished, &mut intents)?;
            } else {
                move_phase(&mut next, HarnessPhase::ModelPending, &mut intents)?;
            }
        }
        DriverInput::ToolResult { effect_known } => {
            if !effect_known {
                move_phase(&mut next, HarnessPhase::RecoveryRequired, &mut intents)?;
            } else {
                next.pending_tools = next.pending_tools.saturating_sub(1);
                move_phase(&mut next, HarnessPhase::ModelPending, &mut intents)?;
            }
        }
        DriverInput::AwaitApproval => {
            move_phase(&mut next, HarnessPhase::AwaitingApproval, &mut intents)?;
        }
        DriverInput::AwaitClarification { interaction_id } => {
            if interaction_id.as_uuid().is_nil()
                || next.pending_interaction_id.is_some()
                || !matches!(next.phase, HarnessPhase::ModelPending)
            {
                return Err(DriverError::ClarificationIdentityMismatch);
            }
            next.pending_interaction_id = Some(interaction_id);
            move_phase(&mut next, HarnessPhase::AwaitingInput, &mut intents)?;
        }
        DriverInput::AnswerClarification { interaction_id } => {
            if next.phase != HarnessPhase::AwaitingInput
                || next.pending_interaction_id != Some(interaction_id)
            {
                return Err(DriverError::ClarificationNotPending);
            }
            next.pending_interaction_id = None;
            move_phase(&mut next, HarnessPhase::ModelPending, &mut intents)?;
        }
        DriverInput::RequestCancel => {
            next.pending_interaction_id = None;
            move_phase(&mut next, HarnessPhase::Cancelling, &mut intents)?;
        }
        DriverInput::StopConfirmed => {
            if next.phase != HarnessPhase::Cancelling {
                return Err(DriverError::InvalidTransition {
                    from: next.phase,
                    to: HarnessPhase::TurnFinished,
                });
            }
            next.terminal = Some(DriverTerminal::Cancelled);
            next.turn.terminal = next.terminal;
            next.pending_interaction_id = None;
            move_phase(&mut next, HarnessPhase::TurnFinished, &mut intents)?;
        }
        DriverInput::RecoverUnknown => {
            move_phase(&mut next, HarnessPhase::RecoveryRequired, &mut intents)?;
        }
        DriverInput::Terminal { outcome } => {
            if next.terminal.is_some_and(|current| current != outcome) {
                return Err(DriverError::TerminalConflict);
            }
            next.terminal = Some(outcome);
            next.turn.terminal = Some(outcome);
            next.pending_interaction_id = None;
            if next.phase != HarnessPhase::TurnFinished {
                next.phase = HarnessPhase::TurnFinished;
                next.turn.phase = HarnessPhase::TurnFinished;
                intents.push(DriverIntent::EmitTerminal);
            }
        }
    }
    Ok(DriverTransition {
        frame: next,
        intents,
    })
}

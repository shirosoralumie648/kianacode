//! SC-15: Runner-side Started/Stopped/Unknown cancellation lifecycle.
//!
//! These are pure state-driver fixtures. They pin the deny-first side of the cancellation
//! boundary: a cancel request must not start new model/tool work, a stop can only be declared
//! from the cancelling phase, an unconfirmed effect must enter recovery instead of success,
//! and a terminal outcome can never be rewritten. They do not perform I/O and do not prove that
//! a real process or provider stopped; that evidence lives in the Core stop-proof path
//! (`kiana-core/src/lifecycle.rs`) and its own fixtures.

use kiana_domain::{RunId, StepId};
use kiana_runner::{
    transition, DriverError, DriverInput, DriverIntent, DriverTerminal, HarnessPhase, RunDriver,
};

/// Bring a fresh driver to `ModelPending` (a run that has started and is waiting on the model).
fn model_pending() -> RunDriver {
    let mut driver = RunDriver::new(RunId::new(), None);
    driver.begin_turn().unwrap();
    driver.begin_step(StepId::new(), 1).unwrap();
    assert_eq!(driver.frame.phase, HarnessPhase::ModelPending);
    assert!(driver.frame.terminal.is_none());
    driver
}

/// Extract the (from, to) pair of a denied transition so assertions stay short and exact.
fn invalid_transition(error: DriverError) -> Option<(HarnessPhase, HarnessPhase)> {
    match error {
        DriverError::InvalidTransition { from, to } => Some((from, to)),
        _ => None,
    }
}

#[test]
fn started_then_cancelling_then_stopped_is_the_only_cancelled_path() {
    let driver = model_pending();

    // RequestCancel moves to Cancelling and asks the harness to stop pending tool work, but it
    // does not by itself claim the run stopped: there is still no terminal outcome.
    let cancelling = transition(&driver.frame, DriverInput::RequestCancel).unwrap();
    assert_eq!(cancelling.frame.phase, HarnessPhase::Cancelling);
    assert_eq!(cancelling.frame.terminal, None);
    let cancel_intents = &cancelling.intents;
    assert!(cancel_intents.contains(&DriverIntent::CancelPendingTools));

    // RecoverUnknown is the honest "we could not confirm the stop" branch and still writes no
    // success terminal.
    let unknown = transition(&cancelling.frame, DriverInput::RecoverUnknown).unwrap();
    assert_eq!(unknown.frame.phase, HarnessPhase::RecoveryRequired);
    assert_eq!(unknown.frame.terminal, None);
    assert!(unknown.intents.contains(&DriverIntent::Reconcile));

    // Only StopConfirmed writes the Cancelled terminal.
    let stopped = transition(&cancelling.frame, DriverInput::StopConfirmed).unwrap();
    assert_eq!(stopped.frame.phase, HarnessPhase::TurnFinished);
    assert_eq!(stopped.frame.terminal, Some(DriverTerminal::Cancelled));
    assert!(stopped.intents.contains(&DriverIntent::EmitTerminal));
}

#[test]
fn stopped_requires_the_cancelling_phase() {
    // A plain running turn cannot be declared stopped: StopConfirmed is only valid after a
    // cancel request, so a caller cannot fabricate a Cancelled outcome.
    let driver = model_pending();
    let denied = transition(&driver.frame, DriverInput::StopConfirmed).unwrap_err();
    assert_eq!(
        invalid_transition(denied),
        Some((HarnessPhase::ModelPending, HarnessPhase::TurnFinished))
    );
}

#[test]
fn cancel_does_not_start_a_second_request_or_dispatch_tools() {
    let driver = model_pending();
    let cancelling = transition(&driver.frame, DriverInput::RequestCancel).unwrap();
    assert_eq!(cancelling.frame.phase, HarnessPhase::Cancelling);

    // A repeated cancel is idempotent: same phase, no new intents, no new work.
    let repeated = transition(&cancelling.frame, DriverInput::RequestCancel).unwrap();
    assert_eq!(repeated.frame.phase, HarnessPhase::Cancelling);
    assert!(repeated.intents.is_empty());

    // A late model step after cancel is refused; no new driver iteration is admitted.
    let step = DriverInput::BeginStep {
        step_id: StepId::new(),
        step: 2,
    };
    let denied = transition(&cancelling.frame, step).unwrap_err();
    assert_eq!(
        invalid_transition(denied),
        Some((HarnessPhase::Cancelling, HarnessPhase::ModelPending))
    );

    // A late tool call after cancel is refused instead of dispatching new effects.
    let output = DriverInput::ModelOutput {
        tool_count: 1,
        complete: false,
    };
    let denied = transition(&cancelling.frame, output).unwrap_err();
    assert_eq!(
        invalid_transition(denied),
        Some((HarnessPhase::Cancelling, HarnessPhase::ToolPending))
    );
}

#[test]
fn unconfirmed_effect_enters_recovery_instead_of_success() {
    let driver = model_pending();
    let tools = DriverInput::ModelOutput {
        tool_count: 1,
        complete: false,
    };
    let tool_pending = transition(&driver.frame, tools).unwrap();
    assert_eq!(tool_pending.frame.phase, HarnessPhase::ToolPending);
    assert_eq!(tool_pending.frame.pending_tools, 1);

    // effect_known=false means the driver must reconcile, not settle as a known result.
    let unknown_result = DriverInput::ToolResult {
        effect_known: false,
    };
    let recovery = transition(&tool_pending.frame, unknown_result).unwrap();
    assert_eq!(recovery.frame.phase, HarnessPhase::RecoveryRequired);
    assert_eq!(recovery.frame.terminal, None);
    assert!(recovery.intents.contains(&DriverIntent::Reconcile));

    // A "model finished" message that arrives while the effect is still unknown cannot be
    // laundered into a Completed terminal.
    let finished = DriverInput::ModelOutput {
        tool_count: 0,
        complete: true,
    };
    let denied = transition(&recovery.frame, finished).unwrap_err();
    assert_eq!(
        invalid_transition(denied),
        Some((HarnessPhase::RecoveryRequired, HarnessPhase::TurnFinished))
    );

    // The only terminal the recovery frame may carry is Unknown.
    let terminal = DriverInput::Terminal {
        outcome: DriverTerminal::Unknown,
    };
    let unknown = transition(&recovery.frame, terminal).unwrap();
    assert_eq!(unknown.frame.terminal, Some(DriverTerminal::Unknown));
    assert_eq!(unknown.frame.phase, HarnessPhase::TurnFinished);
}

#[test]
fn recover_unknown_from_a_running_turn_keeps_the_result_unknown() {
    let driver = model_pending();
    let recovery = transition(&driver.frame, DriverInput::RecoverUnknown).unwrap();
    assert_eq!(recovery.frame.phase, HarnessPhase::RecoveryRequired);
    assert_eq!(recovery.frame.terminal, None);
    assert!(recovery.intents.contains(&DriverIntent::Reconcile));
}

#[test]
fn a_terminal_outcome_cannot_be_rewritten() {
    let driver = model_pending();
    let finished = DriverInput::ModelOutput {
        tool_count: 0,
        complete: true,
    };
    let completed = transition(&driver.frame, finished).unwrap();
    assert_eq!(completed.frame.terminal, Some(DriverTerminal::Completed));
    assert_eq!(completed.frame.phase, HarnessPhase::TurnFinished);

    // Completed cannot be relabelled Cancelled/Failed/Unknown: there is no second terminal.
    for outcome in [
        DriverTerminal::Cancelled,
        DriverTerminal::Failed,
        DriverTerminal::Unknown,
    ] {
        let terminal = DriverInput::Terminal { outcome };
        let denied = transition(&completed.frame, terminal).unwrap_err();
        assert!(matches!(denied, DriverError::TerminalConflict));
    }

    // Re-asserting the same outcome is idempotent and emits no second terminal intent.
    let again = DriverInput::Terminal {
        outcome: DriverTerminal::Completed,
    };
    let repeated = transition(&completed.frame, again).unwrap();
    assert_eq!(repeated.frame.terminal, Some(DriverTerminal::Completed));
    assert!(repeated.intents.is_empty());
}

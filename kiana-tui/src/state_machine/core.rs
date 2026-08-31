//! Core state machine traits and types

use std::fmt::Debug;

/// A state in the finite state machine.
///
/// States must be clonable for history tracking and comparable for validation.
pub trait State: Clone + Debug + PartialEq + Send + Sync {}

/// An event that triggers state transitions.
///
/// Events represent user input, system events, or timer triggers.
pub trait Event: Debug + Clone + Send + Sync {}

/// Result of a state transition attempt.
pub type TransitionResult<S> = Result<TransitionSuccess<S>, TransitionError>;

/// Successful transition with metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionSuccess<S: State> {
    /// The state before transition
    pub from: S,
    /// The state after transition
    pub to: S,
    /// The event that triggered the transition
    pub event_name: String,
    /// Whether any guards were evaluated
    pub guards_checked: usize,
    /// Whether any actions were executed
    pub actions_executed: usize,
}

/// Errors that can occur during state transitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionError {
    /// No transition defined for the current state and event
    NoTransitionDefined { state: String, event: String },
    /// Guard condition failed
    GuardFailed { guard_name: String, reason: String },
    /// Action execution failed
    ActionFailed { action_name: String, reason: String },
    /// Invalid state
    InvalidState { state: String, reason: String },
}

impl std::fmt::Display for TransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTransitionDefined { state, event } => {
                write!(f, "No transition from state '{state}' on event '{event}'")
            }
            Self::GuardFailed { guard_name, reason } => {
                write!(f, "Guard '{guard_name}' failed: {reason}")
            }
            Self::ActionFailed {
                action_name,
                reason,
            } => {
                write!(f, "Action '{action_name}' failed: {reason}")
            }
            Self::InvalidState { state, reason } => {
                write!(f, "Invalid state '{state}': {reason}")
            }
        }
    }
}

impl std::error::Error for TransitionError {}

/// A transition between two states.
///
/// Encapsulates the source state, target state, and transition logic.
pub trait Transition<S: State, E: Event> {
    /// Check if this transition can be taken from the given state.
    fn can_transition(&self, from: &S, event: &E) -> bool;

    /// Execute the transition, consuming the current state.
    fn transition(&self, from: S, event: E) -> TransitionResult<S>;

    /// Get the name of this transition for logging.
    fn name(&self) -> &str {
        "unnamed_transition"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    enum TestState {
        Idle,
        Active,
    }

    impl State for TestState {}

    #[derive(Clone, Debug)]
    enum TestEvent {
        Start,
        Stop,
    }

    impl Event for TestEvent {}

    #[test]
    fn test_transition_error_display() {
        let err = TransitionError::NoTransitionDefined {
            state: "Idle".to_string(),
            event: "Unknown".to_string(),
        };
        assert!(err.to_string().contains("No transition"));

        let err = TransitionError::GuardFailed {
            guard_name: "SessionExists".to_string(),
            reason: "No active session".to_string(),
        };
        assert!(err.to_string().contains("Guard"));
    }

    #[test]
    fn test_transition_success() {
        let success = TransitionSuccess {
            from: TestState::Idle,
            to: TestState::Active,
            event_name: "Start".to_string(),
            guards_checked: 2,
            actions_executed: 1,
        };

        assert_eq!(success.from, TestState::Idle);
        assert_eq!(success.to, TestState::Active);
    }
}

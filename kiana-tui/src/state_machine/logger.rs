//! Transition logging and auditing

use super::{Event, State};
use std::time::Instant;

/// A record of a state transition.
#[derive(Debug, Clone)]
pub struct TransitionLog<S: State, E: Event> {
    /// State before transition
    pub from: S,
    /// State after transition
    pub to: S,
    /// Event that triggered transition
    pub event: E,
    /// Timestamp of transition
    pub timestamp: Instant,
}

impl<S: State, E: Event> TransitionLog<S, E> {
    /// Create a new transition log entry.
    pub fn new(from: S, to: S, event: E) -> Self {
        Self {
            from,
            to,
            event,
            timestamp: Instant::now(),
        }
    }

    /// Get a human-readable description of this transition.
    pub fn describe(&self) -> String {
        format!("{:?} -> {:?} via {:?}", self.from, self.to, self.event)
    }
}

/// Logger for state transitions.
pub struct TransitionLogger {
    enabled: bool,
}

impl TransitionLogger {
    /// Create a new transition logger.
    pub fn new() -> Self {
        Self { enabled: true }
    }

    /// Log a transition.
    pub fn log<S: State, E: Event>(&self, transition: &TransitionLog<S, E>) {
        if self.enabled {
            tracing::info!(
                from = ?transition.from,
                to = ?transition.to,
                event = ?transition.event,
                elapsed_ms = transition.timestamp.elapsed().as_millis(),
                "State transition"
            );
        }
    }

    /// Enable or disable logging.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

impl Default for TransitionLogger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    enum TestState {
        A,
        B,
    }
    impl State for TestState {}

    #[derive(Clone, Debug)]
    enum TestEvent {
        Go,
    }
    impl Event for TestEvent {}

    #[test]
    fn test_transition_log_creation() {
        let log = TransitionLog::new(TestState::A, TestState::B, TestEvent::Go);
        assert_eq!(log.from, TestState::A);
        assert_eq!(log.to, TestState::B);
    }

    #[test]
    fn test_transition_log_describe() {
        let log = TransitionLog::new(TestState::A, TestState::B, TestEvent::Go);
        let desc = log.describe();
        assert!(desc.contains("A"));
        assert!(desc.contains("B"));
        assert!(desc.contains("Go"));
    }

    #[test]
    fn test_transition_logger() {
        let logger = TransitionLogger::new();
        let log = TransitionLog::new(TestState::A, TestState::B, TestEvent::Go);
        logger.log(&log); // Should not panic
    }

    #[test]
    fn test_transition_logger_disable() {
        let mut logger = TransitionLogger::new();
        logger.set_enabled(false);
        let log = TransitionLog::new(TestState::A, TestState::B, TestEvent::Go);
        logger.log(&log); // Should not panic even when disabled
    }
}

//! Actions executed on state entry/exit

use super::State;

/// Context for action execution.
pub struct ActionContext {
    // Add fields as needed for your application
}

/// An action to execute during state transition.
pub trait Action<S: State, C>: Send + Sync {
    /// Execute the action.
    ///
    /// Returns an error string if the action fails.
    fn execute(&self, state: &S, context: &mut C) -> Result<(), String>;

    /// Get the name of this action for logging.
    fn name(&self) -> &str;
}

/// Action that clears input buffer.
pub struct ClearInputAction;

impl<S: State, C> Action<S, C> for ClearInputAction {
    fn execute(&self, _state: &S, _context: &mut C) -> Result<(), String> {
        // In real implementation, would clear input buffer from context
        Ok(())
    }

    fn name(&self) -> &str {
        "ClearInput"
    }
}

/// Action that saves the previous state.
pub struct SaveStateAction;

impl<S: State, C> Action<S, C> for SaveStateAction {
    fn execute(&self, _state: &S, _context: &mut C) -> Result<(), String> {
        // In real implementation, would save state to context
        Ok(())
    }

    fn name(&self) -> &str {
        "SaveState"
    }
}

/// Action that focuses a UI element.
pub struct FocusAction {
    element: String,
}

impl FocusAction {
    /// Create a new focus action.
    pub fn new(element: impl Into<String>) -> Self {
        Self {
            element: element.into(),
        }
    }
}

impl<S: State, C> Action<S, C> for FocusAction {
    fn execute(&self, _state: &S, _context: &mut C) -> Result<(), String> {
        // In real implementation, would focus the specified element
        Ok(())
    }

    fn name(&self) -> &str {
        "Focus"
    }
}

/// Action that logs a message.
pub struct LogAction {
    message: String,
}

impl LogAction {
    /// Create a new log action.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl<S: State, C> Action<S, C> for LogAction {
    fn execute(&self, state: &S, _context: &mut C) -> Result<(), String> {
        tracing::debug!(
            state = ?state,
            message = %self.message,
            "Action log"
        );
        Ok(())
    }

    fn name(&self) -> &str {
        "Log"
    }
}

/// Composite action that executes multiple actions in sequence.
pub struct SequenceAction<S: State, C> {
    actions: Vec<Box<dyn Action<S, C>>>,
}

impl<S: State, C> SequenceAction<S, C> {
    /// Create a new sequence action.
    pub fn new(actions: Vec<Box<dyn Action<S, C>>>) -> Self {
        Self { actions }
    }
}

impl<S: State, C> Action<S, C> for SequenceAction<S, C> {
    fn execute(&self, state: &S, context: &mut C) -> Result<(), String> {
        for action in &self.actions {
            action.execute(state, context)?;
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "Sequence"
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

    struct TestContext;

    #[test]
    fn test_clear_input_action() {
        let action = ClearInputAction;
        let mut context = TestContext;
        let result = action.execute(&TestState::A, &mut context);
        assert!(result.is_ok());
        assert_eq!(
            <ClearInputAction as Action<TestState, TestContext>>::name(&action),
            "ClearInput"
        );
    }

    #[test]
    fn test_save_state_action() {
        let action = SaveStateAction;
        let mut context = TestContext;
        let result = action.execute(&TestState::A, &mut context);
        assert!(result.is_ok());
        assert_eq!(
            <SaveStateAction as Action<TestState, TestContext>>::name(&action),
            "SaveState"
        );
    }

    #[test]
    fn test_focus_action() {
        let action = FocusAction::new("command_palette");
        let mut context = TestContext;
        let result = action.execute(&TestState::A, &mut context);
        assert!(result.is_ok());
        assert_eq!(
            <FocusAction as Action<TestState, TestContext>>::name(&action),
            "Focus"
        );
    }

    #[test]
    fn test_log_action() {
        let action = LogAction::new("test message");
        let mut context = TestContext;
        let result = action.execute(&TestState::A, &mut context);
        assert!(result.is_ok());
        assert_eq!(
            <LogAction as Action<TestState, TestContext>>::name(&action),
            "Log"
        );
    }

    #[test]
    fn test_sequence_action_success() {
        let actions: Vec<Box<dyn Action<TestState, TestContext>>> =
            vec![Box::new(ClearInputAction), Box::new(SaveStateAction)];
        let sequence = SequenceAction::new(actions);
        let mut context = TestContext;
        let result = sequence.execute(&TestState::A, &mut context);
        assert!(result.is_ok());
    }

    struct FailingAction;

    impl<S: State, C> Action<S, C> for FailingAction {
        fn execute(&self, _state: &S, _context: &mut C) -> Result<(), String> {
            Err("Action failed".to_string())
        }

        fn name(&self) -> &str {
            "Failing"
        }
    }

    #[test]
    fn test_sequence_action_stops_on_failure() {
        let actions: Vec<Box<dyn Action<TestState, TestContext>>> = vec![
            Box::new(ClearInputAction),
            Box::new(FailingAction),
            Box::new(SaveStateAction),
        ];
        let sequence = SequenceAction::new(actions);
        let mut context = TestContext;
        let result = sequence.execute(&TestState::A, &mut context);
        assert!(result.is_err());
    }
}

//! State machine execution engine

use super::{
    Action, Event, Guard, State, TransitionError, TransitionLog, TransitionLogger,
    TransitionResult, TransitionSuccess,
};
use std::collections::{HashMap, VecDeque};
use std::marker::PhantomData;

/// Configuration for the state machine.
#[derive(Clone, Debug)]
pub struct StateMachineConfig {
    /// Maximum number of transitions to keep in history
    pub max_history: usize,
    /// Enable debug logging
    pub debug_logging: bool,
    /// Enable state validation after each transition
    pub validate_states: bool,
}

impl Default for StateMachineConfig {
    fn default() -> Self {
        Self {
            max_history: 100,
            debug_logging: false,
            validate_states: true,
        }
    }
}

/// A transition definition with guards and actions.
pub struct TransitionDefinition<S: State, E: Event, C> {
    /// Target state
    pub target: S,
    /// Guards that must pass for transition
    pub guards: Vec<Box<dyn Guard<S, C>>>,
    /// Entry actions to execute before transition
    pub entry_actions: Vec<Box<dyn Action<S, C>>>,
    /// Exit actions to execute after transition
    pub exit_actions: Vec<Box<dyn Action<S, C>>>,
    /// Human-readable name
    pub name: String,
    _phantom: PhantomData<E>,
}

impl<S: State, E: Event, C> TransitionDefinition<S, E, C> {
    /// Create a new transition definition.
    pub fn new(target: S, name: String) -> Self {
        Self {
            target,
            guards: Vec::new(),
            entry_actions: Vec::new(),
            exit_actions: Vec::new(),
            name,
            _phantom: PhantomData,
        }
    }

    /// Add a guard condition.
    pub fn with_guard(mut self, guard: Box<dyn Guard<S, C>>) -> Self {
        self.guards.push(guard);
        self
    }

    /// Add an entry action.
    pub fn with_entry_action(mut self, action: Box<dyn Action<S, C>>) -> Self {
        self.entry_actions.push(action);
        self
    }

    /// Add an exit action.
    pub fn with_exit_action(mut self, action: Box<dyn Action<S, C>>) -> Self {
        self.exit_actions.push(action);
        self
    }
}

/// State machine engine managing transitions and history.
pub struct StateMachine<S: State, E: Event, C> {
    /// Current state
    current: S,
    /// Previous state (for escape/undo)
    previous: Option<S>,
    /// Transition definitions
    transitions: HashMap<String, TransitionDefinition<S, E, C>>,
    /// Transition history
    history: VecDeque<TransitionLog<S, E>>,
    /// Configuration
    config: StateMachineConfig,
    /// Logger
    logger: TransitionLogger,
}

impl<S: State, E: Event, C> StateMachine<S, E, C> {
    /// Create a new state machine with initial state.
    pub fn new(initial: S) -> Self {
        Self::with_config(initial, StateMachineConfig::default())
    }

    /// Create a new state machine with configuration.
    pub fn with_config(initial: S, config: StateMachineConfig) -> Self {
        Self {
            current: initial,
            previous: None,
            transitions: HashMap::new(),
            history: VecDeque::with_capacity(config.max_history),
            config,
            logger: TransitionLogger::new(),
        }
    }

    /// Get the current state.
    pub fn current_state(&self) -> &S {
        &self.current
    }

    /// Get the previous state if any.
    pub fn previous_state(&self) -> Option<&S> {
        self.previous.as_ref()
    }

    /// Register a transition.
    pub fn add_transition(&mut self, from: S, event: E, definition: TransitionDefinition<S, E, C>) {
        let key = Self::make_key(&from, &event);
        self.transitions.insert(key, definition);
    }

    /// Attempt a state transition.
    pub fn transition(&mut self, event: E, context: &mut C) -> TransitionResult<S> {
        let from = self.current.clone();
        let key = Self::make_key(&from, &event);

        // Find transition definition
        let definition =
            self.transitions
                .get(&key)
                .ok_or_else(|| TransitionError::NoTransitionDefined {
                    state: format!("{:?}", from),
                    event: format!("{:?}", event),
                })?;

        // Check guards
        for guard in &definition.guards {
            let result = guard.check(&from, context);
            if !result.passed {
                return Err(TransitionError::GuardFailed {
                    guard_name: guard.name().to_string(),
                    reason: result.reason.unwrap_or_else(|| "Guard failed".to_string()),
                });
            }
        }

        let to = definition.target.clone();

        // Execute exit actions
        for action in &definition.exit_actions {
            if let Err(e) = action.execute(&from, context) {
                return Err(TransitionError::ActionFailed {
                    action_name: action.name().to_string(),
                    reason: e,
                });
            }
        }

        // Update state
        self.previous = Some(self.current.clone());
        self.current = to.clone();

        // Execute entry actions
        for action in &definition.entry_actions {
            if let Err(e) = action.execute(&to, context) {
                // Rollback on entry action failure
                self.current = from.clone();
                self.previous = None;
                return Err(TransitionError::ActionFailed {
                    action_name: action.name().to_string(),
                    reason: e,
                });
            }
        }

        // Log transition
        let log = TransitionLog::new(from.clone(), to.clone(), event.clone());
        if self.config.debug_logging {
            self.logger.log(&log);
        }

        // Add to history
        self.history.push_back(log);
        if self.history.len() > self.config.max_history {
            self.history.pop_front();
        }

        Ok(TransitionSuccess {
            from,
            to,
            event_name: format!("{:?}", event),
            guards_checked: definition.guards.len(),
            actions_executed: definition.entry_actions.len() + definition.exit_actions.len(),
        })
    }

    /// Get transition history.
    pub fn history(&self) -> &VecDeque<TransitionLog<S, E>> {
        &self.history
    }

    /// Clear transition history.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Return to previous state if available.
    pub fn back(&mut self) -> Result<S, TransitionError> {
        let previous = self
            .previous
            .take()
            .ok_or_else(|| TransitionError::InvalidState {
                state: format!("{:?}", self.current),
                reason: "No previous state available".to_string(),
            })?;

        let old_current = self.current.clone();
        self.current = previous.clone();
        self.previous = Some(old_current);

        Ok(previous)
    }

    /// Enable or disable debug logging.
    pub fn set_debug_logging(&mut self, enabled: bool) {
        self.config.debug_logging = enabled;
    }

    /// Make a transition key from state and event.
    fn make_key(state: &S, event: &E) -> String {
        format!("{:?}::{:?}", state, event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_machine::{AppEvent, AppState, GuardResult};

    struct TestContext {
        has_session: bool,
    }

    struct TestGuard;

    impl Guard<AppState, TestContext> for TestGuard {
        fn check(&self, _state: &AppState, context: &TestContext) -> GuardResult {
            GuardResult {
                passed: context.has_session,
                reason: Some("Test guard".to_string()),
            }
        }

        fn name(&self) -> &str {
            "TestGuard"
        }
    }

    #[test]
    fn test_state_machine_creation() {
        let sm: StateMachine<AppState, AppEvent, TestContext> = StateMachine::new(AppState::Normal);
        assert_eq!(sm.current_state(), &AppState::Normal);
        assert_eq!(sm.previous_state(), None);
    }

    #[test]
    fn test_transition_without_definition() {
        let mut sm: StateMachine<AppState, AppEvent, TestContext> =
            StateMachine::new(AppState::Normal);
        let mut context = TestContext { has_session: true };

        let result = sm.transition(AppEvent::CommandPaletteRequested, &mut context);
        assert!(result.is_err());
    }

    #[test]
    fn test_successful_transition() {
        let mut sm: StateMachine<AppState, AppEvent, TestContext> =
            StateMachine::new(AppState::Normal);
        let mut context = TestContext { has_session: true };

        let definition =
            TransitionDefinition::new(AppState::CommandPalette, "open_command_palette".to_string());
        sm.add_transition(
            AppState::Normal,
            AppEvent::CommandPaletteRequested,
            definition,
        );

        let result = sm.transition(AppEvent::CommandPaletteRequested, &mut context);
        assert!(result.is_ok());
        assert_eq!(sm.current_state(), &AppState::CommandPalette);
        assert_eq!(sm.previous_state(), Some(&AppState::Normal));
    }

    #[test]
    fn test_guard_blocks_transition() {
        let mut sm: StateMachine<AppState, AppEvent, TestContext> =
            StateMachine::new(AppState::Normal);
        let mut context = TestContext { has_session: false };

        let definition =
            TransitionDefinition::new(AppState::SessionList, "open_session_list".to_string())
                .with_guard(Box::new(TestGuard));

        sm.add_transition(AppState::Normal, AppEvent::SessionListRequested, definition);

        let result = sm.transition(AppEvent::SessionListRequested, &mut context);
        assert!(result.is_err());
        assert_eq!(sm.current_state(), &AppState::Normal);
    }

    #[test]
    fn test_back_to_previous_state() {
        let mut sm: StateMachine<AppState, AppEvent, TestContext> =
            StateMachine::new(AppState::Normal);
        let mut context = TestContext { has_session: true };

        let definition =
            TransitionDefinition::new(AppState::CommandPalette, "open_command_palette".to_string());
        sm.add_transition(
            AppState::Normal,
            AppEvent::CommandPaletteRequested,
            definition,
        );

        sm.transition(AppEvent::CommandPaletteRequested, &mut context)
            .unwrap();
        assert_eq!(sm.current_state(), &AppState::CommandPalette);

        let result = sm.back();
        assert!(result.is_ok());
        assert_eq!(sm.current_state(), &AppState::Normal);
    }

    #[test]
    fn test_history_tracking() {
        let mut sm: StateMachine<AppState, AppEvent, TestContext> =
            StateMachine::new(AppState::Normal);
        let mut context = TestContext { has_session: true };

        let definition =
            TransitionDefinition::new(AppState::CommandPalette, "open_command_palette".to_string());
        sm.add_transition(
            AppState::Normal,
            AppEvent::CommandPaletteRequested,
            definition,
        );

        assert_eq!(sm.history().len(), 0);

        sm.transition(AppEvent::CommandPaletteRequested, &mut context)
            .unwrap();
        assert_eq!(sm.history().len(), 1);
    }
}

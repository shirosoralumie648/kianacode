//! Integration tests for state machine

#[cfg(test)]
mod integration_tests {
    use crate::state_machine::*;

    struct TestContext {
        has_session: bool,
        overlay_active: bool,
    }

    #[test]
    fn test_full_workflow_command_palette() {
        let mut sm: StateMachine<AppState, AppEvent, TestContext> =
            StateMachine::new(AppState::Normal);
        let mut context = TestContext {
            has_session: true,
            overlay_active: false,
        };

        // Register transitions
        sm.add_transition(
            AppState::Normal,
            AppEvent::CommandPaletteRequested,
            AppTransitions::open_command_palette(),
        );
        sm.add_transition(
            AppState::CommandPalette,
            AppEvent::Escape,
            AppTransitions::return_to_normal(),
        );

        // Open command palette
        let result = sm.transition(AppEvent::CommandPaletteRequested, &mut context);
        assert!(result.is_ok());
        assert_eq!(sm.current_state(), &AppState::CommandPalette);

        // Escape back to normal
        let result = sm.transition(AppEvent::Escape, &mut context);
        assert!(result.is_ok());
        assert_eq!(sm.current_state(), &AppState::Normal);

        // Check history
        assert_eq!(sm.history().len(), 2);
    }

    #[test]
    fn test_invalid_transition_from_command_palette() {
        let mut sm: StateMachine<AppState, AppEvent, TestContext> =
            StateMachine::new(AppState::CommandPalette);
        let mut context = TestContext {
            has_session: true,
            overlay_active: false,
        };

        // Try to open command palette again (no transition defined)
        let result = sm.transition(AppEvent::CommandPaletteRequested, &mut context);
        assert!(result.is_err());
        assert_eq!(sm.current_state(), &AppState::CommandPalette);
    }

    #[test]
    fn test_state_validation() {
        let validator = AppStateValidator;

        let result = validator.validate(&AppState::Normal);
        assert!(result.valid);

        let result = validator.validate(&AppState::CommandPalette);
        assert!(result.valid);

        let result = validator.validate(&AppState::Dialog(DialogKind::Blocking));
        assert!(result.valid);
    }

    #[test]
    fn test_transition_logging() {
        let mut sm = StateMachine::with_config(
            AppState::Normal,
            StateMachineConfig {
                max_history: 10,
                debug_logging: true,
                validate_states: true,
            },
        );
        let mut context = TestContext {
            has_session: true,
            overlay_active: false,
        };

        sm.add_transition(
            AppState::Normal,
            AppEvent::HelpRequested,
            AppTransitions::open_help(),
        );

        sm.transition(AppEvent::HelpRequested, &mut context)
            .unwrap();

        assert_eq!(sm.history().len(), 1);
        let log = &sm.history()[0];
        assert_eq!(log.from, AppState::Normal);
        assert_eq!(log.to, AppState::Help);
    }

    #[test]
    fn test_back_navigation() {
        let mut sm: StateMachine<AppState, AppEvent, TestContext> =
            StateMachine::new(AppState::Normal);
        let mut context = TestContext {
            has_session: true,
            overlay_active: false,
        };

        sm.add_transition(
            AppState::Normal,
            AppEvent::HelpRequested,
            AppTransitions::open_help(),
        );

        // Go to help
        sm.transition(AppEvent::HelpRequested, &mut context)
            .unwrap();
        assert_eq!(sm.current_state(), &AppState::Help);

        // Go back
        let result = sm.back();
        assert!(result.is_ok());
        assert_eq!(sm.current_state(), &AppState::Normal);
    }

    #[test]
    fn test_multiple_transitions() {
        let mut sm: StateMachine<AppState, AppEvent, TestContext> =
            StateMachine::new(AppState::Normal);
        let mut context = TestContext {
            has_session: true,
            overlay_active: false,
        };

        // Register all transitions
        sm.add_transition(
            AppState::Normal,
            AppEvent::CommandPaletteRequested,
            AppTransitions::open_command_palette(),
        );
        sm.add_transition(
            AppState::Normal,
            AppEvent::ConfigEditRequested,
            AppTransitions::open_config_editor(),
        );
        sm.add_transition(
            AppState::Normal,
            AppEvent::HelpRequested,
            AppTransitions::open_help(),
        );

        // Test each transition
        sm.transition(AppEvent::CommandPaletteRequested, &mut context)
            .unwrap();
        assert_eq!(sm.current_state(), &AppState::CommandPalette);

        sm.back().unwrap();

        sm.transition(AppEvent::ConfigEditRequested, &mut context)
            .unwrap();
        assert_eq!(sm.current_state(), &AppState::ConfigEditing);

        sm.back().unwrap();

        sm.transition(AppEvent::HelpRequested, &mut context)
            .unwrap();
        assert_eq!(sm.current_state(), &AppState::Help);
    }
}

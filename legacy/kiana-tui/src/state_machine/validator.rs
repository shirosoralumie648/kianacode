//! State validation

use super::State;

/// Result of state validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationResult {
    /// Whether the state is valid
    pub valid: bool,
    /// Validation errors
    pub errors: Vec<String>,
    /// Validation warnings
    pub warnings: Vec<String>,
}

impl ValidationResult {
    /// Create a valid result with no errors.
    pub fn valid() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Create an invalid result with errors.
    pub fn invalid(errors: Vec<String>) -> Self {
        Self {
            valid: false,
            errors,
            warnings: Vec::new(),
        }
    }

    /// Add a warning to the result.
    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }

    /// Add multiple warnings to the result.
    pub fn with_warnings(mut self, warnings: Vec<String>) -> Self {
        self.warnings.extend(warnings);
        self
    }
}

/// A validator for state consistency.
pub trait StateValidator<S: State> {
    /// Validate the state.
    fn validate(&self, state: &S) -> ValidationResult;

    /// Get the name of this validator.
    fn name(&self) -> &str;
}

/// Validator for application state.
pub struct AppStateValidator;

impl StateValidator<super::AppState> for AppStateValidator {
    fn validate(&self, state: &super::AppState) -> ValidationResult {
        use super::AppState;

        match state {
            AppState::Normal => ValidationResult::valid(),
            AppState::CommandPalette => ValidationResult::valid(),
            AppState::ConfigEditing => ValidationResult::valid(),
            AppState::SessionList => ValidationResult::valid(),
            AppState::Overlay(overlay_state) => {
                // Validate overlay state
                use super::OverlayState;
                match overlay_state {
                    OverlayState::SearchHistory => ValidationResult::valid(),
                    OverlayState::Help => ValidationResult::valid(),
                    OverlayState::Dialog(_) => ValidationResult::valid(),
                }
            }
            AppState::Help => ValidationResult::valid(),
            AppState::Dialog(dialog_kind) => {
                // Validate dialog state
                use super::DialogKind;
                match dialog_kind {
                    DialogKind::Info => ValidationResult::valid(),
                    DialogKind::Confirm => ValidationResult::valid(),
                    DialogKind::Blocking => ValidationResult::valid(),
                    DialogKind::Error => ValidationResult::valid(),
                }
            }
        }
    }

    fn name(&self) -> &str {
        "AppStateValidator"
    }
}

/// Composite validator that runs multiple validators.
pub struct CompositeValidator<S: State> {
    validators: Vec<Box<dyn StateValidator<S>>>,
}

impl<S: State> CompositeValidator<S> {
    /// Create a new composite validator.
    pub fn new(validators: Vec<Box<dyn StateValidator<S>>>) -> Self {
        Self { validators }
    }
}

impl<S: State> StateValidator<S> for CompositeValidator<S> {
    fn validate(&self, state: &S) -> ValidationResult {
        let mut all_errors = Vec::new();
        let mut all_warnings = Vec::new();

        for validator in &self.validators {
            let result = validator.validate(state);
            if !result.valid {
                all_errors.extend(result.errors);
            }
            all_warnings.extend(result.warnings);
        }

        if all_errors.is_empty() {
            ValidationResult::valid().with_warnings(all_warnings)
        } else {
            ValidationResult::invalid(all_errors).with_warnings(all_warnings)
        }
    }

    fn name(&self) -> &str {
        "CompositeValidator"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_machine::{AppState, DialogKind};

    #[test]
    fn test_validation_result_valid() {
        let result = ValidationResult::valid();
        assert!(result.valid);
        assert!(result.errors.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_validation_result_invalid() {
        let result = ValidationResult::invalid(vec!["error1".to_string(), "error2".to_string()]);
        assert!(!result.valid);
        assert_eq!(result.errors.len(), 2);
    }

    #[test]
    fn test_validation_result_with_warning() {
        let result = ValidationResult::valid().with_warning("warning1");
        assert!(result.valid);
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn test_app_state_validator_normal() {
        let validator = AppStateValidator;
        let result = validator.validate(&AppState::Normal);
        assert!(result.valid);
    }

    #[test]
    fn test_app_state_validator_command_palette() {
        let validator = AppStateValidator;
        let result = validator.validate(&AppState::CommandPalette);
        assert!(result.valid);
    }

    #[test]
    fn test_app_state_validator_dialog() {
        let validator = AppStateValidator;
        let result = validator.validate(&AppState::Dialog(DialogKind::Info));
        assert!(result.valid);
    }

    #[test]
    fn test_app_state_validator_name() {
        let validator = AppStateValidator;
        assert_eq!(validator.name(), "AppStateValidator");
    }

    struct AlwaysValidValidator;

    impl StateValidator<AppState> for AlwaysValidValidator {
        fn validate(&self, _state: &AppState) -> ValidationResult {
            ValidationResult::valid()
        }

        fn name(&self) -> &str {
            "AlwaysValid"
        }
    }

    struct AlwaysInvalidValidator;

    impl StateValidator<AppState> for AlwaysInvalidValidator {
        fn validate(&self, _state: &AppState) -> ValidationResult {
            ValidationResult::invalid(vec!["Always invalid".to_string()])
        }

        fn name(&self) -> &str {
            "AlwaysInvalid"
        }
    }

    #[test]
    fn test_composite_validator_all_valid() {
        let validator = CompositeValidator::new(vec![
            Box::new(AlwaysValidValidator),
            Box::new(AlwaysValidValidator),
        ]);
        let result = validator.validate(&AppState::Normal);
        assert!(result.valid);
    }

    #[test]
    fn test_composite_validator_one_invalid() {
        let validator = CompositeValidator::new(vec![
            Box::new(AlwaysValidValidator),
            Box::new(AlwaysInvalidValidator),
        ]);
        let result = validator.validate(&AppState::Normal);
        assert!(!result.valid);
        assert_eq!(result.errors.len(), 1);
    }
}

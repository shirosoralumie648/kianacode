//! Guard conditions for state transitions

use super::State;

/// Result of a guard check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardResult {
    /// Whether the guard passed
    pub passed: bool,
    /// Optional reason for failure
    pub reason: Option<String>,
}

impl GuardResult {
    /// Create a passing guard result.
    pub fn pass() -> Self {
        Self {
            passed: true,
            reason: None,
        }
    }

    /// Create a failing guard result with reason.
    pub fn fail(reason: impl Into<String>) -> Self {
        Self {
            passed: false,
            reason: Some(reason.into()),
        }
    }
}

/// A guard condition that determines if a transition can occur.
pub trait Guard<S: State, C>: Send + Sync {
    /// Check if the guard passes.
    fn check(&self, state: &S, context: &C) -> GuardResult;

    /// Get the name of this guard for logging.
    fn name(&self) -> &str;
}

/// Guard that checks if no overlay is active.
pub struct NoActiveOverlayGuard;

impl<S: State, C> Guard<S, C> for NoActiveOverlayGuard {
    fn check(&self, _state: &S, _context: &C) -> GuardResult {
        // This would check actual overlay state in real implementation
        GuardResult::pass()
    }

    fn name(&self) -> &str {
        "NoActiveOverlay"
    }
}

/// Guard that checks if a session exists.
pub struct SessionExistsGuard;

impl<S: State, C> Guard<S, C> for SessionExistsGuard {
    fn check(&self, _state: &S, _context: &C) -> GuardResult {
        // This would check actual session state in real implementation
        GuardResult::pass()
    }

    fn name(&self) -> &str {
        "SessionExists"
    }
}

/// Guard that always passes (for testing).
pub struct AlwaysGuard;

impl<S: State, C> Guard<S, C> for AlwaysGuard {
    fn check(&self, _state: &S, _context: &C) -> GuardResult {
        GuardResult::pass()
    }

    fn name(&self) -> &str {
        "Always"
    }
}

/// Guard that always fails (for testing).
pub struct NeverGuard;

impl<S: State, C> Guard<S, C> for NeverGuard {
    fn check(&self, _state: &S, _context: &C) -> GuardResult {
        GuardResult::fail("Never guard always fails")
    }

    fn name(&self) -> &str {
        "Never"
    }
}

/// Composite guard that requires all guards to pass (AND).
pub struct AllGuard<S: State, C> {
    guards: Vec<Box<dyn Guard<S, C>>>,
}

impl<S: State, C> AllGuard<S, C> {
    /// Create a new composite AND guard.
    pub fn new(guards: Vec<Box<dyn Guard<S, C>>>) -> Self {
        Self { guards }
    }
}

impl<S: State, C> Guard<S, C> for AllGuard<S, C> {
    fn check(&self, state: &S, context: &C) -> GuardResult {
        for guard in &self.guards {
            let result = guard.check(state, context);
            if !result.passed {
                return GuardResult::fail(format!(
                    "Guard '{}' failed: {}",
                    guard.name(),
                    result.reason.unwrap_or_default()
                ));
            }
        }
        GuardResult::pass()
    }

    fn name(&self) -> &str {
        "All"
    }
}

/// Composite guard that requires any guard to pass (OR).
pub struct AnyGuard<S: State, C> {
    guards: Vec<Box<dyn Guard<S, C>>>,
}

impl<S: State, C> AnyGuard<S, C> {
    /// Create a new composite OR guard.
    pub fn new(guards: Vec<Box<dyn Guard<S, C>>>) -> Self {
        Self { guards }
    }
}

impl<S: State, C> Guard<S, C> for AnyGuard<S, C> {
    fn check(&self, state: &S, context: &C) -> GuardResult {
        for guard in &self.guards {
            let result = guard.check(state, context);
            if result.passed {
                return GuardResult::pass();
            }
        }
        GuardResult::fail("No guard passed")
    }

    fn name(&self) -> &str {
        "Any"
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
    fn test_guard_result_pass() {
        let result = GuardResult::pass();
        assert!(result.passed);
        assert_eq!(result.reason, None);
    }

    #[test]
    fn test_guard_result_fail() {
        let result = GuardResult::fail("test reason");
        assert!(!result.passed);
        assert_eq!(result.reason, Some("test reason".to_string()));
    }

    #[test]
    fn test_always_guard() {
        let guard = AlwaysGuard;
        let result = guard.check(&TestState::A, &TestContext);
        assert!(result.passed);
    }

    #[test]
    fn test_never_guard() {
        let guard = NeverGuard;
        let result = guard.check(&TestState::A, &TestContext);
        assert!(!result.passed);
    }

    #[test]
    fn test_all_guard_passes_when_all_pass() {
        let guard = AllGuard::new(vec![Box::new(AlwaysGuard), Box::new(AlwaysGuard)]);
        let result = guard.check(&TestState::A, &TestContext);
        assert!(result.passed);
    }

    #[test]
    fn test_all_guard_fails_when_any_fails() {
        let guard = AllGuard::new(vec![Box::new(AlwaysGuard), Box::new(NeverGuard)]);
        let result = guard.check(&TestState::A, &TestContext);
        assert!(!result.passed);
    }

    #[test]
    fn test_any_guard_passes_when_one_passes() {
        let guard = AnyGuard::new(vec![Box::new(NeverGuard), Box::new(AlwaysGuard)]);
        let result = guard.check(&TestState::A, &TestContext);
        assert!(result.passed);
    }

    #[test]
    fn test_any_guard_fails_when_all_fail() {
        let guard = AnyGuard::new(vec![Box::new(NeverGuard), Box::new(NeverGuard)]);
        let result = guard.check(&TestState::A, &TestContext);
        assert!(!result.passed);
    }
}

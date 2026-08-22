//! Formal State Machine Framework for TUI Application
//!
//! Provides type-safe state transitions, guard conditions, entry/exit actions,
//! and auditable state history for managing application state.

mod actions;
mod app_states;
mod core;
mod engine;
mod guards;
mod logger;
#[cfg(test)]
mod tests;
mod transitions;
mod validator;

pub use actions::{Action, ActionContext, ClearInputAction, FocusAction, LogAction, SaveStateAction, SequenceAction};
pub use app_states::{AppState, AppEvent, DialogKind, OverlayState};
pub use core::{Event, State, Transition, TransitionError, TransitionResult, TransitionSuccess};
pub use engine::{StateMachine, StateMachineConfig, TransitionDefinition};
pub use guards::{AllGuard, AnyGuard, Guard, GuardResult, NoActiveOverlayGuard, SessionExistsGuard, AlwaysGuard, NeverGuard};
pub use logger::{TransitionLog, TransitionLogger};
pub use transitions::AppTransitions;
pub use validator::{AppStateValidator, CompositeValidator, StateValidator, ValidationResult};

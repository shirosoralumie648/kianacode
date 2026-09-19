//! Runner-facing facade for the shared bounded progress reducer.
//!
//! The reducer is data-only and has no authority.  Harness owns its checkpointed instance while
//! ControlPlane remains responsible for deciding whether a requested repair or clarification may
//! actually be admitted.

pub use kiana_domain::{
    failure_digest, ProgressAction, ProgressDecision, ProgressEvidence, ProgressInput,
    ProgressTracker, DEFAULT_NO_PROGRESS_LIMIT, DEFAULT_PROGRESS_WINDOW,
};

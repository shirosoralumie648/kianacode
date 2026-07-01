/// I/O dependency injection container for the query loop.
///
/// Corresponds to `deps.ts`. Passing a `QueryDeps` override into query parameters
/// lets tests inject fakes directly instead of mocking per-module — the pattern
/// avoids the import-spy boilerplate that test suites otherwise accumulate.
///
/// The scope is intentionally narrow (model call + compaction + UUID) to prove
/// the pattern; additional dependencies can be added incrementally.
use std::sync::Arc;
use uuid::Uuid;

/// Async function types used by the query loop.
///
/// These are `Arc`-wrapped to allow cheap cloning across the query loop
/// iterations while keeping trait-object overhead minimal.
pub struct QueryDeps {
    /// Generate a new random UUID (e.g., for tool-use IDs).
    pub uuid: Arc<dyn Fn() -> Uuid + Send + Sync>,
}

impl QueryDeps {
    /// Return a `QueryDeps` wired to real production implementations.
    pub fn production() -> Self {
        QueryDeps {
            uuid: Arc::new(Uuid::new_v4),
        }
    }

    /// Convenience: generate a new UUID via the injected factory.
    pub fn new_uuid(&self) -> Uuid {
        (self.uuid)()
    }
}

impl Default for QueryDeps {
    fn default() -> Self {
        Self::production()
    }
}

impl std::fmt::Debug for QueryDeps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryDeps").finish_non_exhaustive()
    }
}

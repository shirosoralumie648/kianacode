//! Query adapter for immutable source-snapshot invalidation plans.

use kiana_domain::{IndexInvalidationPlan, WorkspaceSnapshot};

pub fn plan_index_invalidation(
    previous: Option<&WorkspaceSnapshot>,
    current: &WorkspaceSnapshot,
    source_generation: u64,
    target_generation: u64,
) -> Result<IndexInvalidationPlan, String> {
    IndexInvalidationPlan::from_snapshots(previous, current, source_generation, target_generation)
}

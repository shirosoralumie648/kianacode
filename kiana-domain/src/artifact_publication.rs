//! Publication intent, reconciliation and workspace-delta contracts for immutable artifacts.
//!
//! A Company fact may cite an immutable artifact only after its bytes are committed. Committing
//! the blob and appending the EventLog fact remain two separate durable operations, so this
//! module records the publication window explicitly: an intent is written before the fact append
//! and resolved after it. An unresolved intent is reconciled against the fact log. A committed
//! blob without a fact is an orphan: it is reported for operator action and is never cited,
//! rewritten or deleted implicitly.
//!
//! These types carry identity, digests and a bounded textual delta only. They never carry blob
//! bytes, a workspace path that escapes the project root, or authority.

use crate::{json_digest, ArtifactRef, EventId, MAX_ARTIFACT_BYTES};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Component, Path};

pub const ARTIFACT_PUBLICATION_SCHEMA: &str = "kiana.artifact-publication.v1";
pub const ARTIFACT_WORKSPACE_DELTA_SCHEMA: &str = "kiana.artifact-workspace-delta.v1";
pub const ARTIFACT_ORPHAN_RETENTION_SCHEMA: &str = "kiana.artifact-publication-retention.v1";
/// Longest retention window an operator may declare before an orphan blob becomes reclaimable.
pub const MAX_ARTIFACT_ORPHAN_AGE_MS: u64 = 365 * 24 * 60 * 60 * 1000;
/// Largest number of orphans one reclamation run may reclaim.
pub const MAX_ARTIFACT_ORPHAN_COUNT: u32 = 4096;
const MAX_IDENTITY_BYTES: usize = 512;
const MAX_REASON_BYTES: usize = 256;
const MAX_LIMITATION_BYTES: usize = 512;
const MAX_LIMITATIONS: usize = 8;

fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let hex = value.strip_prefix("sha256:").unwrap_or(value);
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// A workspace-relative source path that cannot escape the project root.
fn relative_source_path(value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_IDENTITY_BYTES
        || value.contains('\0')
        || !Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err("artifact_workspace_delta_path_invalid".to_owned());
    }
    Ok(())
}

/// Lifecycle of one cross-store artifact publication attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactPublicationState {
    /// Blob bytes are committed; the Company fact that cites them is not confirmed yet.
    Pending,
    /// The Company fact carrying this reference was committed.
    Committed,
    /// Reconciliation proved no fact will commit this reference; the blob is left untouched.
    Abandoned,
}

/// Durable write-ahead record for one artifact publication.
///
/// The intent binds the exact immutable reference to the logical Company command that will cite
/// it, so a restart can decide whether the fact landed without re-deriving authority or content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactPublicationIntent {
    pub schema: String,
    pub command_id: String,
    pub idempotency_key: String,
    pub aggregate_id: String,
    pub artifact: ArtifactRef,
    pub manifest_digest: String,
    pub state: ArtifactPublicationState,
    pub recorded_at_unix_ms: u64,
    #[serde(default)]
    pub resolved_event_id: Option<EventId>,
    #[serde(default)]
    pub resolved_at_unix_ms: Option<u64>,
}

impl ArtifactPublicationIntent {
    pub fn new(
        command_id: impl Into<String>,
        idempotency_key: impl Into<String>,
        aggregate_id: impl Into<String>,
        artifact: ArtifactRef,
        recorded_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let intent = Self {
            schema: ARTIFACT_PUBLICATION_SCHEMA.to_owned(),
            command_id: command_id.into(),
            idempotency_key: idempotency_key.into(),
            aggregate_id: aggregate_id.into(),
            manifest_digest: json_digest(&serde_json::json!(&artifact)),
            artifact,
            state: ArtifactPublicationState::Pending,
            recorded_at_unix_ms,
            resolved_event_id: None,
            resolved_at_unix_ms: None,
        };
        intent.validate()?;
        Ok(intent)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ARTIFACT_PUBLICATION_SCHEMA || self.recorded_at_unix_ms == 0 {
            return Err("artifact_publication_invalid".to_owned());
        }
        bounded(
            &self.command_id,
            "artifact_publication_command_id",
            MAX_IDENTITY_BYTES,
        )?;
        bounded(
            &self.idempotency_key,
            "artifact_publication_idempotency_key",
            MAX_IDENTITY_BYTES,
        )?;
        bounded(
            &self.aggregate_id,
            "artifact_publication_aggregate_id",
            MAX_IDENTITY_BYTES,
        )?;
        self.artifact.validate()?;
        digest(
            &self.manifest_digest,
            "artifact_publication_manifest_digest",
        )?;
        if self.manifest_digest != json_digest(&serde_json::json!(&self.artifact)) {
            return Err("artifact_publication_manifest_mismatch".to_owned());
        }
        match self.state {
            ArtifactPublicationState::Pending => {
                if self.resolved_event_id.is_some() || self.resolved_at_unix_ms.is_some() {
                    return Err("artifact_publication_pending_resolved".to_owned());
                }
            }
            ArtifactPublicationState::Committed => {
                if self.resolved_event_id.is_none() {
                    return Err("artifact_publication_committed_event_missing".to_owned());
                }
                self.validate_resolution_time()?;
            }
            ArtifactPublicationState::Abandoned => {
                if self.resolved_event_id.is_some() {
                    return Err("artifact_publication_abandoned_event_present".to_owned());
                }
                self.validate_resolution_time()?;
            }
        }
        Ok(())
    }

    fn validate_resolution_time(&self) -> Result<(), String> {
        let resolved = self
            .resolved_at_unix_ms
            .ok_or_else(|| "artifact_publication_resolution_missing".to_owned())?;
        if resolved < self.recorded_at_unix_ms {
            return Err("artifact_publication_resolution_precedes_record".to_owned());
        }
        Ok(())
    }

    pub fn is_pending(&self) -> bool {
        self.state == ArtifactPublicationState::Pending
    }

    /// Produce the terminal form of this intent. Blob bytes and the recorded reference are kept
    /// byte-identical; only the resolution marker is added.
    pub fn resolved(
        &self,
        state: ArtifactPublicationState,
        event_id: Option<EventId>,
        resolved_at_unix_ms: u64,
    ) -> Result<Self, String> {
        if !self.is_pending() {
            return Err("artifact_publication_already_resolved".to_owned());
        }
        let mut resolved = self.clone();
        resolved.state = state;
        resolved.resolved_event_id = event_id;
        resolved.resolved_at_unix_ms = Some(resolved_at_unix_ms);
        resolved.validate()?;
        Ok(resolved)
    }
}

/// Reconciliation classification for a pending publication intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactPublicationVerdict {
    /// The fact is committed and the cited blob is readable back byte-identical.
    Published,
    /// The blob is readable but no fact cites it: an orphan committed blob.
    Unpublished,
    /// The fact is unconfirmed or the blob is unreadable; nothing may be assumed.
    Unverifiable,
}

/// Read-only reconciliation result for one intent. It never authorizes deleting a blob or
/// appending a fact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactPublicationReconciliation {
    pub command_id: String,
    pub artifact: ArtifactRef,
    pub verdict: ArtifactPublicationVerdict,
    pub fact_committed: bool,
    pub blob_readable: bool,
    #[serde(default)]
    pub event_id: Option<EventId>,
    pub reason: String,
}

impl ArtifactPublicationReconciliation {
    pub fn new(
        command_id: impl Into<String>,
        artifact: ArtifactRef,
        fact_committed: bool,
        blob_readable: bool,
        event_id: Option<EventId>,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let verdict = match (fact_committed, blob_readable) {
            (true, true) => ArtifactPublicationVerdict::Published,
            (false, true) => ArtifactPublicationVerdict::Unpublished,
            _ => ArtifactPublicationVerdict::Unverifiable,
        };
        let report = Self {
            command_id: command_id.into(),
            artifact,
            verdict,
            fact_committed,
            blob_readable,
            event_id: event_id.filter(|_| fact_committed),
            reason: reason.into(),
        };
        report.validate()?;
        Ok(report)
    }

    /// A verdict that can be written as a terminal intent state. `Unverifiable` must stay open.
    pub fn terminal_state(&self) -> Option<ArtifactPublicationState> {
        match self.verdict {
            ArtifactPublicationVerdict::Published => Some(ArtifactPublicationState::Committed),
            ArtifactPublicationVerdict::Unpublished => Some(ArtifactPublicationState::Abandoned),
            ArtifactPublicationVerdict::Unverifiable => None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        bounded(
            &self.command_id,
            "artifact_publication_command_id",
            MAX_IDENTITY_BYTES,
        )?;
        self.artifact.validate()?;
        bounded(
            &self.reason,
            "artifact_publication_reason",
            MAX_REASON_BYTES,
        )?;
        let expected = match (self.fact_committed, self.blob_readable) {
            (true, true) => ArtifactPublicationVerdict::Published,
            (false, true) => ArtifactPublicationVerdict::Unpublished,
            _ => ArtifactPublicationVerdict::Unverifiable,
        };
        if self.verdict != expected {
            return Err("artifact_publication_verdict_mismatch".to_owned());
        }
        if self.event_id.is_some() && !self.fact_committed {
            return Err("artifact_publication_event_without_fact".to_owned());
        }
        Ok(())
    }
}

/// Relationship between a committed historical artifact and the current workspace source.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactWorkspaceDeltaState {
    /// The current workspace bytes still hash to the committed reference.
    Unchanged,
    /// The workspace file exists but no longer matches the committed reference.
    Changed,
    /// The workspace file is absent.
    Missing,
    /// The workspace file could not be read under the confined reader.
    Unavailable,
}

/// Server-owned comparison of a committed artifact with its current workspace source.
///
/// `added_lines` / `removed_lines` are a bounded, deterministic line-level summary, not a patch;
/// the browser renders them and never computes a diff itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactWorkspaceDelta {
    pub schema: String,
    pub artifact: ArtifactRef,
    pub relative_path: String,
    pub historical_size_bytes: u64,
    pub current_state: ArtifactWorkspaceDeltaState,
    #[serde(default)]
    pub current_content_hash: Option<String>,
    #[serde(default)]
    pub current_size_bytes: Option<u64>,
    pub added_lines: u64,
    pub removed_lines: u64,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl ArtifactWorkspaceDelta {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        artifact: ArtifactRef,
        relative_path: impl Into<String>,
        historical_size_bytes: u64,
        current_state: ArtifactWorkspaceDeltaState,
        current_content_hash: Option<String>,
        current_size_bytes: Option<u64>,
        added_lines: u64,
        removed_lines: u64,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let delta = Self {
            schema: ARTIFACT_WORKSPACE_DELTA_SCHEMA.to_owned(),
            artifact,
            relative_path: relative_path.into(),
            historical_size_bytes,
            current_state,
            current_content_hash,
            current_size_bytes,
            added_lines,
            removed_lines,
            limitations,
        };
        delta.validate()?;
        Ok(delta)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ARTIFACT_WORKSPACE_DELTA_SCHEMA
            || self.historical_size_bytes > MAX_ARTIFACT_BYTES as u64
            || self.limitations.len() > MAX_LIMITATIONS
        {
            return Err("artifact_workspace_delta_invalid".to_owned());
        }
        self.artifact.validate()?;
        relative_source_path(&self.relative_path)?;
        for limitation in &self.limitations {
            bounded(
                limitation,
                "artifact_workspace_delta_limitation",
                MAX_LIMITATION_BYTES,
            )?;
        }
        if let Some(size) = self.current_size_bytes {
            if size > MAX_ARTIFACT_BYTES as u64 {
                return Err("artifact_workspace_delta_current_size_exceeded".to_owned());
            }
        }
        if let Some(hash) = self.current_content_hash.as_ref() {
            digest(hash, "artifact_workspace_delta_current_hash")?;
        }
        match self.current_state {
            ArtifactWorkspaceDeltaState::Unchanged => {
                if self.current_content_hash.as_deref() != Some(self.artifact.content_hash.as_str())
                    || self.added_lines != 0
                    || self.removed_lines != 0
                {
                    return Err("artifact_workspace_delta_unchanged_mismatch".to_owned());
                }
            }
            ArtifactWorkspaceDeltaState::Changed => {
                if self.current_content_hash.is_none() {
                    return Err("artifact_workspace_delta_changed_hash_missing".to_owned());
                }
            }
            ArtifactWorkspaceDeltaState::Missing | ArtifactWorkspaceDeltaState::Unavailable => {
                if self.current_content_hash.is_some()
                    || self.current_size_bytes.is_some()
                    || self.added_lines != 0
                    || self.removed_lines != 0
                {
                    return Err("artifact_workspace_delta_absent_content".to_owned());
                }
            }
        }
        Ok(())
    }
}

/// Explicit operator policy for reclaiming committed blobs that no Company fact cites.
///
/// Reclamation is opt-in and bounded: `allow_blob_removal` must be set explicitly, and both a
/// minimum age and a per-run cap are required so a repeated or rushed call cannot silently widen
/// deletion. The policy is a declaration of intent, not an authorization token; the caller still
/// passes the Company write context and the store re-checks its own journal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactPublicationRetentionPolicy {
    pub schema: String,
    pub max_orphan_age_ms: u64,
    pub max_orphan_count: u32,
    pub allow_blob_removal: bool,
    pub reason: String,
}

impl ArtifactPublicationRetentionPolicy {
    pub fn new(
        max_orphan_age_ms: u64,
        max_orphan_count: u32,
        allow_blob_removal: bool,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let policy = Self {
            schema: ARTIFACT_ORPHAN_RETENTION_SCHEMA.to_owned(),
            max_orphan_age_ms,
            max_orphan_count,
            allow_blob_removal,
            reason: reason.into(),
        };
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ARTIFACT_ORPHAN_RETENTION_SCHEMA {
            return Err("artifact_orphan_retention_policy_invalid".to_owned());
        }
        if self.max_orphan_age_ms == 0 || self.max_orphan_age_ms > MAX_ARTIFACT_ORPHAN_AGE_MS {
            return Err("artifact_orphan_retention_window_invalid".to_owned());
        }
        if self.max_orphan_count == 0 || self.max_orphan_count > MAX_ARTIFACT_ORPHAN_COUNT {
            return Err("artifact_orphan_retention_count_invalid".to_owned());
        }
        bounded(
            &self.reason,
            "artifact_orphan_retention_reason",
            MAX_REASON_BYTES,
        )
    }
}

/// What a retention plan decided for one orphan publication window.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactOrphanRetentionDecision {
    /// Still inside the retention window, or nothing left to reclaim at the recorded reference.
    Retained,
    /// Past the window and selected, but this run was plan-only and removed nothing.
    Eligible,
    /// Bytes were reclaimed in this run.
    Removed,
    /// Past the window but refused: over the cap, or removal was not permitted, unsupported or
    /// failed. A denial never deletes bytes.
    Denied,
}

/// One orphan publication window considered by a retention plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactOrphanRetention {
    pub command_id: String,
    pub artifact: ArtifactRef,
    pub intent_state: ArtifactPublicationState,
    pub age_ms: u64,
    pub decision: ArtifactOrphanRetentionDecision,
    pub reason: String,
}

impl ArtifactOrphanRetention {
    pub fn new(
        command_id: impl Into<String>,
        artifact: ArtifactRef,
        intent_state: ArtifactPublicationState,
        age_ms: u64,
        decision: ArtifactOrphanRetentionDecision,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let retention = Self {
            command_id: command_id.into(),
            artifact,
            intent_state,
            age_ms,
            decision,
            reason: reason.into(),
        };
        retention.validate()?;
        Ok(retention)
    }

    pub fn validate(&self) -> Result<(), String> {
        bounded(
            &self.command_id,
            "artifact_orphan_retention_command_id",
            MAX_IDENTITY_BYTES,
        )?;
        self.artifact.validate()?;
        bounded(
            &self.reason,
            "artifact_orphan_retention_reason",
            MAX_REASON_BYTES,
        )?;
        // Only a window that reconciliation already proved fact-free may be reclaimed. A pending or
        // committed intent is never an orphan, regardless of what a caller claims.
        if matches!(
            self.decision,
            ArtifactOrphanRetentionDecision::Eligible | ArtifactOrphanRetentionDecision::Removed
        ) && self.intent_state != ArtifactPublicationState::Abandoned
        {
            return Err("artifact_orphan_retention_not_abandoned".to_owned());
        }
        Ok(())
    }
}

/// Deterministic, self-checking retention plan for one reclamation attempt.
///
/// The plan carries its own policy so a reader can re-verify every decision without trusting the
/// producing process. `journal_available` is false when the store exposes no publication journal;
/// such a plan is empty by construction and never authorizes removal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactPublicationRetentionPlan {
    pub schema: String,
    pub journal_available: bool,
    pub policy: ArtifactPublicationRetentionPolicy,
    pub generated_at_unix_ms: u64,
    /// Open (unreconciled) publication windows are reported so an operator can see they were
    /// excluded from reclamation rather than silently ignored.
    pub pending_count: u32,
    pub orphans: Vec<ArtifactOrphanRetention>,
    pub eligible_count: u32,
    pub removed_count: u32,
    pub retained_count: u32,
    pub denied_count: u32,
}

impl ArtifactPublicationRetentionPlan {
    pub fn new(
        policy: ArtifactPublicationRetentionPolicy,
        journal_available: bool,
        pending_count: u32,
        orphans: Vec<ArtifactOrphanRetention>,
        generated_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let eligible_count = orphans
            .iter()
            .filter(|orphan| orphan.decision == ArtifactOrphanRetentionDecision::Eligible)
            .count() as u32;
        let removed_count = orphans
            .iter()
            .filter(|orphan| orphan.decision == ArtifactOrphanRetentionDecision::Removed)
            .count() as u32;
        let retained_count = orphans
            .iter()
            .filter(|orphan| orphan.decision == ArtifactOrphanRetentionDecision::Retained)
            .count() as u32;
        let denied_count = orphans
            .iter()
            .filter(|orphan| orphan.decision == ArtifactOrphanRetentionDecision::Denied)
            .count() as u32;
        let plan = Self {
            schema: ARTIFACT_ORPHAN_RETENTION_SCHEMA.to_owned(),
            journal_available,
            policy,
            generated_at_unix_ms,
            pending_count,
            orphans,
            eligible_count,
            removed_count,
            retained_count,
            denied_count,
        };
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ARTIFACT_ORPHAN_RETENTION_SCHEMA || self.generated_at_unix_ms == 0 {
            return Err("artifact_orphan_retention_plan_invalid".to_owned());
        }
        self.policy.validate()?;
        if !self.journal_available && (!self.orphans.is_empty() || self.pending_count != 0) {
            return Err("artifact_orphan_retention_plan_unbacked".to_owned());
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let (mut eligible, mut removed, mut retained, mut denied) = (0u32, 0u32, 0u32, 0u32);
        for orphan in &self.orphans {
            orphan.validate()?;
            if !seen.insert(orphan.command_id.as_str()) {
                return Err("artifact_orphan_retention_duplicate".to_owned());
            }
            match orphan.decision {
                ArtifactOrphanRetentionDecision::Retained => retained += 1,
                ArtifactOrphanRetentionDecision::Eligible => {
                    if orphan.age_ms < self.policy.max_orphan_age_ms {
                        return Err("artifact_orphan_retention_window_not_elapsed".to_owned());
                    }
                    eligible += 1;
                }
                ArtifactOrphanRetentionDecision::Removed => {
                    if orphan.age_ms < self.policy.max_orphan_age_ms {
                        return Err("artifact_orphan_retention_window_not_elapsed".to_owned());
                    }
                    if !self.policy.allow_blob_removal {
                        return Err("artifact_orphan_retention_removal_not_permitted".to_owned());
                    }
                    removed += 1;
                }
                ArtifactOrphanRetentionDecision::Denied => denied += 1,
            }
        }
        if eligible.saturating_add(removed) > self.policy.max_orphan_count {
            return Err("artifact_orphan_retention_capacity_exceeded".to_owned());
        }
        if self.eligible_count != eligible
            || self.removed_count != removed
            || self.retained_count != retained
            || self.denied_count != denied
        {
            return Err("artifact_orphan_retention_counts_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn is_plan_only(&self) -> bool {
        self.removed_count == 0
    }
}

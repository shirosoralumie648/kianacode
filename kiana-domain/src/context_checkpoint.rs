//! CAS-bound compaction checkpoint for the disposable context view.
//!
//! The checkpoint is not an EventLog implementation. It is the typed boundary that requires an
//! already-written summary artifact and protects the old active view from stale compaction or
//! steering. New Inbox input may advance while compaction is running; the commit carries the
//! current Inbox digest forward instead of overwriting it.

use crate::{json_digest, CompactionArtifact, CompactionCommit, RunId, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const CONTEXT_CHECKPOINT_SCHEMA: &str = "kiana.context-checkpoint.v1";
pub const CONTEXT_CHECKPOINT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCheckpointState {
    Active,
    Committed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextCheckpoint {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub state: ContextCheckpointState,
    pub checkpoint_revision: u64,
    pub source_context_revision: u64,
    pub active_context_revision: u64,
    pub source_cursor: u64,
    pub inbox_digest: String,
    pub inbox_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_digest: Option<String>,
    pub checkpoint_digest: String,
}

impl ContextCheckpoint {
    pub fn new(
        run_id: RunId,
        context_revision: u64,
        source_cursor: u64,
        inbox_digest: impl Into<String>,
        inbox_sequence: u64,
    ) -> Result<Self, String> {
        let mut checkpoint = Self {
            schema: CONTEXT_CHECKPOINT_SCHEMA.to_owned(),
            version: CONTEXT_CHECKPOINT_VERSION,
            run_id,
            state: ContextCheckpointState::Active,
            checkpoint_revision: 1,
            source_context_revision: context_revision,
            active_context_revision: context_revision,
            source_cursor,
            inbox_digest: inbox_digest.into(),
            inbox_sequence,
            artifact_digest: None,
            summary_digest: None,
            commit_digest: None,
            checkpoint_digest: String::new(),
        };
        checkpoint.checkpoint_digest = checkpoint.digest();
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTEXT_CHECKPOINT_SCHEMA
            || !self.version.is_compatible_with(&CONTEXT_CHECKPOINT_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.checkpoint_revision == 0
            || self.source_context_revision == 0
            || self.active_context_revision == 0
            || self.source_cursor == 0
        {
            return Err("context_checkpoint_header_invalid".to_owned());
        }
        digest(&self.inbox_digest, "context_checkpoint_inbox_digest")?;
        digest(&self.checkpoint_digest, "context_checkpoint_digest")?;
        for (value, field) in [
            (&self.artifact_digest, "context_checkpoint_artifact_digest"),
            (&self.summary_digest, "context_checkpoint_summary_digest"),
            (&self.commit_digest, "context_checkpoint_commit_digest"),
        ] {
            if let Some(value) = value {
                digest(value, field)?;
            }
        }
        match self.state {
            ContextCheckpointState::Active => {
                if self.active_context_revision != self.source_context_revision
                    || self.artifact_digest.is_some()
                    || self.summary_digest.is_some()
                    || self.commit_digest.is_some()
                {
                    return Err("context_checkpoint_active_state_invalid".to_owned());
                }
            }
            ContextCheckpointState::Committed => {
                if self.active_context_revision != self.source_context_revision.saturating_add(1)
                    || self.artifact_digest.is_none()
                    || self.summary_digest.is_none()
                    || self.commit_digest.is_none()
                {
                    return Err("context_checkpoint_committed_state_invalid".to_owned());
                }
            }
        }
        if self.checkpoint_digest != self.digest() {
            return Err("context_checkpoint_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Commit only after the summary artifact and CompactionCommit are already complete.
    /// `current_inbox_*` is intentionally read at commit time so new steering is preserved.
    #[allow(clippy::too_many_arguments)]
    pub fn commit(
        &self,
        artifact: &CompactionArtifact,
        commit: &CompactionCommit,
        current_context_revision: u64,
        current_source_cursor: u64,
        current_workspace_revision: u64,
        current_data_epoch: u64,
        current_inbox_digest: impl Into<String>,
        current_inbox_sequence: u64,
    ) -> Result<Self, String> {
        self.validate()?;
        artifact.validate()?;
        commit.validate_before_commit(
            current_context_revision,
            current_source_cursor,
            current_workspace_revision,
            current_data_epoch,
        )?;
        if self.state != ContextCheckpointState::Active {
            return Err("context_checkpoint_already_committed".to_owned());
        }
        if commit.run_id != self.run_id
            || artifact.run_id != self.run_id
            || commit.artifact_digest != artifact.artifact_digest
            || commit.summary_digest != artifact.summary_digest
            || commit.source_context_revision != self.source_context_revision
            || commit.source_cursor_start != self.source_cursor
            || commit.source_cursor_end != current_source_cursor
        {
            return Err("context_checkpoint_source_binding_changed".to_owned());
        }
        if current_context_revision != self.source_context_revision {
            return Err("context_checkpoint_context_revision_changed".to_owned());
        }
        if current_inbox_sequence < self.inbox_sequence {
            return Err("context_checkpoint_inbox_regressed".to_owned());
        }
        let current_inbox_digest = current_inbox_digest.into();
        digest(&current_inbox_digest, "context_checkpoint_inbox_digest")?;
        if current_inbox_sequence == self.inbox_sequence
            && current_inbox_digest != self.inbox_digest
        {
            return Err("context_checkpoint_inbox_changed".to_owned());
        }
        let mut next = Self {
            schema: CONTEXT_CHECKPOINT_SCHEMA.to_owned(),
            version: CONTEXT_CHECKPOINT_VERSION,
            run_id: self.run_id,
            state: ContextCheckpointState::Committed,
            checkpoint_revision: self.checkpoint_revision.saturating_add(1),
            source_context_revision: self.source_context_revision,
            active_context_revision: commit.new_context_revision,
            source_cursor: current_source_cursor,
            inbox_digest: current_inbox_digest,
            inbox_sequence: current_inbox_sequence,
            artifact_digest: Some(artifact.artifact_digest.clone()),
            summary_digest: Some(artifact.summary_digest.clone()),
            commit_digest: Some(commit.commit_digest.clone()),
            checkpoint_digest: String::new(),
        };
        next.checkpoint_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "state": self.state,
            "checkpoint_revision": self.checkpoint_revision,
            "source_context_revision": self.source_context_revision,
            "active_context_revision": self.active_context_revision,
            "source_cursor": self.source_cursor,
            "inbox_digest": self.inbox_digest,
            "inbox_sequence": self.inbox_sequence,
            "artifact_digest": self.artifact_digest,
            "summary_digest": self.summary_digest,
            "commit_digest": self.commit_digest,
        }))
    }
}

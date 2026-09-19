//! Artifact-first compaction commit and invalidation contract.

use crate::{json_digest, EventId, RunId, SchemaVersion, MAX_SOURCE_EVENT_IDS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const COMPACTION_ARTIFACT_SCHEMA: &str = "kiana.compaction-artifact.v1";
pub const COMPACTION_COMMIT_SCHEMA: &str = "kiana.compaction-commit.v1";
pub const COMPACTION_COMMIT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionArtifact {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub summary_digest: String,
    pub artifact_digest: String,
    pub source_cursor_start: u64,
    pub source_cursor_end: u64,
    pub content_bytes: u64,
    pub artifact_record_digest: String,
}

impl CompactionArtifact {
    pub fn new(
        run_id: RunId,
        summary_digest: impl Into<String>,
        artifact_digest: impl Into<String>,
        source_cursor_start: u64,
        source_cursor_end: u64,
        content_bytes: u64,
    ) -> Result<Self, String> {
        let mut artifact = Self {
            schema: COMPACTION_ARTIFACT_SCHEMA.to_owned(),
            version: COMPACTION_COMMIT_VERSION,
            run_id,
            summary_digest: summary_digest.into(),
            artifact_digest: artifact_digest.into(),
            source_cursor_start,
            source_cursor_end,
            content_bytes,
            artifact_record_digest: String::new(),
        };
        artifact.artifact_record_digest = artifact.digest();
        artifact.validate()?;
        Ok(artifact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COMPACTION_ARTIFACT_SCHEMA
            || !self.version.is_compatible_with(&COMPACTION_COMMIT_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.source_cursor_start == 0
            || self.source_cursor_start > self.source_cursor_end
            || self.content_bytes == 0
        {
            return Err("compaction_artifact_header_invalid".to_owned());
        }
        digest(&self.summary_digest, "compaction_summary_digest")?;
        digest(&self.artifact_digest, "compaction_artifact_digest")?;
        digest(
            &self.artifact_record_digest,
            "compaction_artifact_record_digest",
        )?;
        if self.artifact_record_digest != self.digest() {
            return Err("compaction_artifact_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "summary_digest": self.summary_digest,
            "artifact_digest": self.artifact_digest,
            "source_cursor_start": self.source_cursor_start,
            "source_cursor_end": self.source_cursor_end,
            "content_bytes": self.content_bytes,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionCommit {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub artifact_digest: String,
    pub summary_digest: String,
    pub source_context_revision: u64,
    pub new_context_revision: u64,
    pub source_cursor_start: u64,
    pub source_cursor_end: u64,
    pub source_event_ids: Vec<EventId>,
    pub prompt_bundle_digest: String,
    pub model_profile: String,
    pub budget_digest: String,
    pub workspace_revision: u64,
    pub data_epoch: u64,
    pub commit_digest: String,
}

impl CompactionCommit {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        artifact: &CompactionArtifact,
        source_context_revision: u64,
        new_context_revision: u64,
        source_event_ids: Vec<EventId>,
        prompt_bundle_digest: impl Into<String>,
        model_profile: impl Into<String>,
        budget_digest: impl Into<String>,
        workspace_revision: u64,
        data_epoch: u64,
    ) -> Result<Self, String> {
        artifact.validate()?;
        let mut commit = Self {
            schema: COMPACTION_COMMIT_SCHEMA.to_owned(),
            version: COMPACTION_COMMIT_VERSION,
            run_id: artifact.run_id,
            artifact_digest: artifact.artifact_digest.clone(),
            summary_digest: artifact.summary_digest.clone(),
            source_context_revision,
            new_context_revision,
            source_cursor_start: artifact.source_cursor_start,
            source_cursor_end: artifact.source_cursor_end,
            source_event_ids,
            prompt_bundle_digest: prompt_bundle_digest.into(),
            model_profile: model_profile.into(),
            budget_digest: budget_digest.into(),
            workspace_revision,
            data_epoch,
            commit_digest: String::new(),
        };
        commit.commit_digest = commit.digest();
        commit.validate()?;
        Ok(commit)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COMPACTION_COMMIT_SCHEMA
            || !self.version.is_compatible_with(&COMPACTION_COMMIT_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.source_context_revision == 0
            || self.new_context_revision != self.source_context_revision.saturating_add(1)
            || self.source_cursor_start == 0
            || self.source_cursor_start > self.source_cursor_end
            || self.source_event_ids.is_empty()
            || self.source_event_ids.len() > MAX_SOURCE_EVENT_IDS
            || self.workspace_revision == 0
            || self.data_epoch == 0
        {
            return Err("compaction_commit_header_invalid".to_owned());
        }
        for (value, field) in [
            (&self.artifact_digest, "compaction_artifact_digest"),
            (&self.summary_digest, "compaction_summary_digest"),
            (&self.prompt_bundle_digest, "compaction_prompt_digest"),
            (&self.budget_digest, "compaction_budget_digest"),
            (&self.commit_digest, "compaction_commit_digest"),
        ] {
            digest(value, field)?;
        }
        required(&self.model_profile, "compaction_model_profile", 256)?;
        let mut ids = BTreeSet::new();
        for event_id in &self.source_event_ids {
            if !ids.insert(*event_id) {
                return Err("compaction_commit_source_duplicate".to_owned());
            }
        }
        if self.commit_digest != self.digest() {
            return Err("compaction_commit_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_before_commit(
        &self,
        current_context_revision: u64,
        current_source_cursor: u64,
        current_workspace_revision: u64,
        current_data_epoch: u64,
    ) -> Result<(), String> {
        self.validate()?;
        if self.source_context_revision != current_context_revision
            || self.source_cursor_end != current_source_cursor
        {
            return Err("compaction_source_changed".to_owned());
        }
        if self.workspace_revision != current_workspace_revision {
            return Err("compaction_workspace_changed".to_owned());
        }
        if self.data_epoch != current_data_epoch {
            return Err("compaction_data_epoch_changed".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "artifact_digest": self.artifact_digest,
            "summary_digest": self.summary_digest,
            "source_context_revision": self.source_context_revision,
            "new_context_revision": self.new_context_revision,
            "source_cursor_start": self.source_cursor_start,
            "source_cursor_end": self.source_cursor_end,
            "source_event_ids": self.source_event_ids,
            "prompt_bundle_digest": self.prompt_bundle_digest,
            "model_profile": self.model_profile,
            "budget_digest": self.budget_digest,
            "workspace_revision": self.workspace_revision,
            "data_epoch": self.data_epoch,
        }))
    }
}

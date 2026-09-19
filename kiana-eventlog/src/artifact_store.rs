//! Non-durable ArtifactStorePort adapter for PD-14 CI semantics.

use async_trait::async_trait;
use kiana_domain::{journal_sha256, ArtifactRef, ArtifactVersion};
use kiana_ports::{ArtifactStorePort, PortError};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use tokio::sync::Mutex;

type ArtifactKey = (kiana_domain::ArtifactId, u64);

#[derive(Clone, Debug, Default)]
struct ArtifactState {
    staged: BTreeMap<ArtifactKey, (ArtifactVersion, Vec<u8>)>,
    committed: BTreeSet<ArtifactKey>,
}

/// In-process immutable artifact adapter. It is a CI/source semantics fixture, not durable proof.
#[derive(Clone, Debug, Default)]
pub struct MemoryArtifactStore {
    state: Arc<Mutex<ArtifactState>>,
}

impl MemoryArtifactStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn validate_content(version: &ArtifactVersion, content: &[u8]) -> Result<(), PortError> {
        version
            .validate()
            .map_err(|error| PortError::Failed(format!("artifact_version:{error}")))?;
        if version.size_bytes != content.len() as u64
            || journal_sha256(content) != version.content_hash
        {
            return Err(PortError::Conflict(
                "artifact_content_hash_mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    async fn committed(
        &self,
        reference: &ArtifactRef,
    ) -> Result<(ArtifactVersion, Vec<u8>), PortError> {
        reference
            .validate()
            .map_err(|error| PortError::Failed(format!("artifact_reference:{error}")))?;
        let state = self.state.lock().await;
        let key = (reference.artifact_id, reference.version);
        if !state.committed.contains(&key) {
            return Err(PortError::Unavailable(
                "artifact_version_not_committed".to_owned(),
            ));
        }
        let (version, content) = state
            .staged
            .get(&key)
            .cloned()
            .ok_or_else(|| PortError::Failed("artifact_manifest_missing".to_owned()))?;
        if version.content_hash != reference.content_hash
            || version.scope_digest != reference.scope_digest
            || version.artifact_schema != reference.artifact_schema
        {
            return Err(PortError::Conflict(
                "artifact_reference_manifest_mismatch".to_owned(),
            ));
        }
        Ok((version, content))
    }
}

#[async_trait]
impl ArtifactStorePort for MemoryArtifactStore {
    async fn stage_artifact(
        &self,
        version: ArtifactVersion,
        content: Vec<u8>,
    ) -> Result<ArtifactRef, PortError> {
        Self::validate_content(&version, &content)?;
        let key = (version.artifact_id, version.version);
        let mut state = self.state.lock().await;
        if state.staged.contains_key(&key) {
            return Err(PortError::Conflict(
                "artifact_version_already_staged".to_owned(),
            ));
        }
        let reference = version.as_ref();
        state.staged.insert(key, (version, content));
        Ok(reference)
    }

    async fn commit_artifact(
        &self,
        reference: ArtifactRef,
        expected_revision: Option<u64>,
    ) -> Result<(), PortError> {
        reference
            .validate()
            .map_err(|error| PortError::Failed(format!("artifact_reference:{error}")))?;
        if expected_revision.is_some_and(|revision| revision != reference.version) {
            return Err(PortError::Conflict(
                "artifact_manifest_revision_conflict".to_owned(),
            ));
        }
        let mut state = self.state.lock().await;
        let key = (reference.artifact_id, reference.version);
        let (version, content) = state
            .staged
            .get(&key)
            .cloned()
            .ok_or_else(|| PortError::Unavailable("artifact_version_not_staged".to_owned()))?;
        Self::validate_content(&version, &content)?;
        if version.as_ref() != reference {
            return Err(PortError::Conflict(
                "artifact_reference_manifest_mismatch".to_owned(),
            ));
        }
        state.committed.insert(key);
        Ok(())
    }

    async fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, PortError> {
        let (version, content) = self.committed(reference).await?;
        if journal_sha256(&content) != version.content_hash {
            return Err(PortError::Conflict(
                "artifact_content_hash_mismatch".to_owned(),
            ));
        }
        Ok(content)
    }

    async fn verify_artifact(&self, reference: &ArtifactRef) -> Result<(), PortError> {
        let (version, content) = self.committed(reference).await?;
        Self::validate_content(&version, &content)?;
        Ok(())
    }
}

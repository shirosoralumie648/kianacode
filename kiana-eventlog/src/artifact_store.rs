//! Non-durable ArtifactStorePort adapter for PD-14 CI semantics.

use async_trait::async_trait;
use kiana_domain::{
    journal_sha256, ArtifactRef, ArtifactVersion, DataPropagationReceipt, DataPropagationState,
    DataPropagationTarget,
};
use kiana_ports::{ArtifactStorePort, PortError};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use tokio::sync::Mutex;

type ArtifactKey = (kiana_domain::ArtifactId, u64);

#[derive(Clone, Debug, Default)]
struct ArtifactState {
    staged: BTreeMap<ArtifactKey, (ArtifactVersion, Vec<u8>)>,
    committed: BTreeSet<ArtifactKey>,
    invalidated: BTreeMap<ArtifactKey, (u64, String)>,
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
        if state.invalidated.contains_key(&key) {
            return Err(PortError::Conflict(
                "artifact_data_revoked_or_expired".to_owned(),
            ));
        }
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

    /// Invalidate a committed artifact by epoch/tombstone. Bytes stay immutable and are not
    /// treated as a successful read after revoke/delete/expire; physical erasure is an adapter
    /// concern and must produce its own receipt.
    pub async fn invalidate_artifact(
        &self,
        reference: &ArtifactRef,
        previous_epoch: u64,
        data_epoch: u64,
        tombstone_digest: &str,
        observed_at_ms: u64,
    ) -> Result<DataPropagationReceipt, PortError> {
        reference
            .validate()
            .map_err(|error| PortError::Failed(format!("artifact_reference:{error}")))?;
        if previous_epoch == 0
            || data_epoch <= previous_epoch
            || observed_at_ms == 0
            || !Self::valid_digest(tombstone_digest)
        {
            return Err(PortError::Failed(
                "artifact_invalidation_boundary_invalid".to_owned(),
            ));
        }
        let key = (reference.artifact_id, reference.version);
        let mut state = self.state.lock().await;
        if !state.committed.contains(&key) {
            return Err(PortError::Unavailable(
                "artifact_version_not_committed".to_owned(),
            ));
        }
        if let Some((epoch, digest)) = state.invalidated.get(&key) {
            if *epoch == data_epoch && digest == tombstone_digest {
                return Self::invalidation_receipt(
                    reference,
                    previous_epoch,
                    data_epoch,
                    tombstone_digest,
                    observed_at_ms,
                );
            }
            return Err(PortError::Conflict(
                "artifact_invalidation_conflict".to_owned(),
            ));
        }
        state
            .invalidated
            .insert(key, (data_epoch, tombstone_digest.to_owned()));
        Self::invalidation_receipt(
            reference,
            previous_epoch,
            data_epoch,
            tombstone_digest,
            observed_at_ms,
        )
    }

    fn valid_digest(value: &str) -> bool {
        let Some(hex) = value.strip_prefix("sha256:") else {
            return false;
        };
        hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    fn invalidation_receipt(
        reference: &ArtifactRef,
        previous_epoch: u64,
        data_epoch: u64,
        tombstone_digest: &str,
        observed_at_ms: u64,
    ) -> Result<DataPropagationReceipt, PortError> {
        let mut receipt = DataPropagationReceipt {
            schema: kiana_domain::DATA_PROPAGATION_RECEIPT_SCHEMA.to_owned(),
            version: kiana_domain::DATA_PROPAGATION_VERSION,
            target: DataPropagationTarget::Artifact,
            state: DataPropagationState::Invalidated,
            project_ref: reference.scope_digest.clone(),
            previous_epoch,
            data_epoch,
            source_cursor: reference.version,
            tombstone_digest: tombstone_digest.to_owned(),
            receipt_digest: None,
            observed_at_ms,
        };
        receipt.receipt_digest = Some(kiana_domain::json_digest(&json!({
            "artifact_id": reference.artifact_id,
            "version": reference.version,
            "scope_digest": reference.scope_digest,
            "data_epoch": data_epoch,
            "tombstone_digest": tombstone_digest,
        })));
        receipt
            .validate()
            .map_err(|error| PortError::Failed(format!("artifact_invalidation_receipt:{error}")))?;
        Ok(receipt)
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

    async fn invalidate_artifact(
        &self,
        reference: &ArtifactRef,
        previous_epoch: u64,
        data_epoch: u64,
        tombstone_digest: &str,
        observed_at_ms: u64,
    ) -> Result<DataPropagationReceipt, PortError> {
        MemoryArtifactStore::invalidate_artifact(
            self,
            reference,
            previous_epoch,
            data_epoch,
            tombstone_digest,
            observed_at_ms,
        )
        .await
    }
}

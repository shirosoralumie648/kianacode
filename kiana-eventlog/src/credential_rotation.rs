//! Opaque SecretRef rotation/revoke adapter for CI and local composition.
//!
//! This adapter stores references, not secret material.  It is intentionally in-memory and must
//! not be used as durable credential storage; its purpose is to make generation CAS and revoke
//! fencing executable without creating a second provider or authorization path.

use async_trait::async_trait;
use kiana_domain::SecretRef;
use kiana_ports::{CredentialRotationPort, PortError};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Debug, Default)]
pub struct MemoryCredentialRotationStore {
    refs: Arc<RwLock<HashMap<String, SecretRef>>>,
}

impl MemoryCredentialRotationStore {
    pub fn new(initial: impl IntoIterator<Item = SecretRef>) -> Result<Self, PortError> {
        let mut refs = HashMap::new();
        for secret_ref in initial {
            secret_ref
                .validate()
                .map_err(|error| PortError::Failed(format!("credential_ref_invalid:{error}")))?;
            if refs
                .insert(secret_ref.reference_digest.clone(), secret_ref)
                .is_some()
            {
                return Err(PortError::Conflict(
                    "credential_rotation_duplicate_reference".to_owned(),
                ));
            }
        }
        Ok(Self {
            refs: Arc::new(RwLock::new(refs)),
        })
    }

    async fn mutate(
        &self,
        secret_ref: &SecretRef,
        observed_generation: u64,
    ) -> Result<SecretRef, PortError> {
        secret_ref
            .validate()
            .map_err(|error| PortError::Failed(format!("credential_ref_invalid:{error}")))?;
        if observed_generation == 0 {
            return Err(PortError::Failed(
                "credential_rotation_generation_invalid".to_owned(),
            ));
        }
        let mut refs = self.refs.write().await;
        let current = refs.get(&secret_ref.reference_digest).ok_or_else(|| {
            PortError::Unavailable("credential_rotation_reference_missing".to_owned())
        })?;
        if current.generation != observed_generation {
            return Err(PortError::Conflict(
                "credential_rotation_generation_conflict".to_owned(),
            ));
        }
        let next = SecretRef::new(
            current.store.clone(),
            current.key.clone(),
            current.purpose.clone(),
            current.audience.clone(),
            observed_generation.checked_add(1).ok_or_else(|| {
                PortError::Failed("credential_rotation_generation_overflow".to_owned())
            })?,
        )
        .map_err(PortError::Failed)?;
        let old_digest = current.reference_digest.clone();
        refs.remove(&old_digest);
        refs.insert(next.reference_digest.clone(), next.clone());
        Ok(next)
    }
}

#[async_trait]
impl CredentialRotationPort for MemoryCredentialRotationStore {
    async fn rotate_credential(
        &self,
        secret_ref: &SecretRef,
        observed_generation: u64,
    ) -> Result<SecretRef, PortError> {
        self.mutate(secret_ref, observed_generation).await
    }

    async fn revoke_credential(
        &self,
        secret_ref: &SecretRef,
        observed_generation: u64,
    ) -> Result<SecretRef, PortError> {
        // Revocation is represented by the generation fence.  The returned ref is opaque and
        // cannot be resolved by a stale lease; a later admission must prove a new generation.
        self.mutate(secret_ref, observed_generation).await
    }
}

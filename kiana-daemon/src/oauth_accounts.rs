//! Server-owned in-process OAuth account store used by local composition and CI fixtures.
//!
//! The store persists only account identity and token metadata digests. Provider adapters retain raw
//! material in their protected SecretStore and call this port only after effect-time fencing.

use async_trait::async_trait;
use kiana_domain::{OAuthAccountRecord, OAuthTokenMetadata, ProviderAccountId};
use kiana_ports::{OAuthAccountStore, PortError};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::RwLock;

#[derive(Clone, Default)]
pub struct InMemoryOAuthAccountStore {
    accounts: Arc<RwLock<BTreeMap<ProviderAccountId, OAuthAccountRecord>>>,
}

impl InMemoryOAuthAccountStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn validate_record(account: &OAuthAccountRecord) -> Result<(), PortError> {
        account
            .validate_at(account.token_metadata.issued_at_unix_ms)
            .map_err(|error| PortError::Failed(format!("oauth_account_invalid:{error}")))
    }

    fn mutation_error(error: String) -> PortError {
        if error.contains("generation_conflict")
            || error.contains("revision_conflict")
            || error.contains("status_fenced")
        {
            PortError::Conflict(error)
        } else {
            PortError::Failed(error)
        }
    }
}

#[async_trait]
impl OAuthAccountStore for InMemoryOAuthAccountStore {
    async fn read_oauth_account(
        &self,
        account_id: ProviderAccountId,
    ) -> Result<Option<OAuthAccountRecord>, PortError> {
        Ok(self.accounts.read().await.get(&account_id).cloned())
    }

    async fn upsert_oauth_account(
        &self,
        account: OAuthAccountRecord,
        expected_revision: Option<u64>,
    ) -> Result<OAuthAccountRecord, PortError> {
        Self::validate_record(&account)?;
        let mut accounts = self.accounts.write().await;
        let Some(existing) = accounts.get(&account.account_id) else {
            if expected_revision.is_some() || account.revision != 1 {
                return Err(PortError::Conflict(
                    "oauth_account_expected_missing".to_owned(),
                ));
            }
            accounts.insert(account.account_id, account.clone());
            return Ok(account);
        };
        if existing.account_digest == account.account_digest
            && expected_revision.is_none_or(|revision| revision == existing.revision)
        {
            return Ok(existing.clone());
        }
        if expected_revision != Some(existing.revision) {
            return Err(PortError::Conflict(
                "oauth_account_revision_conflict".to_owned(),
            ));
        }
        // Existing records may only change through rotate/reauth/revoke, which validate both
        // revision and credential generation. Upsert is insert-or-idempotent by design.
        let _ = account;
        Err(PortError::Conflict(
            "oauth_account_mutation_requires_cas".to_owned(),
        ))
    }

    async fn rotate_oauth_account(
        &self,
        account_id: ProviderAccountId,
        observed_revision: u64,
        observed_generation: u64,
        next_metadata: OAuthTokenMetadata,
        now_unix_ms: u64,
    ) -> Result<OAuthAccountRecord, PortError> {
        let mut accounts = self.accounts.write().await;
        let current = accounts
            .get(&account_id)
            .cloned()
            .ok_or_else(|| PortError::Unavailable("oauth_account_not_found".to_owned()))?;
        let next = current
            .rotate(
                observed_revision,
                observed_generation,
                next_metadata,
                now_unix_ms,
            )
            .map_err(Self::mutation_error)?;
        accounts.insert(account_id, next.clone());
        Ok(next)
    }

    async fn require_oauth_reauth(
        &self,
        account_id: ProviderAccountId,
        observed_revision: u64,
        observed_generation: u64,
        now_unix_ms: u64,
    ) -> Result<OAuthAccountRecord, PortError> {
        let mut accounts = self.accounts.write().await;
        let current = accounts
            .get(&account_id)
            .cloned()
            .ok_or_else(|| PortError::Unavailable("oauth_account_not_found".to_owned()))?;
        let next = current
            .require_reauth(observed_revision, observed_generation, now_unix_ms)
            .map_err(Self::mutation_error)?;
        accounts.insert(account_id, next.clone());
        Ok(next)
    }

    async fn revoke_oauth_account(
        &self,
        account_id: ProviderAccountId,
        observed_revision: u64,
        observed_generation: u64,
        now_unix_ms: u64,
    ) -> Result<OAuthAccountRecord, PortError> {
        let mut accounts = self.accounts.write().await;
        let current = accounts
            .get(&account_id)
            .cloned()
            .ok_or_else(|| PortError::Unavailable("oauth_account_not_found".to_owned()))?;
        let next = current
            .revoke(observed_revision, observed_generation, now_unix_ms)
            .map_err(Self::mutation_error)?;
        accounts.insert(account_id, next.clone());
        Ok(next)
    }
}

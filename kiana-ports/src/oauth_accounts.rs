//! OAuth account persistence and lifecycle ports.
//!
//! Ports carry server-owned metadata only. Implementations may resolve raw credentials internally
//! at the provider effect boundary; this port never returns access/refresh token material.

use crate::PortError;
use async_trait::async_trait;
use kiana_domain::{OAuthAccountRecord, OAuthTokenMetadata, ProviderAccountId};

pub const OAUTH_ACCOUNT_STORE_PORT_SCHEMA: &str = "kiana.oauth-account-store-port.v1";

#[async_trait]
pub trait OAuthAccountStore: Send + Sync {
    async fn read_oauth_account(
        &self,
        account_id: ProviderAccountId,
    ) -> Result<Option<OAuthAccountRecord>, PortError>;

    /// Insert a new record or idempotently replay the same digest. Existing records require an
    /// exact expected revision; callers cannot overwrite a newer generation.
    async fn upsert_oauth_account(
        &self,
        account: OAuthAccountRecord,
        expected_revision: Option<u64>,
    ) -> Result<OAuthAccountRecord, PortError>;

    /// Compare-and-swap both the account revision and credential generation before rotation.
    async fn rotate_oauth_account(
        &self,
        account_id: ProviderAccountId,
        observed_revision: u64,
        observed_generation: u64,
        next_metadata: OAuthTokenMetadata,
        now_unix_ms: u64,
    ) -> Result<OAuthAccountRecord, PortError>;

    /// Reauthentication is a generation fence. An old in-flight refresh cannot restore a record
    /// after this operation commits.
    async fn require_oauth_reauth(
        &self,
        account_id: ProviderAccountId,
        observed_revision: u64,
        observed_generation: u64,
        now_unix_ms: u64,
    ) -> Result<OAuthAccountRecord, PortError>;

    /// Revocation increments the generation and account revision atomically.
    async fn revoke_oauth_account(
        &self,
        account_id: ProviderAccountId,
        observed_revision: u64,
        observed_generation: u64,
        now_unix_ms: u64,
    ) -> Result<OAuthAccountRecord, PortError>;
}

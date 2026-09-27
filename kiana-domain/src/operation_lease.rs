//! Single-writer operation lease and fencing contracts.
//!
//! `OperationLeaseCas` is a deterministic compare-and-swap projection. It does not hold an OS
//! lock, write EventLog facts or dispatch an effect. An adapter must persist the CAS mutation and
//! revalidate the returned lease at the effect boundary in later deployment steps.

use crate::{
    json_digest, FenceTokenId, InstanceId, OperationId, SchemaVersion, StorageLockId, StorageRootId,
};
use serde::{Deserialize, Serialize};

pub const OPERATION_LEASE_SCHEMA: &str = "kiana.operation-lease.v1";
pub const OPERATION_LEASE_CAS_SCHEMA: &str = "kiana.operation-lease-cas.v1";
pub const OPERATION_LEASE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_OPERATION_LEASE_TTL_MS: u64 = 86_400_000;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationLeaseState {
    Active,
    Released,
    Expired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationLease {
    pub schema: String,
    pub version: SchemaVersion,
    pub lease_id: StorageLockId,
    pub operation_id: OperationId,
    pub owner_instance_id: InstanceId,
    pub storage_root: StorageRootId,
    pub fence_token: FenceTokenId,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub cas_revision: u64,
    pub heartbeat_seq: u64,
    pub issued_at_unix_ms: u64,
    pub last_heartbeat_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub state: OperationLeaseState,
    pub lease_digest: String,
}

impl OperationLease {
    #[allow(clippy::too_many_arguments)]
    fn new(
        operation_id: OperationId,
        owner_instance_id: InstanceId,
        storage_root: StorageRootId,
        fence_token: FenceTokenId,
        authority_epoch: u64,
        data_epoch: u64,
        cas_revision: u64,
        issued_at_unix_ms: u64,
        ttl_ms: u64,
    ) -> Result<Self, String> {
        let expires_at_unix_ms = issued_at_unix_ms
            .checked_add(ttl_ms)
            .ok_or_else(|| "operation_lease_expiry_overflow".to_owned())?;
        let mut lease = Self {
            schema: OPERATION_LEASE_SCHEMA.to_owned(),
            version: OPERATION_LEASE_VERSION,
            lease_id: StorageLockId::new(),
            operation_id,
            owner_instance_id,
            storage_root,
            fence_token,
            authority_epoch,
            data_epoch,
            cas_revision,
            heartbeat_seq: 1,
            issued_at_unix_ms,
            last_heartbeat_at_unix_ms: issued_at_unix_ms,
            expires_at_unix_ms,
            state: OperationLeaseState::Active,
            lease_digest: String::new(),
        };
        lease.lease_digest = lease.digest();
        lease.validate()?;
        Ok(lease)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPERATION_LEASE_SCHEMA
            || self.version != OPERATION_LEASE_VERSION
            || self.lease_id.as_uuid().is_nil()
            || self.operation_id.as_uuid().is_nil()
            || self.owner_instance_id.as_uuid().is_nil()
            || self.storage_root.as_uuid().is_nil()
            || self.fence_token.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.cas_revision == 0
            || self.heartbeat_seq == 0
            || self.issued_at_unix_ms == 0
            || self.last_heartbeat_at_unix_ms < self.issued_at_unix_ms
            || self.expires_at_unix_ms <= self.last_heartbeat_at_unix_ms
        {
            return Err("operation_lease_header_invalid".to_owned());
        }
        if self.state != OperationLeaseState::Active
            && self.last_heartbeat_at_unix_ms >= self.expires_at_unix_ms
        {
            return Err("operation_lease_terminal_time_invalid".to_owned());
        }
        valid_digest(&self.lease_digest, "operation_lease_digest")?;
        if self.lease_digest != self.digest() {
            return Err("operation_lease_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_current(
        &self,
        now_unix_ms: u64,
        authority_epoch: u64,
        data_epoch: u64,
        fence_token: FenceTokenId,
    ) -> Result<(), String> {
        self.validate()?;
        if self.state != OperationLeaseState::Active {
            return Err("operation_lease_not_active".to_owned());
        }
        if now_unix_ms < self.last_heartbeat_at_unix_ms || now_unix_ms >= self.expires_at_unix_ms {
            return Err("operation_lease_expired".to_owned());
        }
        if authority_epoch != self.authority_epoch {
            return Err(if authority_epoch < self.authority_epoch {
                "operation_lease_authority_epoch_rollback"
            } else {
                "operation_lease_authority_epoch_stale"
            }
            .to_owned());
        }
        if data_epoch != self.data_epoch {
            return Err(if data_epoch < self.data_epoch {
                "operation_lease_data_epoch_rollback"
            } else {
                "operation_lease_data_epoch_stale"
            }
            .to_owned());
        }
        if fence_token != self.fence_token {
            return Err("operation_lease_fence_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "lease_id": self.lease_id,
            "operation_id": self.operation_id,
            "owner_instance_id": self.owner_instance_id,
            "storage_root": self.storage_root,
            "fence_token": self.fence_token,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "cas_revision": self.cas_revision,
            "heartbeat_seq": self.heartbeat_seq,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "last_heartbeat_at_unix_ms": self.last_heartbeat_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "state": self.state,
        }))
    }

    fn renewed(&self, cas_revision: u64, now_unix_ms: u64, ttl_ms: u64) -> Result<Self, String> {
        let expires_at_unix_ms = now_unix_ms
            .checked_add(ttl_ms)
            .ok_or_else(|| "operation_lease_expiry_overflow".to_owned())?;
        let mut lease = self.clone();
        lease.cas_revision = cas_revision;
        lease.heartbeat_seq = lease
            .heartbeat_seq
            .checked_add(1)
            .ok_or_else(|| "operation_lease_heartbeat_exhausted".to_owned())?;
        lease.last_heartbeat_at_unix_ms = now_unix_ms;
        lease.expires_at_unix_ms = expires_at_unix_ms;
        lease.state = OperationLeaseState::Active;
        lease.lease_digest = lease.digest();
        lease.validate()?;
        Ok(lease)
    }

    fn terminal(&self, cas_revision: u64, state: OperationLeaseState) -> Result<Self, String> {
        let mut lease = self.clone();
        lease.cas_revision = cas_revision;
        lease.state = state;
        lease.lease_digest = lease.digest();
        lease.validate()?;
        Ok(lease)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationLeaseCas {
    pub schema: String,
    pub version: SchemaVersion,
    pub storage_root: StorageRootId,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub revision: u64,
    pub active: Option<OperationLease>,
    pub last_lease: Option<OperationLease>,
    pub last_fence_token: Option<FenceTokenId>,
    pub cas_digest: String,
}

impl OperationLeaseCas {
    pub fn new(
        storage_root: StorageRootId,
        authority_epoch: u64,
        data_epoch: u64,
    ) -> Result<Self, String> {
        let mut cas = Self {
            schema: OPERATION_LEASE_CAS_SCHEMA.to_owned(),
            version: OPERATION_LEASE_VERSION,
            storage_root,
            authority_epoch,
            data_epoch,
            revision: 0,
            active: None,
            last_lease: None,
            last_fence_token: None,
            cas_digest: String::new(),
        };
        cas.cas_digest = cas.digest();
        cas.validate()?;
        Ok(cas)
    }

    pub fn acquire(
        &mut self,
        expected_revision: u64,
        operation_id: OperationId,
        owner_instance_id: InstanceId,
        fence_token: FenceTokenId,
        authority_epoch: u64,
        data_epoch: u64,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> Result<OperationLease, String> {
        self.validate()?;
        self.check_revision(expected_revision)?;
        if let Some(active) = &self.active {
            if now_unix_ms < active.expires_at_unix_ms {
                return Err("operation_lease_already_active".to_owned());
            }
            return Err("operation_lease_expired_reclaim_required".to_owned());
        }
        self.validate_epochs(authority_epoch, data_epoch)?;
        validate_identity(operation_id, owner_instance_id, fence_token)?;
        if self.last_fence_token == Some(fence_token) {
            return Err("operation_lease_fence_token_reused".to_owned());
        }
        validate_ttl(now_unix_ms, ttl_ms)?;
        let revision = self.next_revision()?;
        let lease = OperationLease::new(
            operation_id,
            owner_instance_id,
            self.storage_root,
            fence_token,
            authority_epoch,
            data_epoch,
            revision,
            now_unix_ms,
            ttl_ms,
        )?;
        self.authority_epoch = authority_epoch;
        self.data_epoch = data_epoch;
        self.revision = revision;
        self.last_fence_token = Some(fence_token);
        self.active = Some(lease.clone());
        self.cas_digest = self.digest();
        self.validate()?;
        Ok(lease)
    }

    pub fn heartbeat(
        &mut self,
        expected_revision: u64,
        operation_id: OperationId,
        owner_instance_id: InstanceId,
        fence_token: FenceTokenId,
        authority_epoch: u64,
        data_epoch: u64,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> Result<OperationLease, String> {
        self.validate()?;
        self.check_revision(expected_revision)?;
        let active = self
            .active
            .clone()
            .ok_or_else(|| "operation_lease_missing".to_owned())?;
        if active.operation_id != operation_id || active.owner_instance_id != owner_instance_id {
            return Err("operation_lease_owner_mismatch".to_owned());
        }
        active.validate_current(now_unix_ms, authority_epoch, data_epoch, fence_token)?;
        validate_ttl(now_unix_ms, ttl_ms)?;
        let revision = self.next_revision()?;
        let lease = active.renewed(revision, now_unix_ms, ttl_ms)?;
        self.revision = revision;
        self.active = Some(lease.clone());
        self.cas_digest = self.digest();
        self.validate()?;
        Ok(lease)
    }

    pub fn expire(
        &mut self,
        expected_revision: u64,
        now_unix_ms: u64,
    ) -> Result<OperationLease, String> {
        self.validate()?;
        self.check_revision(expected_revision)?;
        let active = self
            .active
            .clone()
            .ok_or_else(|| "operation_lease_missing".to_owned())?;
        if now_unix_ms < active.expires_at_unix_ms {
            return Err("operation_lease_expiry_not_due".to_owned());
        }
        let revision = self.next_revision()?;
        let expired = active.terminal(revision, OperationLeaseState::Expired)?;
        self.revision = revision;
        self.last_lease = Some(expired.clone());
        self.active = None;
        self.cas_digest = self.digest();
        self.validate()?;
        Ok(expired)
    }

    pub fn release(
        &mut self,
        expected_revision: u64,
        operation_id: OperationId,
        owner_instance_id: InstanceId,
        fence_token: FenceTokenId,
        now_unix_ms: u64,
    ) -> Result<OperationLease, String> {
        self.validate()?;
        self.check_revision(expected_revision)?;
        let active = self
            .active
            .clone()
            .ok_or_else(|| "operation_lease_missing".to_owned())?;
        if active.operation_id != operation_id || active.owner_instance_id != owner_instance_id {
            return Err("operation_lease_owner_mismatch".to_owned());
        }
        active.validate_current(
            now_unix_ms,
            self.authority_epoch,
            self.data_epoch,
            fence_token,
        )?;
        let revision = self.next_revision()?;
        let released = active.terminal(revision, OperationLeaseState::Released)?;
        self.revision = revision;
        self.last_lease = Some(released.clone());
        self.active = None;
        self.cas_digest = self.digest();
        self.validate()?;
        Ok(released)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPERATION_LEASE_CAS_SCHEMA
            || self.version != OPERATION_LEASE_VERSION
            || self.storage_root.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.data_epoch == 0
        {
            return Err("operation_lease_cas_header_invalid".to_owned());
        }
        if let Some(active) = &self.active {
            active.validate()?;
            if active.state != OperationLeaseState::Active
                || active.storage_root != self.storage_root
                || active.cas_revision != self.revision
                || active.authority_epoch != self.authority_epoch
                || active.data_epoch != self.data_epoch
            {
                return Err("operation_lease_cas_active_mismatch".to_owned());
            }
            if self.last_fence_token != Some(active.fence_token) {
                return Err("operation_lease_cas_fence_mismatch".to_owned());
            }
        }
        if let Some(last) = &self.last_lease {
            last.validate()?;
            if last.storage_root != self.storage_root {
                return Err("operation_lease_cas_last_root_mismatch".to_owned());
            }
        }
        if self
            .last_fence_token
            .is_some_and(|token| token.as_uuid().is_nil())
        {
            return Err("operation_lease_cas_fence_invalid".to_owned());
        }
        valid_digest(&self.cas_digest, "operation_lease_cas_digest")?;
        if self.cas_digest != self.digest() {
            return Err("operation_lease_cas_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "storage_root": self.storage_root,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "revision": self.revision,
            "active": self.active,
            "last_lease": self.last_lease,
            "last_fence_token": self.last_fence_token,
        }))
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn active(&self) -> Option<&OperationLease> {
        self.active.as_ref()
    }

    fn check_revision(&self, expected_revision: u64) -> Result<(), String> {
        if expected_revision != self.revision {
            return Err("operation_lease_cas_conflict".to_owned());
        }
        Ok(())
    }

    fn next_revision(&self) -> Result<u64, String> {
        self.revision
            .checked_add(1)
            .ok_or_else(|| "operation_lease_cas_revision_exhausted".to_owned())
    }

    fn validate_epochs(&self, authority_epoch: u64, data_epoch: u64) -> Result<(), String> {
        if authority_epoch == 0 || data_epoch == 0 {
            return Err("operation_lease_epoch_invalid".to_owned());
        }
        if authority_epoch < self.authority_epoch {
            return Err("operation_lease_authority_epoch_rollback".to_owned());
        }
        if data_epoch < self.data_epoch {
            return Err("operation_lease_data_epoch_rollback".to_owned());
        }
        Ok(())
    }
}

fn validate_identity(
    operation_id: OperationId,
    owner_instance_id: InstanceId,
    fence_token: FenceTokenId,
) -> Result<(), String> {
    if operation_id.as_uuid().is_nil()
        || owner_instance_id.as_uuid().is_nil()
        || fence_token.as_uuid().is_nil()
    {
        return Err("operation_lease_identity_invalid".to_owned());
    }
    Ok(())
}

fn validate_ttl(now_unix_ms: u64, ttl_ms: u64) -> Result<(), String> {
    if now_unix_ms == 0 || ttl_ms == 0 || ttl_ms > MAX_OPERATION_LEASE_TTL_MS {
        return Err("operation_lease_ttl_invalid".to_owned());
    }
    now_unix_ms
        .checked_add(ttl_ms)
        .ok_or_else(|| "operation_lease_expiry_overflow".to_owned())?;
    Ok(())
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

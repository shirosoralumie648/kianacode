//! SC-19 daemon-side secret lease authority: one-shot TTL leases, generation-fenced rotation and
//! revocation receipts, and a secret-free crash/restore journal.
//!
//! The authority owns server-side state for each secret slot (generation, status and revision)
//! and issues opaque, short-lived, one-shot [`CredentialLease`]s. It never stores, reads or
//! returns raw secret material: the slot key is a digest of the reference metadata, every
//! receipt carries only opaque references and digests, and a restore payload that carries a
//! secret value is rejected before any state is rebuilt.
//!
//! ```text
//!   issue_lease   -> CredentialLease (opaque, one-shot, TTL, generation-fenced)
//!   consume_lease -> re-check status/generation then consume exactly once before injection
//!   rotate        -> SecretRotationReceipt (CAS on observed generation, no rollback)
//!   revoke        -> SecretRevocationReceipt (fence status, block further issue/consume)
//!   export/restore-> SecretLeaseJournalRecord (no secret field; plaintext restore is denied)
//! ```
//!
//! Rotation and revocation are compare-and-swap on both the observed generation and the slot
//! status, so an expired, replayed, stale-generation or revoked reference cannot be injected.
//! This module only owns the in-process lease bookkeeping; the raw value stays in the provider
//! `SecretStore` and is materialized at the final transport boundary (see SC-18).

use kiana_domain::{
    json_digest, CredentialLease, RequestId, SecretRef, CREDENTIAL_LEASE_DEFAULT_TTL_MS,
};
use kiana_ports::{PortError, SecretStoreBinding};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

/// Fixed schema name for the secret-free rotation receipt.
pub const SECRET_ROTATION_RECEIPT_SCHEMA: &str = "kiana.secret-rotation-receipt.v1";
/// Fixed schema name for the secret-free revocation receipt.
pub const SECRET_REVOCATION_RECEIPT_SCHEMA: &str = "kiana.secret-revocation-receipt.v1";
/// Fixed schema name for the secret-free lease journal used by crash/restore.
pub const SECRET_LEASE_JOURNAL_SCHEMA: &str = "kiana.secret-lease-journal.v1";
/// Upper bound on a restore payload so a hostile journal cannot exhaust memory.
pub const SECRET_LEASE_JOURNAL_MAX_BYTES: usize = 1 << 20;

fn digest_field(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn bounded_field(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains(['\0', '\r', '\n'])
}

/// Server-side status of a secret slot. It carries no material.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretLeaseStatus {
    Active,
    Revoked,
}

/// Session-independent slot identity: the reference metadata minus the rotate-able generation.
fn slot_id(reference: &SecretRef) -> String {
    json_digest(&json!({
        "store": reference.store,
        "key": reference.key,
        "purpose": reference.purpose,
        "audience": reference.audience,
    }))
}

/// Names that would only appear if a raw secret value were being persisted. Restore uses this to
/// return a precise denial code instead of a generic schema error.
fn contains_plaintext_slot(payload: &str) -> bool {
    const NAMES: [&str; 6] = [
        "secret_value",
        "raw_secret",
        "raw_value",
        "plaintext",
        "access_token",
        "refresh_token",
    ];
    NAMES.iter().any(|needle| payload.contains(needle))
}

/// Secret-free receipt proving a rotation advanced exactly one observed generation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretRotationReceipt {
    pub schema: String,
    pub receipt_id: RequestId,
    pub slot_id: String,
    pub provider_account: String,
    pub purpose: String,
    pub audience: String,
    pub endpoint_digest: String,
    pub previous_generation: u64,
    pub next_generation: u64,
    pub rotated_at_unix_ms: u64,
    pub receipt_digest: String,
}

impl SecretRotationReceipt {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SECRET_ROTATION_RECEIPT_SCHEMA
            || self.receipt_id.as_uuid().is_nil()
            || !digest_field(&self.slot_id)
            || !bounded_field(&self.provider_account, 256)
            || !bounded_field(&self.purpose, 256)
            || !bounded_field(&self.audience, 256)
            || !digest_field(&self.endpoint_digest)
            || self.previous_generation == 0
            || self.next_generation <= self.previous_generation
            || self.rotated_at_unix_ms == 0
            || self.receipt_digest != self.digest()
        {
            return Err("secret_rotation_receipt_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "receipt_id": self.receipt_id,
            "slot_id": self.slot_id,
            "provider_account": self.provider_account,
            "purpose": self.purpose,
            "audience": self.audience,
            "endpoint_digest": self.endpoint_digest,
            "previous_generation": self.previous_generation,
            "next_generation": self.next_generation,
            "rotated_at_unix_ms": self.rotated_at_unix_ms,
        }))
    }
}

/// Secret-free receipt proving a revocation fenced the observed generation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretRevocationReceipt {
    pub schema: String,
    pub receipt_id: RequestId,
    pub slot_id: String,
    pub provider_account: String,
    pub purpose: String,
    pub audience: String,
    pub endpoint_digest: String,
    pub revoked_generation: u64,
    pub reason: String,
    pub revoked_at_unix_ms: u64,
    pub receipt_digest: String,
}

impl SecretRevocationReceipt {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SECRET_REVOCATION_RECEIPT_SCHEMA
            || self.receipt_id.as_uuid().is_nil()
            || !digest_field(&self.slot_id)
            || !bounded_field(&self.provider_account, 256)
            || !bounded_field(&self.purpose, 256)
            || !bounded_field(&self.audience, 256)
            || !digest_field(&self.endpoint_digest)
            || self.revoked_generation == 0
            || !bounded_field(&self.reason, 256)
            || self.revoked_at_unix_ms == 0
            || self.receipt_digest != self.digest()
        {
            return Err("secret_revocation_receipt_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "receipt_id": self.receipt_id,
            "slot_id": self.slot_id,
            "provider_account": self.provider_account,
            "purpose": self.purpose,
            "audience": self.audience,
            "endpoint_digest": self.endpoint_digest,
            "revoked_generation": self.revoked_generation,
            "reason": self.reason,
            "revoked_at_unix_ms": self.revoked_at_unix_ms,
        }))
    }
}

/// Secret-free lease-journal record used for crash/restore. The closed schema has no secret slot,
/// so a persisted record cannot carry a raw value across a restart.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretLeaseJournalRecord {
    pub schema: String,
    pub slot_id: String,
    pub provider_account: String,
    pub purpose: String,
    pub audience: String,
    pub endpoint_digest: String,
    pub generation: u64,
    pub status: SecretLeaseStatus,
    pub revision: u64,
    pub record_digest: String,
}

impl SecretLeaseJournalRecord {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SECRET_LEASE_JOURNAL_SCHEMA
            || !digest_field(&self.slot_id)
            || !bounded_field(&self.provider_account, 256)
            || !bounded_field(&self.purpose, 256)
            || !bounded_field(&self.audience, 256)
            || !digest_field(&self.endpoint_digest)
            || self.generation == 0
            || self.revision == 0
            || self.record_digest != self.digest()
        {
            return Err("secret_lease_journal_record_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "slot_id": self.slot_id,
            "provider_account": self.provider_account,
            "purpose": self.purpose,
            "audience": self.audience,
            "endpoint_digest": self.endpoint_digest,
            "generation": self.generation,
            "status": self.status,
            "revision": self.revision,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Slot {
    provider_account: String,
    purpose: String,
    audience: String,
    endpoint_digest: String,
    generation: u64,
    status: SecretLeaseStatus,
    revision: u64,
}

/// Server-owned secret lease bookkeeping.
///
/// Holds only opaque slot metadata. Raw secret values never enter this type; callers receive only
/// [`CredentialLease`]s and secret-free receipts.
#[derive(Clone, Default)]
pub struct SecretLeaseAuthority {
    slots: Arc<Mutex<BTreeMap<String, Slot>>>,
}

impl std::fmt::Debug for SecretLeaseAuthority {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SecretLeaseAuthority")
            .field("slot_count", &self.slot_count())
            .finish_non_exhaustive()
    }
}

impl SecretLeaseAuthority {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Slot>> {
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn slot_count(&self) -> usize {
        self.lock().len()
    }

    /// Validate the reference and its exact effect binding before any slot state is touched.
    fn validate_binding(
        &self,
        reference: &SecretRef,
        provider_account: &str,
        purpose: &str,
        audience: &str,
        endpoint_digest: &str,
    ) -> Result<(), PortError> {
        reference.validate().map_err(PortError::Failed)?;
        if reference.purpose != purpose || reference.audience != audience {
            return Err(PortError::Conflict(
                "secret_lease_binding_mismatch".to_owned(),
            ));
        }
        SecretStoreBinding::new(provider_account, purpose, audience, endpoint_digest)
            .map(|_| ())
            .map_err(PortError::Failed)
    }

    /// Read the current server-side status for a reference, for read-only callers.
    pub fn slot_status(&self, reference: &SecretRef) -> Result<SecretLeaseStatus, PortError> {
        reference.validate().map_err(PortError::Failed)?;
        self.lock()
            .get(&slot_id(reference))
            .map(|slot| slot.status)
            .ok_or_else(|| PortError::Unavailable("secret_lease_not_registered".to_owned()))
    }

    /// Issue a short-lived one-shot lease. A revoked slot is denied and a reference whose
    /// generation drifted from the server-owned slot is denied; otherwise the slot is registered
    /// on first use.
    #[allow(clippy::too_many_arguments)]
    pub fn issue_lease(
        &self,
        reference: &SecretRef,
        provider_account: &str,
        purpose: &str,
        audience: &str,
        endpoint_digest: &str,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> Result<CredentialLease, PortError> {
        self.validate_binding(reference, provider_account, purpose, audience, endpoint_digest)?;
        let ttl_ms = if ttl_ms == 0 {
            CREDENTIAL_LEASE_DEFAULT_TTL_MS
        } else {
            ttl_ms
        };
        let mut slots = self.lock();
        let slot = slots.entry(slot_id(reference)).or_insert_with(|| Slot {
            provider_account: provider_account.to_owned(),
            purpose: purpose.to_owned(),
            audience: audience.to_owned(),
            endpoint_digest: endpoint_digest.to_owned(),
            generation: reference.generation,
            status: SecretLeaseStatus::Active,
            revision: 1,
        });
        if slot.status == SecretLeaseStatus::Revoked {
            return Err(PortError::Conflict("secret_lease_revoked".to_owned()));
        }
        if slot.generation != reference.generation {
            return Err(PortError::Conflict(
                "secret_lease_generation_stale".to_owned(),
            ));
        }
        CredentialLease::issue(
            reference.clone(),
            provider_account,
            purpose,
            audience,
            endpoint_digest,
            now_unix_ms,
            ttl_ms,
        )
        .map_err(PortError::Failed)
    }

    /// Re-check slot status/generation and consume the lease exactly once immediately before
    /// injection. Expired, replayed, revoked or stale-generation leases are denied.
    #[allow(clippy::too_many_arguments)]
    pub fn consume_lease(
        &self,
        lease: &mut CredentialLease,
        reference: &SecretRef,
        provider_account: &str,
        purpose: &str,
        audience: &str,
        endpoint_digest: &str,
        now_unix_ms: u64,
    ) -> Result<(), PortError> {
        self.validate_binding(reference, provider_account, purpose, audience, endpoint_digest)?;
        if &lease.secret_ref != reference {
            return Err(PortError::Conflict(
                "secret_lease_reference_mismatch".to_owned(),
            ));
        }
        {
            let slots = self.lock();
            let slot = slots
                .get(&slot_id(reference))
                .ok_or_else(|| PortError::Unavailable("secret_lease_not_registered".to_owned()))?;
            if slot.status == SecretLeaseStatus::Revoked {
                return Err(PortError::Conflict("secret_lease_revoked".to_owned()));
            }
            if slot.generation != reference.generation {
                return Err(PortError::Conflict(
                    "secret_lease_generation_stale".to_owned(),
                ));
            }
        }
        lease
            .validate_for(now_unix_ms, provider_account, purpose, audience, endpoint_digest)
            .map_err(PortError::Conflict)?;
        lease.consume(now_unix_ms).map_err(PortError::Conflict)
    }

    /// Rotate the slot with compare-and-swap on the observed generation. A stale observation, a
    /// non-advancing generation or a revoked slot is denied; otherwise a secret-free receipt is
    /// returned and the slot generation is advanced.
    #[allow(clippy::too_many_arguments)]
    pub fn rotate(
        &self,
        reference: &SecretRef,
        observed_generation: u64,
        next_generation: u64,
        provider_account: &str,
        purpose: &str,
        audience: &str,
        endpoint_digest: &str,
        now_unix_ms: u64,
    ) -> Result<SecretRotationReceipt, PortError> {
        self.validate_binding(reference, provider_account, purpose, audience, endpoint_digest)?;
        if now_unix_ms == 0 {
            return Err(PortError::Failed("secret_lease_clock_invalid".to_owned()));
        }
        let key = slot_id(reference);
        let mut slots = self.lock();
        let slot = slots
            .get_mut(&key)
            .ok_or_else(|| PortError::Unavailable("secret_lease_not_registered".to_owned()))?;
        if slot.status == SecretLeaseStatus::Revoked {
            return Err(PortError::Conflict("secret_lease_revoked".to_owned()));
        }
        if observed_generation != slot.generation {
            return Err(PortError::Conflict(
                "secret_lease_generation_conflict".to_owned(),
            ));
        }
        if next_generation <= observed_generation {
            return Err(PortError::Conflict(
                "secret_lease_generation_rollback".to_owned(),
            ));
        }
        let previous_generation = slot.generation;
        slot.generation = next_generation;
        slot.revision = slot.revision.saturating_add(1);
        let mut receipt = SecretRotationReceipt {
            schema: SECRET_ROTATION_RECEIPT_SCHEMA.to_owned(),
            receipt_id: RequestId::new(),
            slot_id: key,
            provider_account: provider_account.to_owned(),
            purpose: purpose.to_owned(),
            audience: audience.to_owned(),
            endpoint_digest: endpoint_digest.to_owned(),
            previous_generation,
            next_generation,
            rotated_at_unix_ms: now_unix_ms,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest();
        receipt.validate().map_err(PortError::Failed)?;
        Ok(receipt)
    }

    /// Revoke the slot with compare-and-swap on the observed generation. After revocation every
    /// further issue/consume/rotate is denied until a new generation is admitted elsewhere.
    pub fn revoke(
        &self,
        reference: &SecretRef,
        observed_generation: u64,
        reason: &str,
        now_unix_ms: u64,
    ) -> Result<SecretRevocationReceipt, PortError> {
        reference.validate().map_err(PortError::Failed)?;
        if !bounded_field(reason, 256) {
            return Err(PortError::Failed("secret_lease_reason_invalid".to_owned()));
        }
        if now_unix_ms == 0 {
            return Err(PortError::Failed("secret_lease_clock_invalid".to_owned()));
        }
        let key = slot_id(reference);
        let mut slots = self.lock();
        let slot = slots
            .get_mut(&key)
            .ok_or_else(|| PortError::Unavailable("secret_lease_not_registered".to_owned()))?;
        if slot.status == SecretLeaseStatus::Revoked {
            return Err(PortError::Conflict(
                "secret_lease_already_revoked".to_owned(),
            ));
        }
        if observed_generation != slot.generation {
            return Err(PortError::Conflict(
                "secret_lease_generation_conflict".to_owned(),
            ));
        }
        slot.status = SecretLeaseStatus::Revoked;
        slot.revision = slot.revision.saturating_add(1);
        let revoked_generation = slot.generation;
        let mut receipt = SecretRevocationReceipt {
            schema: SECRET_REVOCATION_RECEIPT_SCHEMA.to_owned(),
            receipt_id: RequestId::new(),
            slot_id: key,
            provider_account: slot.provider_account.clone(),
            purpose: slot.purpose.clone(),
            audience: slot.audience.clone(),
            endpoint_digest: slot.endpoint_digest.clone(),
            revoked_generation,
            reason: reason.to_owned(),
            revoked_at_unix_ms: now_unix_ms,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest();
        receipt.validate().map_err(PortError::Failed)?;
        Ok(receipt)
    }

    /// Export the server-side slot state as a secret-free journal suitable for durable storage and
    /// crash/restore. The returned records contain no secret value by construction.
    pub fn export_journal(&self) -> Vec<SecretLeaseJournalRecord> {
        let slots = self.lock();
        slots
            .iter()
            .map(|(key, slot)| {
                let mut record = SecretLeaseJournalRecord {
                    schema: SECRET_LEASE_JOURNAL_SCHEMA.to_owned(),
                    slot_id: key.clone(),
                    provider_account: slot.provider_account.clone(),
                    purpose: slot.purpose.clone(),
                    audience: slot.audience.clone(),
                    endpoint_digest: slot.endpoint_digest.clone(),
                    generation: slot.generation,
                    status: slot.status,
                    revision: slot.revision,
                    record_digest: String::new(),
                };
                record.record_digest = record.digest();
                record
            })
            .collect()
    }

    /// Rebuild slot state from a serialized secret-free journal. A payload carrying a raw secret
    /// field is rejected before anything is applied, and a record whose generation would roll a
    /// known slot back is denied.
    pub fn restore_from_journal(&self, payload: &str) -> Result<usize, PortError> {
        if payload.len() > SECRET_LEASE_JOURNAL_MAX_BYTES {
            return Err(PortError::Failed(
                "secret_lease_journal_oversized".to_owned(),
            ));
        }
        let parsed: Result<Vec<SecretLeaseJournalRecord>, _> = serde_json::from_str(payload);
        let records = parsed.map_err(|_| {
            if contains_plaintext_slot(payload) {
                PortError::Conflict("secret_lease_restore_plaintext_rejected".to_owned())
            } else {
                PortError::Failed("secret_lease_journal_invalid".to_owned())
            }
        })?;
        for record in &records {
            record.validate().map_err(PortError::Failed)?;
        }
        let restored = records.len();
        let mut slots = self.lock();
        for record in records {
            if let Some(existing) = slots.get(&record.slot_id) {
                if existing.generation > record.generation {
                    return Err(PortError::Conflict(
                        "secret_lease_restore_generation_rollback".to_owned(),
                    ));
                }
            }
            slots.insert(
                record.slot_id,
                Slot {
                    provider_account: record.provider_account,
                    purpose: record.purpose,
                    audience: record.audience,
                    endpoint_digest: record.endpoint_digest,
                    generation: record.generation,
                    status: record.status,
                    revision: record.revision,
                },
            );
        }
        Ok(restored)
    }
}

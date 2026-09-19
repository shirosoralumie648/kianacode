//! In-memory retention-plan adapter for CI and local composition.
//!
//! The adapter stores validated scans and legal-hold receipts without mutating EventLog facts.
//! It is deliberately non-durable; SC-23 owns deletion/tombstone propagation and must consume
//! this boundary rather than infer eligibility from a projection or current wall-clock state.

use async_trait::async_trait;
use kiana_domain::{EventCursor, LegalHoldReceipt, RetentionScan, StoreIdentityId};
use kiana_ports::{PortError, RetentionStorePort};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Default)]
struct RetentionState {
    scans: BTreeMap<(StoreIdentityId, EventCursor), RetentionScan>,
    holds: BTreeMap<(StoreIdentityId, String), LegalHoldReceipt>,
}

/// Non-durable retention boundary used by focused CI fixtures and composition tests.
#[derive(Clone, Debug, Default)]
pub struct MemoryRetentionStore {
    state: Arc<Mutex<RetentionState>>,
}

impl MemoryRetentionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn append_scan(
        &self,
        store_id: StoreIdentityId,
        scan: RetentionScan,
    ) -> Result<(), PortError> {
        scan.validate()
            .map_err(|error| PortError::Failed(format!("retention_scan:{error}")))?;
        let mut state = self.state.lock().await;
        if let Some((_, latest)) = state
            .scans
            .iter()
            .filter(|((id, _), _)| *id == store_id)
            .max_by_key(|((_, cursor), _)| *cursor)
        {
            if latest.source_cursor > scan.source_cursor {
                return Err(PortError::Conflict(
                    "retention_scan_cursor_regressed".to_owned(),
                ));
            }
        }
        let key = (store_id, scan.source_cursor);
        if let Some(existing) = state.scans.get(&key) {
            if existing.scan_digest == scan.scan_digest {
                return Ok(());
            }
            return Err(PortError::Conflict(
                "retention_scan_digest_conflict".to_owned(),
            ));
        }
        for receipt in &scan.hold_receipts {
            Self::check_hold(&state, store_id, receipt)?;
        }
        for receipt in &scan.hold_receipts {
            state
                .holds
                .insert((store_id, receipt.hold_id.clone()), receipt.clone());
        }
        state.scans.insert(key, scan);
        Ok(())
    }

    pub async fn append_hold_receipt(
        &self,
        store_id: StoreIdentityId,
        receipt: LegalHoldReceipt,
    ) -> Result<(), PortError> {
        receipt
            .validate()
            .map_err(|error| PortError::Failed(format!("legal_hold_receipt:{error}")))?;
        let mut state = self.state.lock().await;
        Self::check_hold(&state, store_id, &receipt)?;
        state
            .holds
            .insert((store_id, receipt.hold_id.clone()), receipt);
        Ok(())
    }

    async fn latest_scan(
        &self,
        store_id: StoreIdentityId,
        before_cursor: EventCursor,
    ) -> Option<RetentionScan> {
        self.state
            .lock()
            .await
            .scans
            .iter()
            .filter(|((id, cursor), _)| *id == store_id && *cursor <= before_cursor)
            .max_by_key(|((_, cursor), _)| *cursor)
            .map(|(_, scan)| scan.clone())
    }

    fn check_hold(
        state: &RetentionState,
        store_id: StoreIdentityId,
        receipt: &LegalHoldReceipt,
    ) -> Result<(), PortError> {
        if let Some(existing) = state.holds.get(&(store_id, receipt.hold_id.clone())) {
            if existing.receipt_digest == receipt.receipt_digest {
                return Ok(());
            }
            return Err(PortError::Conflict(
                "legal_hold_receipt_digest_conflict".to_owned(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl RetentionStorePort for MemoryRetentionStore {
    async fn plan_retention(
        &self,
        store_id: StoreIdentityId,
        before_cursor: EventCursor,
    ) -> Result<serde_json::Value, PortError> {
        let scan = self
            .latest_scan(store_id, before_cursor)
            .await
            .ok_or_else(|| PortError::Unavailable("retention_scan_missing".to_owned()))?;
        Ok(json!({
            "schema": "kiana.retention-plan.v1",
            "store_id": store_id,
            "project_ref": scan.project_ref,
            "policy_revision": scan.policy_revision,
            "data_epoch": scan.data_epoch,
            "source_cursor": scan.source_cursor,
            "projection_cursor": scan.projection_cursor,
            "scan_digest": scan.scan_digest,
            "scan": scan,
        }))
    }

    async fn append_tombstone(
        &self,
        _store_id: StoreIdentityId,
        _object_ref: &str,
        _reason: &str,
    ) -> Result<(), PortError> {
        Err(PortError::Unavailable(
            "retention_tombstone_deferred_to_sc23".to_owned(),
        ))
    }

    async fn purge_tombstoned(
        &self,
        _store_id: StoreIdentityId,
        _expected_tombstone_revision: u64,
    ) -> Result<u64, PortError> {
        Err(PortError::Unavailable(
            "retention_purge_deferred_to_sc23".to_owned(),
        ))
    }
}

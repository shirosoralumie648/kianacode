//! In-memory retention-plan adapter for CI and local composition.
//!
//! The adapter stores validated scans and legal-hold receipts without mutating EventLog facts.
//! It is deliberately non-durable; SC-23 owns deletion/tombstone propagation and must consume
//! this boundary rather than infer eligibility from a projection or current wall-clock state.

use async_trait::async_trait;
use kiana_domain::{
    DataPropagationPlan, DataPropagationReceipt, DeletionManifest, DeletionTombstone, EventCursor,
    LegalHoldReceipt, RequestId, RetentionScan, StoreIdentityId,
};
use kiana_ports::{PortError, RetentionStorePort};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Default)]
struct RetentionState {
    scans: BTreeMap<(StoreIdentityId, EventCursor), RetentionScan>,
    holds: BTreeMap<(StoreIdentityId, String), LegalHoldReceipt>,
    tombstones: BTreeMap<(StoreIdentityId, String), DeletionTombstone>,
    manifests: BTreeMap<(StoreIdentityId, RequestId), DeletionManifest>,
    propagation: BTreeMap<(StoreIdentityId, String, u64), DataPropagationReceipt>,
    deletion_epochs: BTreeMap<StoreIdentityId, u64>,
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

    pub async fn append_deletion_tombstone(
        &self,
        store_id: StoreIdentityId,
        tombstone: DeletionTombstone,
    ) -> Result<(), PortError> {
        tombstone
            .validate()
            .map_err(|error| PortError::Failed(format!("deletion_tombstone:{error}")))?;
        let mut state = self.state.lock().await;
        let key = (store_id, tombstone.tombstone_id.clone());
        if let Some(existing) = state.tombstones.get(&key) {
            if existing.tombstone_digest == tombstone.tombstone_digest {
                return Ok(());
            }
            return Err(PortError::Conflict(
                "deletion_tombstone_digest_conflict".to_owned(),
            ));
        }
        if state
            .deletion_epochs
            .get(&store_id)
            .is_some_and(|epoch| tombstone.data_epoch < *epoch)
        {
            return Err(PortError::Conflict(
                "deletion_data_epoch_regressed".to_owned(),
            ));
        }
        state
            .deletion_epochs
            .entry(store_id)
            .and_modify(|epoch| *epoch = (*epoch).max(tombstone.data_epoch))
            .or_insert(tombstone.data_epoch);
        state.tombstones.insert(key, tombstone);
        Ok(())
    }

    pub async fn append_deletion_manifest(
        &self,
        store_id: StoreIdentityId,
        manifest: DeletionManifest,
    ) -> Result<(), PortError> {
        manifest
            .validate()
            .map_err(|error| PortError::Failed(format!("deletion_manifest:{error}")))?;
        let mut state = self.state.lock().await;
        let key = (store_id, manifest.request_id);
        if let Some(existing) = state.manifests.get(&key) {
            if existing.manifest_digest == manifest.manifest_digest {
                return Ok(());
            }
            return Err(PortError::Conflict(
                "deletion_manifest_digest_conflict".to_owned(),
            ));
        }
        for tombstone_id in &manifest.tombstone_ids {
            let tombstone = state
                .tombstones
                .get(&(store_id, tombstone_id.clone()))
                .ok_or_else(|| PortError::Unavailable("deletion_tombstone_missing".to_owned()))?;
            if tombstone.request_id != manifest.request_id
                || tombstone.project_ref != manifest.project_ref
                || tombstone.policy_revision != manifest.policy_revision
                || tombstone.data_epoch != manifest.data_epoch
                || tombstone.source_cursor != manifest.source_cursor
            {
                return Err(PortError::Conflict(
                    "deletion_manifest_tombstone_boundary_mismatch".to_owned(),
                ));
            }
        }
        state.manifests.insert(key, manifest);
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

    /// Append one propagation target only after its deletion manifest exists. Unknown receipts
    /// remain queryable and are never upgraded by a cache or redaction decision.
    pub async fn append_propagation_receipt(
        &self,
        store_id: StoreIdentityId,
        plan: DataPropagationPlan,
        receipt: DataPropagationReceipt,
    ) -> Result<(), PortError> {
        plan.validate()
            .map_err(|error| PortError::Failed(format!("data_propagation_plan:{error}")))?;
        receipt
            .validate()
            .map_err(|error| PortError::Failed(format!("data_propagation_receipt:{error}")))?;
        if receipt.project_ref != plan.project_ref
            || receipt.previous_epoch != plan.previous_epoch
            || receipt.data_epoch != plan.data_epoch
            || receipt.source_cursor != plan.source_cursor
            || receipt.tombstone_digest != plan.tombstone_digest
            || !plan
                .targets
                .iter()
                .any(|target| target.target == receipt.target)
        {
            return Err(PortError::Conflict(
                "data_propagation_receipt_boundary_mismatch".to_owned(),
            ));
        }
        let mut state = self.state.lock().await;
        let manifest = state
            .manifests
            .values()
            .find(|manifest| {
                manifest.project_ref == plan.project_ref
                    && manifest.data_epoch == plan.data_epoch
                    && manifest.source_cursor == plan.source_cursor
            })
            .ok_or_else(|| PortError::Unavailable("deletion_manifest_missing".to_owned()))?;
        if manifest.tombstone_ids.is_empty() {
            return Err(PortError::Unavailable(
                "deletion_manifest_tombstone_missing".to_owned(),
            ));
        }
        let key = (
            store_id,
            receipt.target.as_str().to_owned(),
            receipt.data_epoch,
        );
        if let Some(existing) = state.propagation.get(&key) {
            if existing == &receipt {
                return Ok(());
            }
            return Err(PortError::Conflict(
                "data_propagation_receipt_conflict".to_owned(),
            ));
        }
        state.propagation.insert(key, receipt);
        Ok(())
    }

    pub async fn propagation_receipts(
        &self,
        store_id: StoreIdentityId,
        data_epoch: u64,
    ) -> Vec<DataPropagationReceipt> {
        self.state
            .lock()
            .await
            .propagation
            .iter()
            .filter(|((id, _, epoch), _)| *id == store_id && *epoch == data_epoch)
            .map(|(_, receipt)| receipt.clone())
            .collect()
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
            "project_ref": scan.project_ref.clone(),
            "policy_revision": scan.policy_revision,
            "data_epoch": scan.data_epoch,
            "source_cursor": scan.source_cursor,
            "projection_cursor": scan.projection_cursor,
            "scan_digest": scan.scan_digest.clone(),
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

    async fn append_deletion_tombstone(
        &self,
        store_id: StoreIdentityId,
        tombstone: DeletionTombstone,
    ) -> Result<(), PortError> {
        MemoryRetentionStore::append_deletion_tombstone(self, store_id, tombstone).await
    }

    async fn append_deletion_manifest(
        &self,
        store_id: StoreIdentityId,
        manifest: DeletionManifest,
    ) -> Result<(), PortError> {
        MemoryRetentionStore::append_deletion_manifest(self, store_id, manifest).await
    }

    async fn append_propagation_receipt(
        &self,
        store_id: StoreIdentityId,
        plan: DataPropagationPlan,
        receipt: DataPropagationReceipt,
    ) -> Result<(), PortError> {
        MemoryRetentionStore::append_propagation_receipt(self, store_id, plan, receipt).await
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

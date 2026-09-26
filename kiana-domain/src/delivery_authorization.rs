//! Delivery approval, dispatch, effect receipt and recipient confirmation contract.
//!
//! Authorization is bound to one immutable DeliveryManifest.  Dispatch is a one-way intent;
//! only a Broker receipt can record an effect.  An Unknown receipt fences the intent for
//! reconciliation, and a sender claim can never become a recipient confirmation.

use crate::{json_digest, DeliveryManifest, LocalDeliveryPackage};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const DELIVERY_AUTHORIZATION_SCHEMA: &str = "kiana.delivery-authorization.v1";
pub const DELIVERY_DISPATCH_SCHEMA: &str = "kiana.delivery-dispatch.v1";
pub const DELIVERY_EFFECT_RECEIPT_SCHEMA: &str = "kiana.delivery-effect-receipt.v1";
pub const DELIVERY_CONFIRMATION_SCHEMA: &str = "kiana.delivery-confirmation.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains(['\0', '\r', '\n']) {
        Err(field)
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), &'static str> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field);
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryDispatchState {
    Requested,
    AwaitingEffect,
    AwaitingReconciliation,
    Delivered,
    Failed,
    Confirmed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryAuthorization {
    pub schema: String,
    pub authorization_id: String,
    pub delivery_id: String,
    pub manifest_id: String,
    pub manifest_digest: String,
    pub project_id: String,
    pub baseline_version: u64,
    pub destination: String,
    pub recipient_ref: String,
    pub approved_by: String,
    pub approved_at: u64,
    pub expires_at: u64,
    pub digest: String,
}

impl DeliveryAuthorization {
    pub fn validate_against(
        &self,
        manifest: &DeliveryManifest,
        now: u64,
    ) -> Result<(), &'static str> {
        manifest.validate()?;
        if self.schema != DELIVERY_AUTHORIZATION_SCHEMA
            || self.baseline_version == 0
            || self.expires_at <= self.approved_at
            || now >= self.expires_at
            || self.manifest_id != manifest.manifest_id
            || self.manifest_digest != manifest.digest
            || self.delivery_id != manifest.delivery_id
            || self.project_id != manifest.project_id
            || self.baseline_version != manifest.baseline_version
            || self.destination != manifest.destination
            || self.recipient_ref != manifest.recipient_ref
        {
            return Err("delivery_authorization_manifest_or_expiry_invalid");
        }
        for (value, field) in [
            (&self.authorization_id, "delivery_authorization_id_required"),
            (
                &self.delivery_id,
                "delivery_authorization_delivery_required",
            ),
            (
                &self.approved_by,
                "delivery_authorization_approver_required",
            ),
        ] {
            required(value, field)?;
        }
        digest(
            &self.manifest_digest,
            "delivery_authorization_manifest_digest_invalid",
        )?;
        if self.digest != self.canonical_digest() {
            return Err("delivery_authorization_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "authorization_id": self.authorization_id,
            "delivery_id": self.delivery_id,
            "manifest_id": self.manifest_id,
            "manifest_digest": self.manifest_digest,
            "project_id": self.project_id,
            "baseline_version": self.baseline_version,
            "destination": self.destination,
            "recipient_ref": self.recipient_ref,
            "approved_by": self.approved_by,
            "approved_at": self.approved_at,
            "expires_at": self.expires_at,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryDispatchIntent {
    pub schema: String,
    pub intent_id: String,
    pub authorization_id: String,
    pub delivery_id: String,
    pub manifest_digest: String,
    pub dispatch_request_id: String,
    pub requested_by: String,
    pub requested_at: u64,
    pub state: DeliveryDispatchState,
    pub digest: String,
}

impl DeliveryDispatchIntent {
    fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DELIVERY_DISPATCH_SCHEMA || self.requested_at == 0 {
            return Err("delivery_dispatch_header_invalid");
        }
        for (value, field) in [
            (&self.intent_id, "delivery_dispatch_intent_required"),
            (
                &self.authorization_id,
                "delivery_dispatch_authorization_required",
            ),
            (&self.delivery_id, "delivery_dispatch_delivery_required"),
            (
                &self.dispatch_request_id,
                "delivery_dispatch_request_required",
            ),
            (&self.requested_by, "delivery_dispatch_sender_required"),
        ] {
            required(value, field)?;
        }
        digest(
            &self.manifest_digest,
            "delivery_dispatch_manifest_digest_invalid",
        )?;
        if self.digest != self.canonical_digest() {
            return Err("delivery_dispatch_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "intent_id": self.intent_id,
            "authorization_id": self.authorization_id,
            "delivery_id": self.delivery_id,
            "manifest_digest": self.manifest_digest,
            "dispatch_request_id": self.dispatch_request_id,
            "requested_by": self.requested_by,
            "requested_at": self.requested_at,
            "state": self.state,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryReceiptSource {
    Broker,
    SenderClaim,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryEffectOutcome {
    Delivered,
    Failed,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryEffectReceipt {
    pub schema: String,
    pub receipt_id: String,
    pub intent_id: String,
    pub delivery_id: String,
    pub manifest_digest: String,
    pub package_id: String,
    pub package_digest: String,
    pub destination: String,
    pub recipient_ref: String,
    pub source: DeliveryReceiptSource,
    pub outcome: DeliveryEffectOutcome,
    pub evidence_refs: Vec<String>,
    pub observed_at: u64,
    pub digest: String,
}

impl DeliveryEffectReceipt {
    fn validate_against(
        &self,
        intent: &DeliveryDispatchIntent,
        manifest: &DeliveryManifest,
        package: &LocalDeliveryPackage,
    ) -> Result<(), &'static str> {
        manifest.validate()?;
        package.validate_against(manifest)?;
        intent.validate()?;
        if self.schema != DELIVERY_EFFECT_RECEIPT_SCHEMA
            || self.observed_at == 0
            || self.source != DeliveryReceiptSource::Broker
            || self.intent_id != intent.intent_id
            || self.delivery_id != manifest.delivery_id
            || self.manifest_digest != manifest.digest
            || self.package_id != package.package_id
            || self.package_digest != package.package_digest
            || self.destination != manifest.destination
            || self.recipient_ref != manifest.recipient_ref
            || self.evidence_refs.is_empty()
        {
            return Err("delivery_effect_receipt_binding_invalid");
        }
        for (value, field) in [
            (&self.receipt_id, "delivery_effect_receipt_id_required"),
            (&self.intent_id, "delivery_effect_intent_required"),
        ] {
            required(value, field)?;
        }
        digest(
            &self.manifest_digest,
            "delivery_effect_receipt_manifest_digest_invalid",
        )?;
        digest(
            &self.package_digest,
            "delivery_effect_receipt_package_digest_invalid",
        )?;
        for evidence in &self.evidence_refs {
            required(evidence, "delivery_effect_receipt_evidence_invalid")?;
        }
        if self.digest != self.canonical_digest() {
            return Err("delivery_effect_receipt_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "receipt_id": self.receipt_id,
            "intent_id": self.intent_id,
            "delivery_id": self.delivery_id,
            "manifest_digest": self.manifest_digest,
            "package_id": self.package_id,
            "package_digest": self.package_digest,
            "destination": self.destination,
            "recipient_ref": self.recipient_ref,
            "source": self.source,
            "outcome": self.outcome,
            "evidence_refs": self.evidence_refs,
            "observed_at": self.observed_at,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryRecipientConfirmation {
    pub schema: String,
    pub confirmation_id: String,
    pub receipt_id: String,
    pub delivery_id: String,
    pub manifest_digest: String,
    pub package_digest: String,
    pub recipient_ref: String,
    pub confirmed_by: String,
    pub confirmed_at: u64,
    pub human_confirmed: bool,
    pub digest: String,
}

impl DeliveryRecipientConfirmation {
    fn validate_against(&self, receipt: &DeliveryEffectReceipt) -> Result<(), &'static str> {
        if self.schema != DELIVERY_CONFIRMATION_SCHEMA
            || self.confirmed_at == 0
            || !self.human_confirmed
            || receipt.outcome != DeliveryEffectOutcome::Delivered
            || receipt.source != DeliveryReceiptSource::Broker
            || self.receipt_id != receipt.receipt_id
            || self.delivery_id != receipt.delivery_id
            || self.manifest_digest != receipt.manifest_digest
            || self.package_digest != receipt.package_digest
            || self.recipient_ref != receipt.recipient_ref
            || self.confirmed_by != receipt.recipient_ref
        {
            return Err("delivery_confirmation_binding_invalid");
        }
        for (value, field) in [
            (&self.confirmation_id, "delivery_confirmation_id_required"),
            (&self.receipt_id, "delivery_confirmation_receipt_required"),
            (
                &self.confirmed_by,
                "delivery_confirmation_recipient_required",
            ),
        ] {
            required(value, field)?;
        }
        digest(
            &self.manifest_digest,
            "delivery_confirmation_manifest_digest_invalid",
        )?;
        digest(
            &self.package_digest,
            "delivery_confirmation_package_digest_invalid",
        )?;
        if self.digest != self.canonical_digest() {
            return Err("delivery_confirmation_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "confirmation_id": self.confirmation_id,
            "receipt_id": self.receipt_id,
            "delivery_id": self.delivery_id,
            "manifest_digest": self.manifest_digest,
            "package_digest": self.package_digest,
            "recipient_ref": self.recipient_ref,
            "confirmed_by": self.confirmed_by,
            "confirmed_at": self.confirmed_at,
            "human_confirmed": self.human_confirmed,
        }))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeliveryAuthorizationLedger {
    pub authorizations: BTreeMap<String, DeliveryAuthorization>,
    pub dispatches: BTreeMap<String, DeliveryDispatchIntent>,
    pub receipts: BTreeMap<String, DeliveryEffectReceipt>,
    pub confirmations: BTreeMap<String, DeliveryRecipientConfirmation>,
}

impl DeliveryAuthorizationLedger {
    pub fn authorize(
        &mut self,
        authorization: DeliveryAuthorization,
        manifest: &DeliveryManifest,
        now: u64,
    ) -> Result<(), &'static str> {
        authorization.validate_against(manifest, now)?;
        if let Some(existing) = self.authorizations.get(&authorization.authorization_id) {
            if existing.digest == authorization.digest {
                return Ok(());
            }
            return Err("delivery_authorization_duplicate_digest_mismatch");
        }
        self.authorizations
            .insert(authorization.authorization_id.clone(), authorization);
        Ok(())
    }

    pub fn request_dispatch(
        &mut self,
        intent: DeliveryDispatchIntent,
        manifest: &DeliveryManifest,
        now: u64,
    ) -> Result<(), &'static str> {
        intent.validate()?;
        let authorization = self
            .authorizations
            .get(&intent.authorization_id)
            .ok_or("delivery_authorization_not_found")?;
        authorization.validate_against(manifest, now)?;
        if intent.delivery_id != manifest.delivery_id || intent.manifest_digest != manifest.digest {
            return Err("delivery_dispatch_manifest_binding_invalid");
        }
        if let Some(existing) = self.dispatches.get(&intent.intent_id) {
            if existing.digest == intent.digest {
                return Ok(());
            }
            return Err("delivery_dispatch_duplicate_digest_mismatch");
        }
        if let Some(existing) = self
            .dispatches
            .values()
            .find(|dispatch| dispatch.delivery_id == intent.delivery_id)
        {
            if existing.state == DeliveryDispatchState::AwaitingReconciliation {
                return Err("delivery_unknown_dispatch_fenced");
            }
            return Err("delivery_dispatch_already_requested");
        }
        self.dispatches.insert(intent.intent_id.clone(), intent);
        Ok(())
    }

    pub fn record_effect(
        &mut self,
        receipt: DeliveryEffectReceipt,
        manifest: &DeliveryManifest,
        package: &LocalDeliveryPackage,
    ) -> Result<(), &'static str> {
        let intent = self
            .dispatches
            .get(&receipt.intent_id)
            .ok_or("delivery_dispatch_not_found")?;
        receipt.validate_against(intent, manifest, package)?;
        if let Some(existing) = self.receipts.get(&receipt.receipt_id) {
            if existing.digest == receipt.digest {
                return Ok(());
            }
            return Err("delivery_effect_receipt_duplicate_digest_mismatch");
        }
        if !matches!(
            intent.state,
            DeliveryDispatchState::Requested | DeliveryDispatchState::AwaitingEffect
        ) {
            return Err("delivery_effect_dispatch_state_invalid");
        }
        let intent_id = receipt.intent_id.clone();
        let receipt_id = receipt.receipt_id.clone();
        let outcome = receipt.outcome;
        self.receipts.insert(receipt_id, receipt);
        let next_state = match outcome {
            DeliveryEffectOutcome::Delivered => DeliveryDispatchState::Delivered,
            DeliveryEffectOutcome::Failed => DeliveryDispatchState::Failed,
            DeliveryEffectOutcome::Unknown => DeliveryDispatchState::AwaitingReconciliation,
        };
        let dispatch = self.dispatches.get_mut(&intent_id).unwrap();
        dispatch.state = next_state;
        dispatch.digest = dispatch.canonical_digest();
        Ok(())
    }

    pub fn confirm_recipient(
        &mut self,
        confirmation: DeliveryRecipientConfirmation,
    ) -> Result<(), &'static str> {
        let receipt = self
            .receipts
            .get(&confirmation.receipt_id)
            .ok_or("delivery_effect_receipt_not_found")?;
        confirmation.validate_against(receipt)?;
        if let Some(existing) = self.confirmations.get(&confirmation.confirmation_id) {
            if existing.digest == confirmation.digest {
                return Ok(());
            }
            return Err("delivery_confirmation_duplicate_digest_mismatch");
        }
        self.confirmations
            .insert(confirmation.confirmation_id.clone(), confirmation);
        if let Some(intent) = self
            .dispatches
            .values_mut()
            .find(|intent| intent.intent_id == receipt.intent_id)
        {
            intent.state = DeliveryDispatchState::Confirmed;
            intent.digest = intent.canonical_digest();
        }
        Ok(())
    }

    pub fn dispatch_for_delivery(&self, delivery_id: &str) -> Option<&DeliveryDispatchIntent> {
        self.dispatches
            .values()
            .find(|intent| intent.delivery_id == delivery_id)
    }
}

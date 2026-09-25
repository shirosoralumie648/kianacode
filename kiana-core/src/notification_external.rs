//! Default-off external notification admission and receipt observation.
//!
//! This is a pure contract gate. Even an enabled policy returns an explicit handoff plan with
//! `direct_effect=false`; no HTTP/A2A client, connector, socket or secret signer is present here.

use kiana_domain::{
    ExternalNotificationEnvelope, ExternalNotificationPolicy, ExternalNotificationReceipt,
    ExternalNotificationReceiptStatus, EXTERNAL_NOTIFICATION_ADMISSION_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalNotificationDisposition {
    NotSupported,
    ReadyForExplicitConnector,
    ReconcileRequired,
    Acknowledged,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalNotificationAdmission {
    pub schema: String,
    pub disposition: ExternalNotificationDisposition,
    pub notification_id: String,
    pub delivery_id: String,
    pub envelope_digest: String,
    pub reason: String,
    pub control_plane_required: bool,
    pub direct_effect: bool,
    pub admission_digest: String,
}

impl ExternalNotificationAdmission {
    fn new(
        envelope: &ExternalNotificationEnvelope,
        disposition: ExternalNotificationDisposition,
        reason: &str,
        control_plane_required: bool,
    ) -> Result<Self, String> {
        let mut admission = Self {
            schema: EXTERNAL_NOTIFICATION_ADMISSION_SCHEMA.to_owned(),
            disposition,
            notification_id: envelope.notification_id.clone(),
            delivery_id: envelope.delivery_id.clone(),
            envelope_digest: envelope.envelope_digest.clone(),
            reason: reason.to_owned(),
            control_plane_required,
            direct_effect: false,
            admission_digest: String::new(),
        };
        admission.admission_digest = kiana_domain::json_digest(&json!({
            "schema": admission.schema,
            "disposition": admission.disposition,
            "notification_id": admission.notification_id,
            "delivery_id": admission.delivery_id,
            "envelope_digest": admission.envelope_digest,
            "reason": admission.reason,
            "control_plane_required": admission.control_plane_required,
            "direct_effect": admission.direct_effect,
        }));
        Ok(admission)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalNotificationReceiptObservation {
    Acknowledged,
    Rejected,
    ReconcileRequired,
}

pub fn admit_external_notification(
    policy: &ExternalNotificationPolicy,
    envelope: &ExternalNotificationEnvelope,
    now_unix_ms: u64,
    expected_nonce: Option<&str>,
    expected_authority_epoch: u64,
    expected_data_epoch: u64,
) -> Result<ExternalNotificationAdmission, String> {
    policy.validate()?;
    envelope.validate()?;
    if !policy.enabled {
        return ExternalNotificationAdmission::new(
            envelope,
            ExternalNotificationDisposition::NotSupported,
            "external_notification_disabled",
            true,
        );
    }
    if expected_authority_epoch == 0 || expected_data_epoch == 0 {
        return Err("external_notification_expected_epoch_invalid".to_owned());
    }
    if !policy
        .allowed_origins
        .contains(&envelope.destination_origin)
    {
        return Err("external_notification_origin_not_allowlisted".to_owned());
    }
    if expected_nonce != Some(envelope.nonce.as_str()) {
        return Err("external_notification_nonce_mismatch".to_owned());
    }
    if envelope.authority_epoch != expected_authority_epoch
        || envelope.data_epoch != expected_data_epoch
        || envelope.authority_epoch != policy.authority_epoch
        || envelope.data_epoch != policy.data_epoch
    {
        return Err("external_notification_epoch_mismatch".to_owned());
    }
    if now_unix_ms < envelope.created_at_unix_ms || now_unix_ms >= envelope.expires_at_unix_ms {
        return Err("external_notification_expired".to_owned());
    }
    ExternalNotificationAdmission::new(
        envelope,
        ExternalNotificationDisposition::ReadyForExplicitConnector,
        "explicit_connector_and_control_plane_required",
        true,
    )
}

pub fn observe_external_notification_receipt(
    envelope: &ExternalNotificationEnvelope,
    receipt: &ExternalNotificationReceipt,
) -> Result<ExternalNotificationReceiptObservation, String> {
    envelope.validate()?;
    receipt.validate()?;
    if receipt.notification_id != envelope.notification_id
        || receipt.delivery_id != envelope.delivery_id
        || receipt.transport != envelope.transport
        || receipt.payload_digest != envelope.payload_digest
        || receipt.nonce != envelope.nonce
        || receipt.authority_epoch != envelope.authority_epoch
    {
        return Err("external_notification_receipt_binding_mismatch".to_owned());
    }
    Ok(match receipt.status {
        ExternalNotificationReceiptStatus::Acknowledged => {
            ExternalNotificationReceiptObservation::Acknowledged
        }
        ExternalNotificationReceiptStatus::Rejected => {
            ExternalNotificationReceiptObservation::Rejected
        }
        ExternalNotificationReceiptStatus::Unknown => {
            ExternalNotificationReceiptObservation::ReconcileRequired
        }
    })
}

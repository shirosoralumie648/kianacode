//! Default-off external notification admission and receipt observation.
//!
//! This is a pure contract gate. Even an enabled policy returns an explicit handoff plan with
//! `direct_effect=false`; no HTTP/A2A client, connector, socket or secret signer is present here.
//!
//! The policy a request is admitted against is the server-pinned registry snapshot, never a
//! caller-supplied struct: the request may only echo the digest it expects
//! ([`ExternalNotificationAdmissionContext`]), and a mismatch is refused. Nonce and delivery reuse
//! is refused by an explicit bounded [`ExternalNotificationReplayStore`]. Signatures are key-bound
//! only; [`check_external_notification_signature`] always reports
//! [`ExternalNotificationSignatureVerification::NotSupported`] instead of faking verification.

use kiana_domain::{
    ExternalNotificationEnvelope, ExternalNotificationPolicySnapshot, ExternalNotificationReceipt,
    ExternalNotificationReceiptStatus, ExternalNotificationReplayStore,
    EXTERNAL_NOTIFICATION_ADMISSION_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const EXTERNAL_NOTIFICATION_REGISTRY_SCHEMA: &str =
    "kiana.external-notification-policy-registry.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalNotificationDisposition {
    NotSupported,
    ReadyForExplicitConnector,
    ReconcileRequired,
    Acknowledged,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalNotificationSignatureVerification {
    /// No signer, key material or verification routine exists in this build. The signature digest
    /// is shape-checked and the key id is bound to the pinned policy, but nothing is verified.
    NotSupported,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalNotificationSignatureCheck {
    pub signing_key_id: String,
    pub signature_digest: String,
    pub verification: ExternalNotificationSignatureVerification,
    pub reason: String,
    pub check_digest: String,
}

/// What a caller believes it is admitting against. Every field is compared with the server-pinned
/// registry snapshot; none of them can enable, widen or re-key the policy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalNotificationAdmissionContext {
    pub policy_digest: String,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub now_unix_ms: u64,
}

impl ExternalNotificationAdmissionContext {
    pub fn new(
        policy_digest: impl Into<String>,
        authority_epoch: u64,
        data_epoch: u64,
        now_unix_ms: u64,
    ) -> Self {
        Self {
            policy_digest: policy_digest.into(),
            authority_epoch,
            data_epoch,
            now_unix_ms,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalNotificationAdmission {
    pub schema: String,
    pub disposition: ExternalNotificationDisposition,
    pub notification_id: String,
    pub delivery_id: String,
    pub envelope_digest: String,
    pub policy_revision: u64,
    pub policy_digest: String,
    pub signing_key_id: Option<String>,
    pub signature_verification: ExternalNotificationSignatureVerification,
    pub reason: String,
    pub control_plane_required: bool,
    pub direct_effect: bool,
    pub admission_digest: String,
}

impl ExternalNotificationAdmission {
    fn new(
        envelope: &ExternalNotificationEnvelope,
        registry: &ExternalNotificationPolicyRegistry,
        disposition: ExternalNotificationDisposition,
        reason: &str,
        control_plane_required: bool,
    ) -> Result<Self, String> {
        let policy = registry.policy();
        let mut admission = Self {
            schema: EXTERNAL_NOTIFICATION_ADMISSION_SCHEMA.to_owned(),
            disposition,
            notification_id: envelope.notification_id.clone(),
            delivery_id: envelope.delivery_id.clone(),
            envelope_digest: envelope.envelope_digest.clone(),
            policy_revision: registry.snapshot.revision,
            policy_digest: registry.snapshot.policy.policy_digest.clone(),
            signing_key_id: policy.signing_key_id.clone(),
            signature_verification: ExternalNotificationSignatureVerification::NotSupported,
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
            "policy_revision": admission.policy_revision,
            "policy_digest": admission.policy_digest,
            "signing_key_id": admission.signing_key_id,
            "signature_verification": admission.signature_verification,
            "reason": admission.reason,
            "control_plane_required": admission.control_plane_required,
            "direct_effect": admission.direct_effect,
        }));
        Ok(admission)
    }
}

/// Server-owned external notification policy. A request never constructs one of these; it may only
/// name the digest of the snapshot the server already pinned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalNotificationPolicyRegistry {
    snapshot: ExternalNotificationPolicySnapshot,
}

impl ExternalNotificationPolicyRegistry {
    /// The explicit disabled default installed by a fresh server.
    pub fn disabled(
        authority_epoch: u64,
        data_epoch: u64,
        issued_at_unix_ms: u64,
    ) -> Result<Self, String> {
        Self::install(ExternalNotificationPolicySnapshot::disabled(
            authority_epoch,
            data_epoch,
            issued_at_unix_ms,
        )?)
    }

    /// Install a server-issued snapshot. This is the server/operator path; no request handler
    /// should call it with request data.
    pub fn install(snapshot: ExternalNotificationPolicySnapshot) -> Result<Self, String> {
        snapshot.validate()?;
        Ok(Self { snapshot })
    }

    /// Publish a newer snapshot. The revision must strictly advance; equal or older revisions are
    /// refused so a stale or replayed policy cannot silently re-enable delivery.
    pub fn publish(&mut self, snapshot: ExternalNotificationPolicySnapshot) -> Result<(), String> {
        snapshot.validate()?;
        if snapshot.revision <= self.snapshot.revision {
            return Err("external_notification_policy_revision_rollback".to_owned());
        }
        if snapshot.policy.policy_digest == self.snapshot.policy.policy_digest
            && snapshot.revision != self.snapshot.revision
        {
            return Err("external_notification_policy_replay".to_owned());
        }
        self.snapshot = snapshot;
        Ok(())
    }

    pub fn snapshot(&self) -> &ExternalNotificationPolicySnapshot {
        &self.snapshot
    }

    pub fn revision(&self) -> u64 {
        self.snapshot.revision
    }

    pub fn policy(&self) -> &kiana_domain::ExternalNotificationPolicy {
        &self.snapshot.policy
    }

    /// Admit one envelope against the pinned snapshot. Refusals return a stable code and never
    /// touch the replay store, so a denied delivery cannot consume the caller's nonce.
    pub fn admit(
        &self,
        envelope: &ExternalNotificationEnvelope,
        context: &ExternalNotificationAdmissionContext,
        replay: &mut ExternalNotificationReplayStore,
    ) -> Result<ExternalNotificationAdmission, String> {
        self.snapshot.validate()?;
        envelope.validate()?;
        let policy = &self.snapshot.policy;
        if context.policy_digest != policy.policy_digest {
            // A request cannot substitute its own enabled policy: it may only echo the digest of
            // the snapshot the server holds.
            return Err("external_notification_policy_not_pinned".to_owned());
        }
        if !policy.enabled {
            return ExternalNotificationAdmission::new(
                envelope,
                self,
                ExternalNotificationDisposition::NotSupported,
                "external_notification_disabled",
                true,
            );
        }
        if context.authority_epoch == 0 || context.data_epoch == 0 {
            return Err("external_notification_expected_epoch_invalid".to_owned());
        }
        if !policy
            .allowed_origins
            .contains(&envelope.destination_origin)
        {
            return Err("external_notification_origin_not_allowlisted".to_owned());
        }
        if envelope.authority_epoch != context.authority_epoch
            || envelope.data_epoch != context.data_epoch
            || envelope.authority_epoch != policy.authority_epoch
            || envelope.data_epoch != policy.data_epoch
        {
            return Err("external_notification_epoch_mismatch".to_owned());
        }
        if context.now_unix_ms < envelope.created_at_unix_ms
            || context.now_unix_ms >= envelope.expires_at_unix_ms
        {
            return Err("external_notification_expired".to_owned());
        }
        envelope.bind_signing_key(policy)?;
        // The signature digest is shape-checked by `validate`; this build cannot verify it.
        check_external_notification_signature(policy, envelope)?;
        replay.admit(
            &envelope.delivery_id,
            &envelope.nonce,
            envelope.expires_at_unix_ms,
            context.now_unix_ms,
        )?;
        ExternalNotificationAdmission::new(
            envelope,
            self,
            ExternalNotificationDisposition::ReadyForExplicitConnector,
            "explicit_connector_and_control_plane_required",
            true,
        )
    }
}

/// Pure key binding for an external notification signature.
///
/// The returned check always reports `NotSupported`: this module has no key material and no
/// verification routine. Callers must not surface it as a verified signature.
pub fn check_external_notification_signature(
    policy: &kiana_domain::ExternalNotificationPolicy,
    envelope: &ExternalNotificationEnvelope,
) -> Result<ExternalNotificationSignatureCheck, String> {
    envelope.bind_signing_key(policy)?;
    let signing_key_id = envelope
        .signing_key_id
        .clone()
        .ok_or_else(|| "external_notification_signature_key_missing".to_owned())?;
    let mut check = ExternalNotificationSignatureCheck {
        signing_key_id,
        signature_digest: envelope.signature_digest.clone(),
        verification: ExternalNotificationSignatureVerification::NotSupported,
        reason: "external_notification_signature_verification_not_supported".to_owned(),
        check_digest: String::new(),
    };
    check.check_digest = kiana_domain::json_digest(&json!({
        "signing_key_id": check.signing_key_id,
        "signature_digest": check.signature_digest,
        "verification": check.verification,
        "reason": check.reason,
    }));
    Ok(check)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalNotificationReceiptObservation {
    Acknowledged,
    Rejected,
    ReconcileRequired,
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

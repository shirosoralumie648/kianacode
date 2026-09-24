//! BQ-18 allow-listed provider fallback admission and per-attempt receipt contracts.
//!
//! A fallback is a new model attempt, never a transport retry shortcut.  The value contracts in
//! this module are intentionally side-effect free: the ControlPlane must recheck authority,
//! data, budget, credential and price inputs before issuing a fresh permit; the provider adapter
//! can only validate the resulting admission before entering the existing gateway path.

use crate::{
    json_digest, AttemptId, BillingUnknownReason, CapabilitySupport, CostBreakdown,
    CostBreakdownKind, ModelCapabilities, ModelRoute, NormalizedUsage, ProviderReceiptRef,
    RateCard, RateCardId, RunId, SchemaVersion, UsageConfidence,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const FALLBACK_ADMISSION_SCHEMA: &str = "kiana.fallback-admission.v1";
pub const FALLBACK_ATTEMPT_RECEIPT_SCHEMA: &str = "kiana.fallback-attempt-receipt.v1";
pub const FALLBACK_ADMISSION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_FALLBACK_ROUTES: usize = 256;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn same_digest(left: &str, right: &str, reason: &str) -> Result<(), String> {
    if left == right {
        Ok(())
    } else {
        Err(reason.to_owned())
    }
}

/// Capabilities required by the already admitted request.  A fallback may preserve or exceed
/// these values, but it cannot silently downgrade a capability merely because a backup route was
/// selected.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackCapabilityRequirements {
    pub require_tools: bool,
    pub require_streaming: bool,
    pub require_structured_output: bool,
    pub require_images: bool,
    pub require_reasoning_replay: bool,
    pub min_context_window: u64,
    pub min_output_tokens: u64,
}

impl FallbackCapabilityRequirements {
    pub fn validate(&self) -> Result<(), String> {
        if self.min_context_window == 0 || self.min_output_tokens == 0 {
            return Err("fallback_capability_requirement_invalid".to_owned());
        }
        Ok(())
    }

    fn accepts(&self, capabilities: &ModelCapabilities) -> Result<(), String> {
        self.validate()?;
        let denied = (self.require_tools && capabilities.tools != CapabilitySupport::Supported)
            || (self.require_streaming && capabilities.streaming != CapabilitySupport::Supported)
            || (self.require_structured_output
                && capabilities.structured_output != CapabilitySupport::Supported)
            || (self.require_images && capabilities.images != CapabilitySupport::Supported)
            || (self.require_reasoning_replay
                && capabilities.reasoning_replay != CapabilitySupport::Supported)
            || capabilities.context_window < self.min_context_window
            || capabilities.max_output < self.min_output_tokens;
        if denied {
            Err("fallback_capability_downgrade_rejected".to_owned())
        } else {
            Ok(())
        }
    }
}

/// A server-owned route/model allowlist.  Both the exact route digest and the provider/model
/// identity are pinned so a profile alias cannot smuggle a different model under an equivalent
/// name.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackRouteAllowlistEntry {
    pub route_digest: String,
    pub provider_id: String,
    pub model_id: String,
}

impl FallbackRouteAllowlistEntry {
    pub fn from_route(route: &ModelRoute) -> Result<Self, String> {
        let entry = Self {
            route_digest: route.digest(),
            provider_id: route.provider_id.clone(),
            model_id: route.model_id.clone(),
        };
        entry.validate().map(|_| entry)
    }

    pub fn validate(&self) -> Result<(), String> {
        digest(&self.route_digest, "fallback_allowlist_route_digest")?;
        required(&self.provider_id, "fallback_allowlist_provider", 256)?;
        required(&self.model_id, "fallback_allowlist_model", 256)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackRouteAllowlist {
    pub schema: String,
    pub version: SchemaVersion,
    pub entries: Vec<FallbackRouteAllowlistEntry>,
    pub allowlist_digest: String,
}

impl FallbackRouteAllowlist {
    pub fn from_routes(routes: &[ModelRoute]) -> Result<Self, String> {
        if routes.is_empty() || routes.len() > MAX_FALLBACK_ROUTES {
            return Err("fallback_allowlist_invalid".to_owned());
        }
        let mut entries = routes
            .iter()
            .map(FallbackRouteAllowlistEntry::from_route)
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort_by(|left, right| {
            (&left.provider_id, &left.model_id, &left.route_digest).cmp(&(
                &right.provider_id,
                &right.model_id,
                &right.route_digest,
            ))
        });
        if entries
            .windows(2)
            .any(|pair| pair[0].route_digest == pair[1].route_digest)
        {
            return Err("fallback_allowlist_duplicate_route".to_owned());
        }
        let mut allowlist = Self {
            schema: FALLBACK_ADMISSION_SCHEMA.to_owned(),
            version: FALLBACK_ADMISSION_VERSION,
            entries,
            allowlist_digest: String::new(),
        };
        allowlist.allowlist_digest = allowlist.digest();
        allowlist.validate()?;
        Ok(allowlist)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != FALLBACK_ADMISSION_SCHEMA
            || !self.version.is_compatible_with(&FALLBACK_ADMISSION_VERSION)
            || self.entries.is_empty()
            || self.entries.len() > MAX_FALLBACK_ROUTES
            || self.allowlist_digest != self.digest()
        {
            return Err("fallback_allowlist_invalid".to_owned());
        }
        digest(&self.allowlist_digest, "fallback_allowlist_digest")?;
        for entry in &self.entries {
            entry.validate()?;
        }
        if self.entries.windows(2).any(|pair| {
            (
                &pair[0].provider_id,
                &pair[0].model_id,
                &pair[0].route_digest,
            ) >= (
                &pair[1].provider_id,
                &pair[1].model_id,
                &pair[1].route_digest,
            )
        }) {
            return Err("fallback_allowlist_noncanonical".to_owned());
        }
        Ok(())
    }

    pub fn contains(&self, route: &ModelRoute) -> Result<(), String> {
        self.validate()?;
        let route_digest = route.digest();
        self.entries
            .iter()
            .any(|entry| {
                entry.route_digest == route_digest
                    && entry.provider_id == route.provider_id
                    && entry.model_id == route.model_id
            })
            .then_some(())
            .ok_or_else(|| "fallback_route_not_allowlisted".to_owned())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "entries": self.entries,
        }))
    }
}

/// Server-owned facts that must be current when choosing a fallback.  None of these values are
/// accepted from provider wire data or model text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackAdmissionRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub original_attempt_id: AttemptId,
    pub original_route_digest: String,
    pub authority_digest: String,
    pub authority_epoch: u64,
    pub data_boundary_digest: String,
    pub budget_digest: String,
    pub original_budget_reservation_digest: String,
    pub original_permit_digest: String,
    pub requirements: FallbackCapabilityRequirements,
    pub now_unix_ms: u64,
    pub request_digest: String,
}

impl FallbackAdmissionRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        run_id: RunId,
        original_attempt_id: AttemptId,
        original_route: &ModelRoute,
        authority_digest: impl Into<String>,
        authority_epoch: u64,
        data_boundary_digest: impl Into<String>,
        budget_digest: impl Into<String>,
        original_budget_reservation_digest: impl Into<String>,
        original_permit_digest: impl Into<String>,
        requirements: FallbackCapabilityRequirements,
        now_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: FALLBACK_ADMISSION_SCHEMA.to_owned(),
            version: FALLBACK_ADMISSION_VERSION,
            run_id,
            original_attempt_id,
            original_route_digest: original_route.digest(),
            authority_digest: authority_digest.into(),
            authority_epoch,
            data_boundary_digest: data_boundary_digest.into(),
            budget_digest: budget_digest.into(),
            original_budget_reservation_digest: original_budget_reservation_digest.into(),
            original_permit_digest: original_permit_digest.into(),
            requirements,
            now_unix_ms,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != FALLBACK_ADMISSION_SCHEMA
            || !self.version.is_compatible_with(&FALLBACK_ADMISSION_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.original_attempt_id.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.now_unix_ms == 0
            || self.request_digest != self.digest()
        {
            return Err("fallback_admission_request_invalid".to_owned());
        }
        digest(
            &self.original_route_digest,
            "fallback_original_route_digest",
        )?;
        digest(&self.authority_digest, "fallback_authority_digest")?;
        digest(&self.data_boundary_digest, "fallback_data_boundary_digest")?;
        digest(&self.budget_digest, "fallback_budget_digest")?;
        digest(
            &self.original_budget_reservation_digest,
            "fallback_original_budget_reservation_digest",
        )?;
        digest(
            &self.original_permit_digest,
            "fallback_original_permit_digest",
        )?;
        self.requirements.validate()
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "original_attempt_id": self.original_attempt_id,
            "original_route_digest": self.original_route_digest,
            "authority_digest": self.authority_digest,
            "authority_epoch": self.authority_epoch,
            "data_boundary_digest": self.data_boundary_digest,
            "budget_digest": self.budget_digest,
            "original_budget_reservation_digest": self.original_budget_reservation_digest,
            "original_permit_digest": self.original_permit_digest,
            "requirements": self.requirements,
            "now_unix_ms": self.now_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackAdmissionCandidate {
    pub route: ModelRoute,
    pub capabilities: ModelCapabilities,
    pub authority_digest: String,
    pub authority_epoch: u64,
    pub data_boundary_digest: String,
    pub credential_revision: Option<String>,
    pub budget_digest: String,
    pub budget_reservation_digest: Option<String>,
    pub rate_card: Option<RateCard>,
    pub permit_digest: String,
}

impl FallbackAdmissionCandidate {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        route: ModelRoute,
        capabilities: ModelCapabilities,
        authority_digest: impl Into<String>,
        authority_epoch: u64,
        data_boundary_digest: impl Into<String>,
        credential_revision: Option<String>,
        budget_digest: impl Into<String>,
        budget_reservation_digest: Option<String>,
        rate_card: Option<RateCard>,
        permit_digest: impl Into<String>,
    ) -> Self {
        Self {
            route,
            capabilities,
            authority_digest: authority_digest.into(),
            authority_epoch,
            data_boundary_digest: data_boundary_digest.into(),
            credential_revision,
            budget_digest: budget_digest.into(),
            budget_reservation_digest,
            rate_card,
            permit_digest: permit_digest.into(),
        }
    }

    pub fn validate(&self, now_unix_ms: u64) -> Result<(), String> {
        required(&self.route.provider_id, "fallback_candidate_provider", 256)?;
        required(
            &self.route.connection_id,
            "fallback_candidate_connection",
            256,
        )?;
        required(&self.route.model_id, "fallback_candidate_model", 256)?;
        required(&self.route.profile, "fallback_candidate_profile", 256)?;
        required(
            &self.route.configuration_revision,
            "fallback_candidate_configuration_revision",
            256,
        )?;
        if self.authority_epoch == 0 || now_unix_ms == 0 {
            return Err("fallback_candidate_authority_invalid".to_owned());
        }
        digest(
            &self.authority_digest,
            "fallback_candidate_authority_digest",
        )?;
        digest(
            &self.data_boundary_digest,
            "fallback_candidate_data_boundary_digest",
        )?;
        digest(&self.budget_digest, "fallback_candidate_budget_digest")?;
        let reservation = self
            .budget_reservation_digest
            .as_deref()
            .ok_or_else(|| "fallback_budget_reservation_missing".to_owned())?;
        digest(reservation, "fallback_candidate_budget_reservation_digest")?;
        digest(&self.permit_digest, "fallback_candidate_permit_digest")?;
        let credential = self
            .credential_revision
            .as_deref()
            .ok_or_else(|| "fallback_credential_missing".to_owned())?;
        digest(credential, "fallback_candidate_credential_revision")?;
        if self.capabilities.context_window == 0
            || self.capabilities.max_output == 0
            || self.capabilities.source.trim().is_empty()
            || self.capabilities.revision.trim().is_empty()
        {
            return Err("fallback_candidate_capabilities_invalid".to_owned());
        }
        let card = self
            .rate_card
            .as_ref()
            .ok_or_else(|| "fallback_rate_card_missing".to_owned())?;
        card.validate()?;
        if !card.is_effective_at(now_unix_ms)
            || card.provider_id != self.route.provider_id
            || (card.model_selector != "*" && card.model_selector != self.route.model_id)
        {
            return Err("fallback_rate_card_scope_mismatch".to_owned());
        }
        Ok(())
    }
}

/// The result of one ControlPlane fallback admission.  The next attempt owns a fresh route,
/// budget reservation, permit and price-card binding; the original attempt is only a history link.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackAttemptAdmission {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub original_attempt_id: AttemptId,
    pub attempt_id: AttemptId,
    pub route: ModelRoute,
    pub route_digest: String,
    pub authority_digest: String,
    pub authority_epoch: u64,
    pub data_boundary_digest: String,
    pub budget_digest: String,
    pub budget_reservation_digest: String,
    pub credential_revision: String,
    pub rate_card_id: RateCardId,
    pub rate_card_version: u64,
    pub rate_card_digest: String,
    pub permit_digest: String,
    pub capability_digest: String,
    pub admitted_at_unix_ms: u64,
    pub admission_digest: String,
}

impl FallbackAttemptAdmission {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != FALLBACK_ADMISSION_SCHEMA
            || !self.version.is_compatible_with(&FALLBACK_ADMISSION_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.original_attempt_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || self.original_attempt_id == self.attempt_id
            || self.route.digest() != self.route_digest
            || self.authority_epoch == 0
            || self.rate_card_id.as_uuid().is_nil()
            || self.rate_card_version == 0
            || self.admitted_at_unix_ms == 0
            || self.admission_digest != self.digest()
        {
            return Err("fallback_attempt_admission_invalid".to_owned());
        }
        required(&self.route.provider_id, "fallback_admission_provider", 256)?;
        required(
            &self.route.connection_id,
            "fallback_admission_connection",
            256,
        )?;
        required(&self.route.model_id, "fallback_admission_model", 256)?;
        required(&self.route.profile, "fallback_admission_profile", 256)?;
        required(
            &self.route.configuration_revision,
            "fallback_admission_configuration_revision",
            256,
        )?;
        for (value, field) in [
            (&self.route_digest, "fallback_admission_route_digest"),
            (
                &self.authority_digest,
                "fallback_admission_authority_digest",
            ),
            (
                &self.data_boundary_digest,
                "fallback_admission_data_boundary_digest",
            ),
            (&self.budget_digest, "fallback_admission_budget_digest"),
            (
                &self.budget_reservation_digest,
                "fallback_admission_budget_reservation_digest",
            ),
            (
                &self.credential_revision,
                "fallback_admission_credential_revision",
            ),
            (
                &self.rate_card_digest,
                "fallback_admission_rate_card_digest",
            ),
            (&self.permit_digest, "fallback_admission_permit_digest"),
            (
                &self.capability_digest,
                "fallback_admission_capability_digest",
            ),
            (&self.admission_digest, "fallback_admission_digest"),
        ] {
            digest(value, field)?;
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "original_attempt_id": self.original_attempt_id,
            "attempt_id": self.attempt_id,
            "route": self.route,
            "route_digest": self.route_digest,
            "authority_digest": self.authority_digest,
            "authority_epoch": self.authority_epoch,
            "data_boundary_digest": self.data_boundary_digest,
            "budget_digest": self.budget_digest,
            "budget_reservation_digest": self.budget_reservation_digest,
            "credential_revision": self.credential_revision,
            "rate_card_id": self.rate_card_id,
            "rate_card_version": self.rate_card_version,
            "rate_card_digest": self.rate_card_digest,
            "permit_digest": self.permit_digest,
            "capability_digest": self.capability_digest,
            "admitted_at_unix_ms": self.admitted_at_unix_ms,
        }))
    }

    pub const fn is_fresh_attempt(&self) -> bool {
        self.original_attempt_id != self.attempt_id
    }
}

/// Recheck all server-owned fallback inputs and produce a fresh attempt admission.  This helper
/// cannot send a request, consume a permit or treat an old `Unknown` attempt as success.
pub fn admit_fallback_attempt(
    request: &FallbackAdmissionRequest,
    candidate: &FallbackAdmissionCandidate,
    allowlist: &FallbackRouteAllowlist,
    new_attempt_id: AttemptId,
) -> Result<FallbackAttemptAdmission, String> {
    request.validate()?;
    candidate.validate(request.now_unix_ms)?;
    allowlist.contains(&candidate.route)?;
    if new_attempt_id.as_uuid().is_nil() || new_attempt_id == request.original_attempt_id {
        return Err("fallback_attempt_identity_invalid".to_owned());
    }
    let original_route_digest = request.original_route_digest.as_str();
    if original_route_digest == candidate.route.digest() {
        return Err("fallback_route_must_change".to_owned());
    }
    same_digest(
        &candidate.authority_digest,
        &request.authority_digest,
        "fallback_authority_recheck_failed",
    )?;
    if candidate.authority_epoch != request.authority_epoch {
        return Err("fallback_authority_epoch_stale".to_owned());
    }
    same_digest(
        &candidate.data_boundary_digest,
        &request.data_boundary_digest,
        "fallback_data_boundary_mismatch",
    )?;
    same_digest(
        &candidate.budget_digest,
        &request.budget_digest,
        "fallback_budget_recheck_required",
    )?;
    let reservation = candidate
        .budget_reservation_digest
        .as_deref()
        .ok_or_else(|| "fallback_budget_reservation_missing".to_owned())?;
    if reservation == request.original_budget_reservation_digest {
        return Err("fallback_cannot_reuse_original_budget_reservation".to_owned());
    }
    if candidate.permit_digest == request.original_permit_digest {
        return Err("fallback_cannot_reuse_original_permit".to_owned());
    }
    request.requirements.accepts(&candidate.capabilities)?;
    if request.requirements.require_streaming && !candidate.route.streaming {
        return Err("fallback_capability_downgrade_rejected".to_owned());
    }
    let card = candidate
        .rate_card
        .as_ref()
        .ok_or_else(|| "fallback_rate_card_missing".to_owned())?;
    let mut admission = FallbackAttemptAdmission {
        schema: FALLBACK_ADMISSION_SCHEMA.to_owned(),
        version: FALLBACK_ADMISSION_VERSION,
        run_id: request.run_id,
        original_attempt_id: request.original_attempt_id,
        attempt_id: new_attempt_id,
        route: candidate.route.clone(),
        route_digest: candidate.route.digest(),
        authority_digest: candidate.authority_digest.clone(),
        authority_epoch: candidate.authority_epoch,
        data_boundary_digest: candidate.data_boundary_digest.clone(),
        budget_digest: candidate.budget_digest.clone(),
        budget_reservation_digest: reservation.to_owned(),
        credential_revision: candidate
            .credential_revision
            .clone()
            .ok_or_else(|| "fallback_credential_missing".to_owned())?,
        rate_card_id: card.rate_card_id,
        rate_card_version: card.card_version,
        rate_card_digest: card.rate_card_digest.clone(),
        permit_digest: candidate.permit_digest.clone(),
        capability_digest: json_digest(&candidate.capabilities),
        admitted_at_unix_ms: request.now_unix_ms,
        admission_digest: String::new(),
    };
    admission.admission_digest = admission.digest();
    admission.validate()?;
    Ok(admission)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackAttemptOutcome {
    Estimated,
    Measured,
    Unknown,
}

impl FallbackAttemptOutcome {
    pub const fn is_success(self) -> bool {
        matches!(self, Self::Estimated | Self::Measured)
    }
}

/// One fallback attempt's independent usage, pinned price card and receipt observation.  A receipt
/// from another attempt cannot satisfy these identity checks, and `Unknown` has no success path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackAttemptReceipt {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub route_digest: String,
    pub authority_digest: String,
    pub data_boundary_digest: String,
    pub rate_card_id: RateCardId,
    pub rate_card_version: u64,
    pub rate_card_digest: String,
    pub usage: NormalizedUsage,
    pub cost: CostBreakdown,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_receipt: Option<ProviderReceiptRef>,
    pub outcome: FallbackAttemptOutcome,
    pub receipt_digest: String,
}

impl FallbackAttemptReceipt {
    pub fn from_admission(
        admission: &FallbackAttemptAdmission,
        usage: NormalizedUsage,
        cost: CostBreakdown,
        provider_receipt: Option<ProviderReceiptRef>,
    ) -> Result<Self, String> {
        admission.validate()?;
        usage.validate()?;
        cost.validate()?;
        let outcome = match &cost.kind {
            CostBreakdownKind::Estimated { .. } => FallbackAttemptOutcome::Estimated,
            CostBreakdownKind::Measured { .. } => FallbackAttemptOutcome::Measured,
            CostBreakdownKind::Unknown { .. } => FallbackAttemptOutcome::Unknown,
        };
        let mut receipt = Self {
            schema: FALLBACK_ATTEMPT_RECEIPT_SCHEMA.to_owned(),
            version: FALLBACK_ADMISSION_VERSION,
            run_id: admission.run_id,
            attempt_id: admission.attempt_id,
            route_digest: admission.route_digest.clone(),
            authority_digest: admission.authority_digest.clone(),
            data_boundary_digest: admission.data_boundary_digest.clone(),
            rate_card_id: admission.rate_card_id,
            rate_card_version: admission.rate_card_version,
            rate_card_digest: admission.rate_card_digest.clone(),
            usage,
            cost,
            provider_receipt,
            outcome,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest();
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != FALLBACK_ATTEMPT_RECEIPT_SCHEMA
            || !self.version.is_compatible_with(&FALLBACK_ADMISSION_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || self.rate_card_id.as_uuid().is_nil()
            || self.rate_card_version == 0
            || self.receipt_digest != self.digest()
        {
            return Err("fallback_attempt_receipt_invalid".to_owned());
        }
        for (value, field) in [
            (&self.route_digest, "fallback_receipt_route_digest"),
            (&self.authority_digest, "fallback_receipt_authority_digest"),
            (
                &self.data_boundary_digest,
                "fallback_receipt_data_boundary_digest",
            ),
            (&self.rate_card_digest, "fallback_receipt_rate_card_digest"),
            (&self.receipt_digest, "fallback_receipt_digest"),
        ] {
            digest(value, field)?;
        }
        self.usage.validate()?;
        self.cost.validate()?;
        if self.usage.run_id != self.run_id || self.usage.attempt_id != self.attempt_id {
            return Err("fallback_receipt_usage_identity_mismatch".to_owned());
        }
        if self.cost.run_id != self.run_id || self.cost.attempt_id != Some(self.attempt_id) {
            return Err("fallback_receipt_cost_identity_mismatch".to_owned());
        }
        if self.usage.provider_id.is_none()
            || self.usage.requested_model_id.trim().is_empty()
            || self.usage.route_id.trim().is_empty()
        {
            return Err("fallback_receipt_usage_route_missing".to_owned());
        }
        match (&self.outcome, &self.cost.kind, &self.provider_receipt) {
            (
                FallbackAttemptOutcome::Estimated,
                CostBreakdownKind::Estimated { estimate },
                None,
            ) if estimate.rate_card_id == self.rate_card_id
                && estimate.rate_card_version == self.rate_card_version => {}
            (
                FallbackAttemptOutcome::Measured,
                CostBreakdownKind::Measured {
                    provider_receipt, ..
                },
                Some(receipt),
            ) if receipt.as_str() == provider_receipt.as_str() => {}
            (FallbackAttemptOutcome::Unknown, CostBreakdownKind::Unknown { .. }, None)
                if self.usage.confidence != UsageConfidence::Known => {}
            _ => return Err("fallback_receipt_cost_outcome_mismatch".to_owned()),
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "attempt_id": self.attempt_id,
            "route_digest": self.route_digest,
            "authority_digest": self.authority_digest,
            "data_boundary_digest": self.data_boundary_digest,
            "rate_card_id": self.rate_card_id,
            "rate_card_version": self.rate_card_version,
            "rate_card_digest": self.rate_card_digest,
            "usage": self.usage,
            "cost": self.cost,
            "provider_receipt": self.provider_receipt,
            "outcome": self.outcome,
        }))
    }

    pub const fn is_success(&self) -> bool {
        self.outcome.is_success()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FallbackReceiptApplyOutcome {
    Applied,
    Duplicate,
}

/// Process-local idempotency aid for fallback receipts.  It never turns an old Unknown into a
/// success and rejects a different receipt for the same attempt.
#[derive(Default)]
pub struct FallbackReceiptLedger {
    entries: BTreeMap<AttemptId, FallbackAttemptReceipt>,
}

impl FallbackReceiptLedger {
    pub fn apply(
        &mut self,
        receipt: FallbackAttemptReceipt,
    ) -> Result<FallbackReceiptApplyOutcome, String> {
        receipt.validate()?;
        if let Some(existing) = self.entries.get(&receipt.attempt_id) {
            if existing.receipt_digest == receipt.receipt_digest {
                return Ok(FallbackReceiptApplyOutcome::Duplicate);
            }
            return Err("fallback_attempt_receipt_conflict".to_owned());
        }
        self.entries.insert(receipt.attempt_id, receipt);
        Ok(FallbackReceiptApplyOutcome::Applied)
    }

    pub fn get(&self, attempt_id: AttemptId) -> Option<&FallbackAttemptReceipt> {
        self.entries.get(&attempt_id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// A stable reason helper for source guards and reconciliation projections.  It deliberately does
/// not infer success from an unknown provider result.
pub const fn fallback_unknown_reason() -> BillingUnknownReason {
    BillingUnknownReason::ProviderUnreported
}

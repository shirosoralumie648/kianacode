//! BQ-15 tool/effect/resource usage facts and receipt summaries.
//!
//! Usage is recorded only after the existing ControlPlane/Broker boundary has produced a
//! server-owned terminal fact.  The model cannot mint a usage entry: each observation is bound to
//! one Run/Invocation/attempt, owner scope, resource digest and lease digest.  Rejected or
//! not-started requests carry zero effect and never contribute to a successful invocation count.
//! Byte and wall-time fields are bounded before they can enter a receipt projection.

use crate::{
    json_digest, AttemptId, EventId, ExecutionId, InvocationId, RunId, SchemaVersion, UsageId,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const EFFECT_USAGE_SCHEMA: &str = "kiana.effect-usage.v1";
pub const EFFECT_USAGE_RECEIPT_SCHEMA: &str = "kiana.effect-usage-receipt.v1";
pub const EFFECT_USAGE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// Hard per-observation limits.  These are admission/receipt bounds, not a promise that an
/// adapter can retain unbounded output in memory.
pub const MAX_EFFECT_USAGE_INVOCATIONS: u64 = 1_024;
pub const MAX_EFFECT_USAGE_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_EFFECT_USAGE_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_EFFECT_USAGE_LOG_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_EFFECT_USAGE_STORAGE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_EFFECT_USAGE_WALL_TIME_MS: u64 = 24 * 60 * 60 * 1_000;
pub const MAX_EFFECT_USAGE_SOURCE_EVENTS: usize = 4_096;

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// The three ledgers remain separate even when one terminal fact carries several dimensions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageLedgerLayer {
    Model,
    Tool,
    Effect,
}

impl Default for UsageLedgerLayer {
    fn default() -> Self {
        Self::Model
    }
}

/// Resource kinds observed by the existing model/tool/Broker path.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectResourceKind {
    Model,
    Shell,
    Mcp,
    Artifact,
    Log,
    Storage,
}

impl EffectResourceKind {
    fn valid_for(self, layer: UsageLedgerLayer) -> bool {
        match layer {
            UsageLedgerLayer::Model => matches!(self, Self::Model),
            UsageLedgerLayer::Tool => matches!(self, Self::Shell | Self::Mcp),
            UsageLedgerLayer::Effect => {
                matches!(self, Self::Artifact | Self::Log | Self::Storage)
            }
        }
    }
}

/// Terminal classification from a committed Broker/handler fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectUsageState {
    Rejected,
    NotStarted,
    Started,
    Succeeded,
    Failed,
    Unknown,
}

impl EffectUsageState {
    pub const fn is_success(self) -> bool {
        matches!(self, Self::Succeeded)
    }

    pub const fn is_unknown(self) -> bool {
        matches!(self, Self::Unknown)
    }

    pub const fn is_not_started(self) -> bool {
        matches!(self, Self::Rejected | Self::NotStarted)
    }
}

/// A single immutable, server-bound usage observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectUsageObservation {
    pub schema: String,
    pub version: SchemaVersion,
    pub usage_id: UsageId,
    pub run_id: RunId,
    pub invocation_id: InvocationId,
    pub execution_id: ExecutionId,
    pub attempt_id: AttemptId,
    pub attempt: u32,
    pub layer: UsageLedgerLayer,
    pub resource: EffectResourceKind,
    /// Digest of the server-owned principal/project/scope owner.
    pub owner_digest: String,
    /// Digest of the lease/fencing material checked at the effect boundary.
    pub lease_digest: String,
    /// Digest of the canonical resource/path set; raw paths never enter the usage fact.
    pub resource_digest: String,
    pub started_count: u64,
    pub successful_count: u64,
    pub output_bytes: u64,
    pub artifact_bytes: u64,
    pub log_bytes: u64,
    pub storage_bytes: u64,
    pub wall_time_ms: u64,
    pub state: EffectUsageState,
    pub source_event_id: EventId,
    pub source_cursor: u64,
    pub usage_digest: String,
}

impl EffectUsageObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        usage_id: UsageId,
        run_id: RunId,
        invocation_id: InvocationId,
        execution_id: ExecutionId,
        attempt_id: AttemptId,
        attempt: u32,
        layer: UsageLedgerLayer,
        resource: EffectResourceKind,
        owner_digest: impl Into<String>,
        lease_digest: impl Into<String>,
        resource_digest: impl Into<String>,
        started_count: u64,
        successful_count: u64,
        output_bytes: u64,
        artifact_bytes: u64,
        log_bytes: u64,
        storage_bytes: u64,
        wall_time_ms: u64,
        state: EffectUsageState,
        source_event_id: EventId,
        source_cursor: u64,
    ) -> Result<Self, String> {
        let mut usage = Self {
            schema: EFFECT_USAGE_SCHEMA.to_owned(),
            version: EFFECT_USAGE_VERSION,
            usage_id,
            run_id,
            invocation_id,
            execution_id,
            attempt_id,
            attempt,
            layer,
            resource,
            owner_digest: owner_digest.into(),
            lease_digest: lease_digest.into(),
            resource_digest: resource_digest.into(),
            started_count,
            successful_count,
            output_bytes,
            artifact_bytes,
            log_bytes,
            storage_bytes,
            wall_time_ms,
            state,
            source_event_id,
            source_cursor,
            usage_digest: String::new(),
        };
        usage.usage_digest = usage.digest();
        usage.validate()?;
        Ok(usage)
    }

    pub fn from_json(value: &serde_json::Value) -> Result<Self, String> {
        let usage: Self = serde_json::from_value(value.clone())
            .map_err(|_| "effect_usage_decode_failed".to_owned())?;
        usage.validate()?;
        Ok(usage)
    }

    pub fn to_json(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(self).map_err(|_| "effect_usage_encode_failed".to_owned())
    }

    /// Replace the adapter's untrusted timing hint with the Broker's measured wall time and
    /// reseal the immutable fact before it reaches EventLog/Receipt projection.
    pub fn with_server_wall_time_ms(mut self, wall_time_ms: u64) -> Result<Self, String> {
        self.wall_time_ms = wall_time_ms;
        self.usage_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    /// Revalidate identity at the handler/receipt boundary.  A path, lease, owner, run or
    /// attempt drift is an error and cannot be represented as a successful observation.
    pub fn validate_binding(
        &self,
        run_id: RunId,
        invocation_id: InvocationId,
        attempt_id: AttemptId,
        attempt: u32,
        owner_digest: &str,
        lease_digest: &str,
        resource_digest: &str,
    ) -> Result<(), String> {
        self.validate()?;
        if self.run_id != run_id {
            return Err("effect_usage_run_mismatch".to_owned());
        }
        if self.invocation_id != invocation_id {
            return Err("effect_usage_invocation_mismatch".to_owned());
        }
        if self.attempt_id != attempt_id || self.attempt != attempt {
            return Err("effect_usage_attempt_mismatch".to_owned());
        }
        if self.owner_digest != owner_digest {
            return Err("effect_usage_owner_mismatch".to_owned());
        }
        if self.lease_digest != lease_digest {
            return Err("effect_usage_lease_mismatch".to_owned());
        }
        if self.resource_digest != resource_digest {
            return Err("effect_usage_resource_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EFFECT_USAGE_SCHEMA
            || !self.version.is_compatible_with(&EFFECT_USAGE_VERSION)
            || self.usage_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.execution_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || self.attempt == 0
            || !self.resource.valid_for(self.layer)
            || self.source_event_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.started_count > MAX_EFFECT_USAGE_INVOCATIONS
            || self.successful_count > self.started_count
            || self.output_bytes > MAX_EFFECT_USAGE_OUTPUT_BYTES
            || self.artifact_bytes > MAX_EFFECT_USAGE_ARTIFACT_BYTES
            || self.log_bytes > MAX_EFFECT_USAGE_LOG_BYTES
            || self.storage_bytes > MAX_EFFECT_USAGE_STORAGE_BYTES
            || self.wall_time_ms > MAX_EFFECT_USAGE_WALL_TIME_MS
        {
            return Err("effect_usage_header_or_bound_invalid".to_owned());
        }
        for (value, field) in [
            (&self.owner_digest, "effect_usage_owner_digest"),
            (&self.lease_digest, "effect_usage_lease_digest"),
            (&self.resource_digest, "effect_usage_resource_digest"),
            (&self.usage_digest, "effect_usage_digest"),
        ] {
            valid_digest(value, field)?;
        }
        if self.state.is_not_started()
            && (self.started_count != 0
                || self.successful_count != 0
                || self.output_bytes != 0
                || self.artifact_bytes != 0
                || self.log_bytes != 0
                || self.storage_bytes != 0
                || self.wall_time_ms != 0)
        {
            return Err("effect_usage_not_started_has_effect".to_owned());
        }
        if !self.state.is_not_started() && self.started_count == 0 {
            return Err("effect_usage_started_count_required".to_owned());
        }
        if self.state.is_success() && self.successful_count == 0 {
            return Err("effect_usage_success_count_required".to_owned());
        }
        if self.usage_digest != self.digest() {
            return Err("effect_usage_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "usage_id": self.usage_id,
            "run_id": self.run_id,
            "invocation_id": self.invocation_id,
            "execution_id": self.execution_id,
            "attempt_id": self.attempt_id,
            "attempt": self.attempt,
            "layer": self.layer,
            "resource": self.resource,
            "owner_digest": self.owner_digest,
            "lease_digest": self.lease_digest,
            "resource_digest": self.resource_digest,
            "started_count": self.started_count,
            "successful_count": self.successful_count,
            "output_bytes": self.output_bytes,
            "artifact_bytes": self.artifact_bytes,
            "log_bytes": self.log_bytes,
            "storage_bytes": self.storage_bytes,
            "wall_time_ms": self.wall_time_ms,
            "state": self.state,
            "source_event_id": self.source_event_id,
            "source_cursor": self.source_cursor,
        }))
    }
}

/// Bounded per-layer receipt counters.  Rejected/not-started observations are retained as
/// counters but contribute no started/success/effect bytes.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageLayerSummary {
    pub layer: UsageLedgerLayer,
    pub started_invocations: u64,
    pub successful_invocations: u64,
    pub failed_invocations: u64,
    pub unknown_invocations: u64,
    pub rejected_invocations: u64,
    pub not_started_invocations: u64,
    pub output_bytes: u64,
    pub artifact_bytes: u64,
    pub log_bytes: u64,
    pub storage_bytes: u64,
    pub wall_time_ms: u64,
}

impl UsageLayerSummary {
    fn validate(&self) -> Result<(), String> {
        if self.successful_invocations > self.started_invocations
            || self.started_invocations > MAX_EFFECT_USAGE_INVOCATIONS
            || self.successful_invocations > MAX_EFFECT_USAGE_INVOCATIONS
            || self.output_bytes > MAX_EFFECT_USAGE_OUTPUT_BYTES
            || self.artifact_bytes > MAX_EFFECT_USAGE_ARTIFACT_BYTES
            || self.log_bytes > MAX_EFFECT_USAGE_LOG_BYTES
            || self.storage_bytes > MAX_EFFECT_USAGE_STORAGE_BYTES
            || self.wall_time_ms > MAX_EFFECT_USAGE_WALL_TIME_MS
        {
            return Err("effect_usage_layer_bounds_invalid".to_owned());
        }
        Ok(())
    }

    fn add(&mut self, usage: &EffectUsageObservation) -> Result<(), String> {
        let add = |left: &mut u64, right: u64, max: u64| -> Result<(), String> {
            *left = left
                .checked_add(right)
                .ok_or_else(|| "effect_usage_receipt_overflow".to_owned())?;
            if *left > max {
                return Err("effect_usage_receipt_bound_exceeded".to_owned());
            }
            Ok(())
        };
        add(
            &mut self.started_invocations,
            usage.started_count,
            MAX_EFFECT_USAGE_INVOCATIONS,
        )?;
        add(
            &mut self.successful_invocations,
            usage.successful_count,
            MAX_EFFECT_USAGE_INVOCATIONS,
        )?;
        match usage.state {
            EffectUsageState::Rejected => {
                self.rejected_invocations = self
                    .rejected_invocations
                    .checked_add(1)
                    .ok_or_else(|| "effect_usage_receipt_overflow".to_owned())?
            }
            EffectUsageState::NotStarted => {
                self.not_started_invocations = self
                    .not_started_invocations
                    .checked_add(1)
                    .ok_or_else(|| "effect_usage_receipt_overflow".to_owned())?
            }
            EffectUsageState::Failed => {
                self.failed_invocations = self
                    .failed_invocations
                    .checked_add(1)
                    .ok_or_else(|| "effect_usage_receipt_overflow".to_owned())?
            }
            EffectUsageState::Unknown => {
                self.unknown_invocations = self
                    .unknown_invocations
                    .checked_add(1)
                    .ok_or_else(|| "effect_usage_receipt_overflow".to_owned())?
            }
            EffectUsageState::Started | EffectUsageState::Succeeded => {}
        }
        add(
            &mut self.output_bytes,
            usage.output_bytes,
            MAX_EFFECT_USAGE_OUTPUT_BYTES,
        )?;
        add(
            &mut self.artifact_bytes,
            usage.artifact_bytes,
            MAX_EFFECT_USAGE_ARTIFACT_BYTES,
        )?;
        add(
            &mut self.log_bytes,
            usage.log_bytes,
            MAX_EFFECT_USAGE_LOG_BYTES,
        )?;
        add(
            &mut self.storage_bytes,
            usage.storage_bytes,
            MAX_EFFECT_USAGE_STORAGE_BYTES,
        )?;
        add(
            &mut self.wall_time_ms,
            usage.wall_time_ms,
            MAX_EFFECT_USAGE_WALL_TIME_MS,
        )?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageVerification {
    Complete,
    Partial,
    Unknown,
}

/// Read-only usage projection embedded in a Receipt.  It is rebuilt from committed terminal
/// facts and never authorizes a capability or mutates the EventLog/budget ledger.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectUsageReceipt {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub layers: Vec<UsageLayerSummary>,
    pub verification: UsageVerification,
    pub receipt_digest: String,
}

impl EffectUsageReceipt {
    pub fn from_observations(
        run_id: RunId,
        observations: impl IntoIterator<Item = EffectUsageObservation>,
    ) -> Result<Self, String> {
        if run_id.as_uuid().is_nil() {
            return Err("effect_usage_receipt_run_invalid".to_owned());
        }
        let mut by_usage = BTreeMap::<UsageId, (String, EventId)>::new();
        let mut source_event_ids = BTreeSet::new();
        let mut source_cursor = 0;
        let mut summaries = BTreeMap::<UsageLedgerLayer, UsageLayerSummary>::new();
        let mut verification = UsageVerification::Complete;
        for usage in observations {
            usage.validate()?;
            if usage.run_id != run_id {
                return Err("effect_usage_receipt_run_mismatch".to_owned());
            }
            if let Some((previous_digest, previous_event)) = by_usage.insert(
                usage.usage_id,
                (usage.usage_digest.clone(), usage.source_event_id),
            ) {
                if previous_digest != usage.usage_digest || previous_event != usage.source_event_id
                {
                    return Err("effect_usage_duplicate_conflict".to_owned());
                }
                continue;
            }
            if !source_event_ids.insert(usage.source_event_id) {
                return Err("effect_usage_duplicate_source_event".to_owned());
            }
            source_cursor = source_cursor.max(usage.source_cursor);
            if usage.state.is_unknown() {
                verification = UsageVerification::Unknown;
            }
            let summary = summaries
                .entry(usage.layer)
                .or_insert_with(|| UsageLayerSummary {
                    layer: usage.layer,
                    ..UsageLayerSummary::default()
                });
            summary.add(&usage)?;
        }
        if source_event_ids.is_empty() || source_event_ids.len() > MAX_EFFECT_USAGE_SOURCE_EVENTS {
            return Err("effect_usage_receipt_source_empty_or_bounded".to_owned());
        }
        if verification == UsageVerification::Complete
            && summaries.values().any(|summary| {
                summary.rejected_invocations > 0 || summary.not_started_invocations > 0
            })
        {
            verification = UsageVerification::Partial;
        }
        let mut receipt = Self {
            schema: EFFECT_USAGE_RECEIPT_SCHEMA.to_owned(),
            version: EFFECT_USAGE_VERSION,
            run_id,
            source_cursor,
            source_event_ids: source_event_ids.into_iter().collect(),
            layers: summaries.into_values().collect(),
            verification,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest();
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EFFECT_USAGE_RECEIPT_SCHEMA
            || !self.version.is_compatible_with(&EFFECT_USAGE_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.len() > MAX_EFFECT_USAGE_SOURCE_EVENTS
            || self
                .source_event_ids
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self
                .layers
                .windows(2)
                .any(|pair| pair[0].layer >= pair[1].layer)
        {
            return Err("effect_usage_receipt_header_invalid".to_owned());
        }
        valid_digest(&self.receipt_digest, "effect_usage_receipt_digest")?;
        if self.receipt_digest != self.digest() {
            return Err("effect_usage_receipt_digest_mismatch".to_owned());
        }
        for summary in &self.layers {
            summary.validate()?;
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "source_cursor": self.source_cursor,
            "source_event_ids": self.source_event_ids,
            "layers": self.layers,
            "verification": self.verification,
        }))
    }

    pub fn layer(&self, layer: UsageLedgerLayer) -> Option<&UsageLayerSummary> {
        self.layers.iter().find(|summary| summary.layer == layer)
    }
}

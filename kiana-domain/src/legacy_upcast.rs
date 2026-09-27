//! BQ-27 legacy usage/cassette/config upcasters and migration receipts.
//!
//! Old local data has to keep working, and it has to keep working *honestly*. The three failures
//! this module exists to prevent are all the same failure wearing different clothes: a legacy
//! payload being made to say more than it ever said.
//!
//! 1. An unknown schema major is never best-effort parsed. [`classify_legacy_schema`] returns the
//!    typed `legacy_upcast_unknown_major` refusal, the same rule
//!    [`crate::upcast_storage_value`] and [`crate::upcast_security_registry`] already apply.
//!    A different major of a *known* family is refused too, because nothing here knows what that
//!    major meant.
//! 2. A legacy `cost_micros: 0` is **not** a measured zero. It is what a struct default wrote when
//!    nobody knew the cost, so [`LegacyMeasureOrigin::LegacyUnsetZero`] never promotes. More
//!    strongly: a legacy cost never promotes at all, even when the file carries a positive number,
//!    because an old ledger has no rate-card binding and no provider receipt and therefore cannot
//!    produce a measured total. `LegacyUpcastTotals::cost_micros` is structurally absent.
//! 3. Re-running a migration is append-only. [`LegacyUpcastHistory::append`] treats a replayed
//!    report as a no-op and refuses a second, different report for the same legacy source instead
//!    of overwriting what was already recorded.
//!
//! This module produces reports. It does not read a file, write a payload, take a lock, run a
//! migration, or prove anything about a real store. [`crate::MigrationRegistry`],
//! [`crate::MigrationRecord`] and the adapters own those.

use crate::{
    json_digest, redact_text, scan_secret_sentinels, BillingUnknownReason, ModelProtocol,
    ProviderConfigSource, ProviderSelectionMode, SchemaVersion, SecretScanChannel,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const LEGACY_UPCAST_REQUEST_SCHEMA: &str = "kiana.legacy-upcast-request.v1";
pub const LEGACY_UPCAST_REPORT_SCHEMA: &str = "kiana.legacy-upcast-report.v1";
pub const LEGACY_UPCAST_FIELD_SCHEMA: &str = "kiana.legacy-upcast-field.v1";
pub const LEGACY_UPCAST_TOTALS_SCHEMA: &str = "kiana.legacy-upcast-totals.v1";
pub const LEGACY_UPCAST_ROLLBACK_SCHEMA: &str = "kiana.legacy-upcast-rollback.v1";
pub const LEGACY_UPCAST_HISTORY_SCHEMA: &str = "kiana.legacy-upcast-history.v1";
pub const LEGACY_UPCAST_RECORD_SCHEMA: &str = "kiana.legacy-upcast-record.v1";
pub const LEGACY_UPCAST_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// The pre-BQ-01 usage fold. `cost_micros` is a bare `u64` here, which is the whole problem: a
/// zero written by a default cannot be told apart from a measured zero at the type level.
pub const LEGACY_COST_LEDGER_SCHEMA: &str = "kiana.cost-ledger.v0";
/// The pre-BQ-07 quota. It carries limits only; it has no window, no group and no consumption
/// counters, so every `used_*` field on today's [`crate::QuotaWindowBudget`] is absent after the
/// upcast and must never be written as `0`.
pub const LEGACY_QUOTA_SCHEMA: &str = "kiana.quota.v0";
/// The pre-BQ-20 harness script (`KIANA_HARNESS_SCRIPT`). It may carry token counts per output and
/// carries no cost, no rate card and no provider receipt.
pub const LEGACY_CASSETTE_SCHEMA: &str = "kiana.harness-cassette.v0";
/// The pre-BQ-04 provider configuration. It has profiles with no `profile_version`, so the version
/// on the upcast snapshot is assigned by the upcaster and is recorded as assigned, not reported.
pub const LEGACY_PROVIDER_CONFIG_SCHEMA: &str = "kiana.provider-config.v0";

pub const MAX_LEGACY_UPCAST_FIELDS: usize = 64;
pub const MAX_LEGACY_HISTORY_RECORDS: usize = 4_096;
pub const MAX_LEGACY_TEXT: usize = 256;
pub const MAX_LEGACY_CASSETTE_OUTPUTS: usize = 256;
pub const MAX_LEGACY_CASSETTE_TEXT: usize = 8_192;
pub const MAX_LEGACY_PROFILES: usize = 64;

/// How a legacy field's number came to be there, and therefore what it is allowed to become.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyMeasureOrigin {
    /// The legacy payload carried a strictly positive, explicitly reported value.
    Reported,
    /// The legacy payload omitted the field entirely.
    Absent,
    /// The legacy payload carried a literal zero. Legacy structs wrote zero for "not known", so a
    /// legacy zero is an absent measurement and never a measured zero.
    LegacyUnsetZero,
}

impl LegacyMeasureOrigin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reported => "reported",
            Self::Absent => "absent",
            Self::LegacyUnsetZero => "legacy_unset_zero",
        }
    }

    /// Classify one legacy JSON number. A missing key and an explicit `null` are both `Absent`;
    /// `Some(0)` is `LegacyUnsetZero`, never `Reported`.
    pub fn of(value: Option<&Value>) -> Self {
        match value {
            None | Some(Value::Null) => Self::Absent,
            Some(Value::Number(number)) if number.as_u64() == Some(0) => Self::LegacyUnsetZero,
            Some(_) => Self::Reported,
        }
    }
}

/// What class of number a legacy field is, which decides whether a reported value may survive.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyFieldClass {
    /// A bound from the old struct: a budget, a window size, a request cap.
    Limit,
    /// A count of something that happened: tokens, requests, concurrent work.
    Usage,
    /// Money. Never promotable from a legacy payload.
    Cost,
}

impl LegacyFieldClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Limit => "limit",
            Self::Usage => "usage",
            Self::Cost => "cost",
        }
    }
}

/// One legacy field and what the upcaster did with it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyUpcastField {
    pub schema: String,
    pub field: String,
    pub class: LegacyFieldClass,
    pub source: LegacyMeasureOrigin,
    /// The value carried into the current shape. `None` whenever the value was not really known.
    pub promoted: Option<u64>,
    /// Present exactly when the value is not promotable, so an unknown is never silent.
    pub unknown_reason: Option<BillingUnknownReason>,
    /// Stable code saying why the value is or is not promotable.
    pub reason: String,
    pub field_digest: String,
}

impl LegacyUpcastField {
    pub fn new(
        field: impl Into<String>,
        class: LegacyFieldClass,
        source: LegacyMeasureOrigin,
        promoted: Option<u64>,
        unknown_reason: Option<BillingUnknownReason>,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: LEGACY_UPCAST_FIELD_SCHEMA.to_owned(),
            field: field.into(),
            class,
            source,
            promoted,
            unknown_reason,
            reason: reason.into(),
            field_digest: String::new(),
        };
        value.field_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        safe_text(&self.field, "legacy_upcast_field_name")?;
        safe_text(&self.reason, "legacy_upcast_field_reason")?;
        if self.schema != LEGACY_UPCAST_FIELD_SCHEMA {
            return Err("legacy_upcast_field_header_invalid".to_owned());
        }
        // A promoted zero is the exact shape the card rejects. Nothing this module emits may ever
        // manufacture a numeric zero, whichever class the field belongs to.
        if self.promoted == Some(0) {
            return Err("legacy_upcast_explicit_zero_forbidden".to_owned());
        }
        // A field that was not really reported cannot be promoted, and must say so out loud.
        if self.source != LegacyMeasureOrigin::Reported
            && (self.promoted.is_some() || self.unknown_reason.is_none())
        {
            return Err("legacy_upcast_absent_field_promoted".to_owned());
        }
        // Money from a legacy payload is never promotable: there is no rate card and no provider
        // receipt behind it, so it can be reported but never summed, settled or reconciled.
        if self.class == LegacyFieldClass::Cost
            && (self.promoted.is_some() || self.unknown_reason.is_none())
        {
            return Err("legacy_upcast_measured_cost_forbidden".to_owned());
        }
        if self.source == LegacyMeasureOrigin::Reported
            && self.class != LegacyFieldClass::Cost
            && (self.promoted.is_none() || self.unknown_reason.is_some())
        {
            return Err("legacy_upcast_field_reported_not_promoted".to_owned());
        }
        valid_digest(&self.field_digest, "legacy_upcast_field_digest")?;
        if self.field_digest != self.digest() {
            return Err("legacy_upcast_field_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "field": self.field,
            "class": self.class,
            "source": self.source,
            "promoted": self.promoted,
            "unknown_reason": self.unknown_reason,
            "reason": self.reason,
        }))
    }
}

/// The totals an upcast may hand on. Both `None` fields are structural, not incidental: nothing
/// this module produces may ever claim a measured cost or a consumed quota.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyUpcastTotals {
    pub schema: String,
    /// How many fields carried a value into the current shape.
    pub promoted_fields: u32,
    /// How many fields stayed absent or unknown after the upcast.
    pub unknown_fields: u32,
    /// Always `None`. A legacy payload has no rate card and no provider receipt.
    pub cost_micros: Option<u64>,
    /// Always `None`. A legacy quota carries limits, never consumption counters.
    pub used_units: Option<u64>,
    pub totals_digest: String,
}

impl Default for LegacyUpcastTotals {
    fn default() -> Self {
        Self {
            schema: LEGACY_UPCAST_TOTALS_SCHEMA.to_owned(),
            promoted_fields: 0,
            unknown_fields: 0,
            cost_micros: None,
            used_units: None,
            totals_digest: String::new(),
        }
    }
}

impl LegacyUpcastTotals {
    /// Recount the fields. The two `None` totals are asserted here, not merely initialised.
    pub fn from_fields(fields: &[LegacyUpcastField]) -> Result<Self, String> {
        for field in fields {
            field.validate()?;
        }
        let mut totals = Self {
            promoted_fields: fields
                .iter()
                .filter(|field| field.promoted.is_some())
                .count() as u32,
            unknown_fields: fields
                .iter()
                .filter(|field| field.promoted.is_none())
                .count() as u32,
            ..Self::default()
        };
        totals.totals_digest = totals.digest();
        totals.validate_against(fields)?;
        Ok(totals)
    }

    pub fn validate_against(&self, fields: &[LegacyUpcastField]) -> Result<(), String> {
        if self.schema != LEGACY_UPCAST_TOTALS_SCHEMA
            || self.promoted_fields + self.unknown_fields != fields.len() as u32
            || self.promoted_fields
                != fields
                    .iter()
                    .filter(|field| field.promoted.is_some())
                    .count() as u32
            || self.unknown_fields
                != fields
                    .iter()
                    .filter(|field| field.promoted.is_none())
                    .count() as u32
        {
            return Err("legacy_upcast_field_totals_mismatch".to_owned());
        }
        if self.cost_micros.is_some() {
            return Err("legacy_upcast_measured_cost_forbidden".to_owned());
        }
        if self.used_units.is_some() {
            return Err("legacy_upcast_measured_usage_forbidden".to_owned());
        }
        valid_digest(&self.totals_digest, "legacy_upcast_totals_digest")?;
        if self.totals_digest != self.digest() {
            return Err("legacy_upcast_totals_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "promoted_fields": self.promoted_fields,
            "unknown_fields": self.unknown_fields,
            "cost_micros": self.cost_micros,
            "used_units": self.used_units,
        }))
    }
}

/// What an upcast cost to undo. The report is read-only by construction: it names the legacy bytes
/// to re-read and the backup a caller would need before any writeback, and it never performs one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyUpcastRollback {
    pub schema: String,
    /// Always `true`. Producing the report is not the same as writing it back.
    pub read_only: bool,
    /// A backup reference the caller must have verified before writing anything back. The domain
    /// neither creates nor checks it; its absence is recorded, not assumed good.
    pub verified_backup_ref: Option<String>,
    /// Digest of the legacy bytes to re-read when the upcast is discarded. Never the bytes.
    pub legacy_source_digest: String,
    pub rollback_digest: String,
}

impl LegacyUpcastRollback {
    pub fn new(
        verified_backup_ref: Option<String>,
        legacy_source_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut rollback = Self {
            schema: LEGACY_UPCAST_ROLLBACK_SCHEMA.to_owned(),
            read_only: true,
            verified_backup_ref,
            legacy_source_digest: legacy_source_digest.into(),
            rollback_digest: String::new(),
        };
        rollback.rollback_digest = rollback.digest();
        rollback.validate()?;
        Ok(rollback)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LEGACY_UPCAST_ROLLBACK_SCHEMA {
            return Err("legacy_upcast_rollback_header_invalid".to_owned());
        }
        // This module produces a report and nothing else. A report claiming it wrote anything is
        // rejected rather than reported.
        if !self.read_only {
            return Err("legacy_upcast_read_only_forbidden".to_owned());
        }
        if let Some(reference) = &self.verified_backup_ref {
            safe_text(reference, "legacy_upcast_backup_ref")?;
        }
        valid_digest(
            &self.legacy_source_digest,
            "legacy_upcast_legacy_source_digest",
        )?;
        valid_digest(&self.rollback_digest, "legacy_upcast_rollback_digest")?;
        if self.rollback_digest != self.digest() {
            return Err("legacy_upcast_rollback_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "read_only": self.read_only,
            "verified_backup_ref": self.verified_backup_ref,
            "legacy_source_digest": self.legacy_source_digest,
        }))
    }
}

/// Which legacy material this upcaster is for.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyPayloadKind {
    CostLedger,
    Quota,
    Cassette,
    ProviderConfig,
}

impl LegacyPayloadKind {
    pub const ALL: [Self; 4] = [
        Self::CostLedger,
        Self::Quota,
        Self::Cassette,
        Self::ProviderConfig,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CostLedger => "cost_ledger",
            Self::Quota => "quota",
            Self::Cassette => "cassette",
            Self::ProviderConfig => "provider_config",
        }
    }
}

/// One named upcaster. A legacy schema that is not named here has no upcaster, which is a typed
/// refusal rather than a best-effort parse.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyUpcastSource {
    pub kind: LegacyPayloadKind,
    pub from: &'static str,
    pub to: &'static str,
}

pub const LEGACY_UPCAST_SOURCES: &[LegacyUpcastSource] = &[
    LegacyUpcastSource {
        kind: LegacyPayloadKind::CostLedger,
        from: LEGACY_COST_LEDGER_SCHEMA,
        to: crate::NORMALIZED_USAGE_SCHEMA,
    },
    LegacyUpcastSource {
        kind: LegacyPayloadKind::Quota,
        from: LEGACY_QUOTA_SCHEMA,
        to: crate::QUOTA_WINDOW_BUDGET_SCHEMA,
    },
    LegacyUpcastSource {
        kind: LegacyPayloadKind::Cassette,
        from: LEGACY_CASSETTE_SCHEMA,
        to: crate::PROVIDER_CASSETTE_SCHEMA,
    },
    LegacyUpcastSource {
        kind: LegacyPayloadKind::ProviderConfig,
        from: LEGACY_PROVIDER_CONFIG_SCHEMA,
        to: crate::PROVIDER_CONFIG_SNAPSHOT_SCHEMA,
    },
];

pub fn legacy_upcast_registry_digest() -> String {
    let entries = LEGACY_UPCAST_SOURCES
        .iter()
        .map(|source| {
            json!({
                "kind": source.kind,
                "from": source.from,
                "to": source.to,
            })
        })
        .collect::<Vec<_>>();
    json_digest(&json!(entries))
}

pub fn legacy_upcast_source_schema(kind: LegacyPayloadKind) -> &'static str {
    LEGACY_UPCAST_SOURCES
        .iter()
        .find(|source| source.kind == kind)
        .map(|source| source.from)
        .unwrap_or("")
}

pub fn legacy_upcast_destination_schema(kind: LegacyPayloadKind) -> &'static str {
    LEGACY_UPCAST_SOURCES
        .iter()
        .find(|source| source.kind == kind)
        .map(|source| source.to)
        .unwrap_or("")
}

/// What a declared schema name is, decided before anything is parsed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacySchemaClass {
    /// A registered legacy source, upcastable by exactly one named upcaster.
    LegacySource(LegacyPayloadKind),
    /// A registered destination: the material has already been upcast.
    CurrentDestination(LegacyPayloadKind),
}

/// Classify a schema name the legacy payload itself declares. Only the exact registered source and
/// destination names are recognised. Anything else — an unknown family, or another major of a
/// family this module knows — is `legacy_upcast_unknown_major`, never a best-effort parse.
pub fn classify_legacy_schema(payload_schema: &str) -> Result<LegacySchemaClass, String> {
    for source in LEGACY_UPCAST_SOURCES {
        if payload_schema == source.from {
            return Ok(LegacySchemaClass::LegacySource(source.kind));
        }
        if payload_schema == source.to {
            return Ok(LegacySchemaClass::CurrentDestination(source.kind));
        }
    }
    Err("legacy_upcast_unknown_major".to_owned())
}

/// Parse one legacy payload under `deny_unknown_fields`. An unrecognised field is a typed refusal
/// rather than a silently dropped one: this module must not invent a translation for a field it
/// does not know, because a dropped field is a quiet downgrade of what the file said.
pub fn parse_legacy<T: DeserializeOwned>(value: &Value) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|error| {
        if error.to_string().contains("unknown field") {
            "legacy_upcast_non_migratable_field".to_owned()
        } else {
            "legacy_upcast_payload_malformed".to_owned()
        }
    })
}

/// The pre-BQ-01 usage fold, as it was written to disk.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyCostLedger {
    pub records: Vec<crate::UsageRecord>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// The trap. A bare `u64` where the current fold has `Option<u64>`, so an unset cost and a
    /// measured zero cost are the same bytes.
    pub cost_micros: u64,
}

impl LegacyCostLedger {
    fn fields(&self) -> Result<Vec<LegacyUpcastField>, String> {
        if self.records.len() > MAX_LEGACY_UPCAST_FIELDS {
            return Err("legacy_upcast_records_exceeded".to_owned());
        }
        // The legacy ledger's own token totals are what gets translated. The per-record values are
        // deliberately not re-summed: a record whose tokens are absent is a hole, and re-summing
        // over a hole would hand back a smaller and more confident number than the file contains.
        // `CostLedger::from_records` already collapses such a fold to `None` for the same reason.
        Ok(vec![
            cost_field("cost_micros", self.cost_micros)?,
            usage_field("input_tokens", self.input_tokens)?,
            usage_field("output_tokens", self.output_tokens)?,
        ])
    }
}

/// The pre-BQ-07 quota, as it was written to disk.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyQuota {
    pub scope: String,
    pub model_calls: u64,
    pub tokens: u64,
    pub concurrency: u32,
}

impl LegacyQuota {
    fn fields(&self) -> Result<Vec<LegacyUpcastField>, String> {
        safe_text(&self.scope, "legacy_upcast_quota_scope")?;
        // The `used_*` fields the current `QuotaWindowBudget` requires do not exist in an old
        // quota. They stay absent rather than becoming `0`: a zero there would report "nothing
        // was consumed" instead of "nothing was ever measured".
        Ok(vec![
            limit_field("model_calls", self.model_calls)?,
            limit_field("tokens", self.tokens)?,
            limit_field("concurrency", u64::from(self.concurrency))?,
            absent_usage("used_requests", "legacy_quota_has_no_consumption_counter")?,
            absent_usage("used_tokens", "legacy_quota_has_no_consumption_counter")?,
            absent_usage(
                "active_concurrency",
                "legacy_quota_has_no_consumption_counter",
            )?,
        ])
    }
}

/// The pre-BQ-20 harness script, as it was written to disk.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyCassette {
    pub outputs: Vec<LegacyCassetteOutput>,
}

/// One scripted model output. `usage` is optional and token-only: a cassette has never carried a
/// cost, a rate card or a provider receipt, and adding one would make an offline fixture look
/// like a billable measurement.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyCassetteOutput {
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<LegacyCassetteUsage>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyCassetteUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

impl LegacyCassette {
    fn validate(&self) -> Result<(), String> {
        if self.outputs.is_empty() || self.outputs.len() > MAX_LEGACY_CASSETTE_OUTPUTS {
            return Err("legacy_upcast_cassette_outputs_invalid".to_owned());
        }
        for output in &self.outputs {
            if output.text.len() > MAX_LEGACY_CASSETTE_TEXT {
                return Err("legacy_upcast_cassette_text_invalid".to_owned());
            }
            // A cassette body is model-visible text, so it is scanned the way a transcript is.
            scan_secret_sentinels(SecretScanChannel::Transcript, &output.text)
                .map_err(|_| "legacy_upcast_cassette_text_secret_detected".to_owned())?;
            for value in [output.model_id.as_deref(), output.stop_reason.as_deref()]
                .into_iter()
                .flatten()
            {
                safe_text(value, "legacy_upcast_cassette_metadata")?;
            }
        }
        Ok(())
    }

    fn fields(&self) -> Result<Vec<LegacyUpcastField>, String> {
        self.validate()?;
        let input = self
            .outputs
            .iter()
            .map(|output| output.usage.as_ref().and_then(|usage| usage.input_tokens))
            .collect::<Vec<_>>();
        let output_tokens = self
            .outputs
            .iter()
            .map(|output| output.usage.as_ref().and_then(|usage| usage.output_tokens))
            .collect::<Vec<_>>();
        Ok(vec![
            fold_measure("input_tokens", &input)?,
            fold_measure("output_tokens", &output_tokens)?,
            // A cassette has no cost field at all, which is the same hole a legacy `0` was.
            cost_field("cost_micros", 0)?,
        ])
    }
}

/// The pre-BQ-04 provider configuration, as it was written to disk.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyProviderConfig {
    pub selection_mode: String,
    pub profiles: Vec<LegacyConfigProfile>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyConfigProfile {
    pub profile: String,
    pub provider_id: String,
    pub protocol: String,
    /// A reference, never key material. Kept optional because an old config could carry none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<String>,
}

impl LegacyProviderConfig {
    /// The current selection mode this legacy file maps onto, or the typed refusal when the value
    /// is something this module does not know. A config that cannot be read is not defaulted.
    pub fn selection_mode(&self) -> Result<ProviderSelectionMode, String> {
        match self.selection_mode.as_str() {
            "live" => Ok(ProviderSelectionMode::Live),
            "cassette" => Ok(ProviderSelectionMode::Cassette),
            _ => Err("legacy_upcast_selection_mode_unknown".to_owned()),
        }
    }

    pub fn config_source(&self) -> ProviderConfigSource {
        ProviderConfigSource::LegacyCassette
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.profiles.is_empty() || self.profiles.len() > MAX_LEGACY_PROFILES {
            return Err("legacy_upcast_profiles_invalid".to_owned());
        }
        self.selection_mode()?;
        let mut names = BTreeSet::new();
        for profile in &self.profiles {
            for value in [
                profile.profile.as_str(),
                profile.provider_id.as_str(),
                profile.protocol.as_str(),
            ] {
                safe_text(value, "legacy_upcast_profile")?;
            }
            parse_protocol_wire_name(&profile.protocol)
                .ok_or_else(|| "legacy_upcast_protocol_unknown".to_owned())?;
            if let Some(reference) = &profile.credential_ref {
                safe_text(reference, "legacy_upcast_credential_ref")?;
            }
            if !names.insert(profile.profile.as_str()) {
                return Err("legacy_upcast_profile_duplicate".to_owned());
            }
        }
        Ok(())
    }

    fn fields(&self) -> Result<Vec<LegacyUpcastField>, String> {
        self.validate()?;
        // An old profile has no version. The `1` the upcasted snapshot carries was assigned here,
        // so it is recorded as assigned rather than reported: a version nobody ever wrote down
        // must not be presented as one that was.
        Ok(vec![LegacyUpcastField::new(
            "profile_version",
            LegacyFieldClass::Limit,
            LegacyMeasureOrigin::Absent,
            None,
            Some(BillingUnknownReason::Absent),
            "legacy_provider_version_assigned_at_upcast",
        )?])
    }
}

/// The inputs to one upcast attempt. The payload bytes themselves are never carried: only the
/// schema name they declare and a digest of them.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyUpcastRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub kind: LegacyPayloadKind,
    /// The schema name the legacy payload itself declares, read before it is parsed.
    pub payload_schema: String,
    /// Digest of the bytes this run read, before any upcast.
    pub payload_digest: String,
    /// The registry the caller believes it is upcasting against.
    pub registry_digest: String,
    /// An already-verified backup reference, when the caller has one.
    pub verified_backup_ref: Option<String>,
    pub request_digest: String,
}

impl LegacyUpcastRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: LegacyPayloadKind,
        payload_schema: impl Into<String>,
        payload_digest: impl Into<String>,
        verified_backup_ref: Option<String>,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: LEGACY_UPCAST_REQUEST_SCHEMA.to_owned(),
            version: LEGACY_UPCAST_VERSION,
            kind,
            payload_schema: payload_schema.into(),
            payload_digest: payload_digest.into(),
            registry_digest: legacy_upcast_registry_digest(),
            verified_backup_ref,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LEGACY_UPCAST_REQUEST_SCHEMA
            || self.version != LEGACY_UPCAST_VERSION
            || !LegacyPayloadKind::ALL.contains(&self.kind)
        {
            return Err("legacy_upcast_request_header_invalid".to_owned());
        }
        safe_text(&self.payload_schema, "legacy_upcast_payload_schema")?;
        valid_digest(&self.payload_digest, "legacy_upcast_payload_digest")?;
        valid_digest(&self.registry_digest, "legacy_upcast_registry_digest")?;
        if let Some(reference) = &self.verified_backup_ref {
            safe_text(reference, "legacy_upcast_backup_ref")?;
        }
        valid_digest(&self.request_digest, "legacy_upcast_request_digest")?;
        if self.request_digest != self.digest() {
            return Err("legacy_upcast_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "kind": self.kind,
            "payload_schema": self.payload_schema,
            "payload_digest": self.payload_digest,
            "registry_digest": self.registry_digest,
            "verified_backup_ref": self.verified_backup_ref,
        }))
    }
}

/// What one upcast attempt did.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyUpcastStatus {
    /// A legacy payload was translated into the current shape.
    Upcast,
    /// The material was already in the current shape; nothing was translated again.
    AlreadyCurrent,
}

impl LegacyUpcastStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Upcast => "upcast",
            Self::AlreadyCurrent => "already_current",
        }
    }
}

/// What one upcast attempt produced, and what it deliberately did not claim.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyUpcastReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub status: LegacyUpcastStatus,
    pub kind: LegacyPayloadKind,
    pub source_schema: String,
    pub destination_schema: String,
    pub fields: Vec<LegacyUpcastField>,
    pub totals: LegacyUpcastTotals,
    pub rollback: LegacyUpcastRollback,
    /// Stable code for the attempt. Empty is impossible: every outcome names itself.
    pub reason: String,
    pub request_digest: String,
    pub report_digest: String,
}

impl LegacyUpcastReport {
    /// Upcast a pre-BQ-01 usage fold. The legacy `cost_micros` becomes an unknown cost whether it
    /// is `0` or a positive number: a zero was never a measurement, and a positive number with no
    /// rate card and no provider receipt behind it is not one either.
    pub fn evaluate_cost_ledger(
        request: &LegacyUpcastRequest,
        legacy: &LegacyCostLedger,
    ) -> Result<Self, String> {
        let admission = admit(request)?;
        require_upcast(admission)?;
        Self::assemble(request, admission, legacy.fields()?)
    }

    /// Upcast a pre-BQ-07 quota. Limits travel; the consumption counters the current budget
    /// requires stay absent, because an old quota never measured them.
    pub fn evaluate_quota(
        request: &LegacyUpcastRequest,
        legacy: &LegacyQuota,
    ) -> Result<Self, String> {
        let admission = admit(request)?;
        require_upcast(admission)?;
        Self::assemble(request, admission, legacy.fields()?)
    }

    /// Upcast a pre-BQ-20 cassette script. Token counts fold only when every output reported one;
    /// the cost stays unknown because a cassette has none.
    pub fn evaluate_cassette(
        request: &LegacyUpcastRequest,
        legacy: &LegacyCassette,
    ) -> Result<Self, String> {
        let admission = admit(request)?;
        require_upcast(admission)?;
        Self::assemble(request, admission, legacy.fields()?)
    }

    /// Upcast a pre-BQ-04 provider configuration. No number is manufactured: the only field is
    /// the profile version the legacy file never carried.
    pub fn evaluate_provider_config(
        request: &LegacyUpcastRequest,
        legacy: &LegacyProviderConfig,
    ) -> Result<Self, String> {
        let admission = admit(request)?;
        require_upcast(admission)?;
        Self::assemble(request, admission, legacy.fields()?)
    }

    /// Re-running the migration over material that was already upcast. The old bytes are not read
    /// and not re-translated, so there is nothing to overwrite: the caller re-validates the report
    /// the first run produced through [`LegacyUpcastHistory::append`].
    pub fn already_current(request: &LegacyUpcastRequest) -> Result<Self, String> {
        let admission = admit(request)?;
        if admission.status != LegacyUpcastStatus::AlreadyCurrent {
            return Err("legacy_upcast_not_already_current".to_owned());
        }
        Self::assemble(request, admission, Vec::new())
    }

    fn assemble(
        request: &LegacyUpcastRequest,
        admission: LegacyAdmission,
        fields: Vec<LegacyUpcastField>,
    ) -> Result<Self, String> {
        if fields.len() > MAX_LEGACY_UPCAST_FIELDS {
            return Err("legacy_upcast_fields_exceeded".to_owned());
        }
        let mut names = BTreeSet::new();
        for field in &fields {
            field.validate()?;
            if !names.insert(field.field.as_str()) {
                return Err("legacy_upcast_field_duplicate".to_owned());
            }
        }
        let totals = LegacyUpcastTotals::from_fields(&fields)?;
        let rollback = LegacyUpcastRollback::new(
            request.verified_backup_ref.clone(),
            request.payload_digest.clone(),
        )?;
        let mut report = Self {
            schema: LEGACY_UPCAST_REPORT_SCHEMA.to_owned(),
            version: LEGACY_UPCAST_VERSION,
            status: admission.status,
            kind: request.kind,
            source_schema: request.payload_schema.clone(),
            destination_schema: legacy_upcast_destination_schema(request.kind).to_owned(),
            fields,
            totals,
            rollback,
            reason: admission.reason.to_owned(),
            request_digest: request.request_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(request)?;
        Ok(report)
    }

    /// Re-derive the whole decision. A report that was edited after the fact — a status, a
    /// reason, a promoted value, a field list, a digest — is rejected here.
    pub fn validate_against(&self, request: &LegacyUpcastRequest) -> Result<(), String> {
        request.validate()?;
        let admission = admit(request)?;
        if self.schema != LEGACY_UPCAST_REPORT_SCHEMA
            || self.version != LEGACY_UPCAST_VERSION
            || self.status != admission.status
            || self.kind != request.kind
            || self.source_schema != request.payload_schema
            || self.destination_schema != legacy_upcast_destination_schema(request.kind)
            || self.reason != admission.reason
            || self.request_digest != request.request_digest
            || self.fields.len() > MAX_LEGACY_UPCAST_FIELDS
        {
            return Err("legacy_upcast_report_binding_invalid".to_owned());
        }
        let mut names = BTreeSet::new();
        for field in &self.fields {
            field.validate()?;
            if !names.insert(field.field.as_str()) {
                return Err("legacy_upcast_field_duplicate".to_owned());
            }
        }
        // An already-current run translated nothing, so it must not have invented a field either.
        if self.status == LegacyUpcastStatus::AlreadyCurrent && !self.fields.is_empty() {
            return Err("legacy_upcast_already_current_carries_fields".to_owned());
        }
        self.totals.validate_against(&self.fields)?;
        self.rollback.validate()?;
        if !self.rollback.read_only
            || self.rollback.legacy_source_digest != request.payload_digest
            || self.rollback.verified_backup_ref != request.verified_backup_ref
        {
            return Err("legacy_upcast_rollback_binding_invalid".to_owned());
        }
        valid_digest(&self.report_digest, "legacy_upcast_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("legacy_upcast_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// The value the upcast handed on for one field, if any. An absent measurement has no value to
    /// hand on, which is the whole point.
    pub fn promoted(&self, field: &str) -> Option<u64> {
        self.fields
            .iter()
            .find(|entry| entry.field == field)
            .and_then(|entry| entry.promoted)
    }

    /// The reason one field was or was not promotable.
    pub fn reason_for(&self, field: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|entry| entry.field == field)
            .map(|entry| entry.reason.as_str())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "status": self.status,
            "kind": self.kind,
            "source_schema": self.source_schema,
            "destination_schema": self.destination_schema,
            "fields": self.fields,
            "totals": self.totals,
            "rollback": self.rollback,
            "reason": self.reason,
            "request_digest": self.request_digest,
        }))
    }
}

#[derive(Clone, Copy)]
struct LegacyAdmission {
    status: LegacyUpcastStatus,
    reason: &'static str,
}

/// The fixed decision order, so a request that breaks several rules always reports the same first
/// one: request shape, then registry drift, then already-current, then the upcaster name.
fn admit(request: &LegacyUpcastRequest) -> Result<LegacyAdmission, String> {
    request.validate()?;
    if request.registry_digest != legacy_upcast_registry_digest() {
        return Err("legacy_upcast_registry_drift".to_owned());
    }
    if request.payload_schema == legacy_upcast_destination_schema(request.kind) {
        return Ok(LegacyAdmission {
            status: LegacyUpcastStatus::AlreadyCurrent,
            reason: "legacy_payload_already_current",
        });
    }
    if request.payload_schema != legacy_upcast_source_schema(request.kind) {
        return Err("legacy_upcast_unknown_major".to_owned());
    }
    Ok(LegacyAdmission {
        status: LegacyUpcastStatus::Upcast,
        reason: "legacy_payload_upcast",
    })
}

fn require_upcast(admission: LegacyAdmission) -> Result<(), String> {
    if admission.status != LegacyUpcastStatus::Upcast {
        return Err("legacy_upcast_already_current_use_idempotent_path".to_owned());
    }
    Ok(())
}

/// A reported token count. A legacy `None` and a legacy `0` are both absent measurements, and only
/// a strictly positive reported value may become a number.
fn usage_field(name: &str, value: Option<u64>) -> Result<LegacyUpcastField, String> {
    match value {
        None => absent_usage(name, "legacy_usage_field_absent"),
        Some(0) => absent_usage(name, "legacy_usage_zero_is_absent_measurement"),
        Some(reported) => LegacyUpcastField::new(
            name,
            LegacyFieldClass::Usage,
            LegacyMeasureOrigin::Reported,
            Some(reported),
            None,
            "legacy_usage_reported",
        ),
    }
}

/// A usage field the legacy payload never carried.
fn absent_usage(name: &str, reason: &'static str) -> Result<LegacyUpcastField, String> {
    LegacyUpcastField::new(
        name,
        LegacyFieldClass::Usage,
        LegacyMeasureOrigin::Absent,
        None,
        Some(BillingUnknownReason::Absent),
        reason,
    )
}

/// A legacy limit. A zero limit is the same "never written" hole a zero cost is, so it does not
/// become a zero budget either; the caller has to supply a real limit.
fn limit_field(name: &str, value: u64) -> Result<LegacyUpcastField, String> {
    match value {
        0 => LegacyUpcastField::new(
            name,
            LegacyFieldClass::Limit,
            LegacyMeasureOrigin::LegacyUnsetZero,
            None,
            Some(BillingUnknownReason::Absent),
            "legacy_limit_zero_not_promoted",
        ),
        reported => LegacyUpcastField::new(
            name,
            LegacyFieldClass::Limit,
            LegacyMeasureOrigin::Reported,
            Some(reported),
            None,
            "legacy_limit_reported",
        ),
    }
}

/// A legacy cost. Never promotable, whether the legacy number was zero or positive: there is no
/// rate card and no provider receipt behind it, so it is reported as unknown and not summed.
fn cost_field(name: &str, value: u64) -> Result<LegacyUpcastField, String> {
    if value == 0 {
        LegacyUpcastField::new(
            name,
            LegacyFieldClass::Cost,
            LegacyMeasureOrigin::LegacyUnsetZero,
            None,
            Some(BillingUnknownReason::ProviderUnreported),
            "legacy_zero_is_absent_measurement",
        )
    } else {
        LegacyUpcastField::new(
            name,
            LegacyFieldClass::Cost,
            LegacyMeasureOrigin::Reported,
            None,
            Some(BillingUnknownReason::ProviderUnreported),
            "legacy_cost_lacks_rate_card_and_receipt",
        )
    }
}

/// Fold several legacy measurements of one field into the current-shape number. A single absent
/// contributor collapses the whole fold: adding over a hole would report a smaller and more
/// confident number than the legacy file ever contained.
fn fold_measure(name: &str, contributors: &[Option<u64>]) -> Result<LegacyUpcastField, String> {
    if contributors.is_empty() {
        return absent_usage(name, "legacy_usage_field_absent");
    }
    let mut total = 0u64;
    for contributor in contributors {
        let Some(value) = contributor else {
            return absent_usage(name, "legacy_usage_partial_report");
        };
        match *value {
            0 => return absent_usage(name, "legacy_usage_zero_is_absent_measurement"),
            reported => {
                total = total
                    .checked_add(reported)
                    .ok_or_else(|| "legacy_upcast_fold_overflow".to_owned())?;
            }
        }
    }
    LegacyUpcastField::new(
        name,
        LegacyFieldClass::Usage,
        LegacyMeasureOrigin::Reported,
        Some(total),
        None,
        "legacy_usage_reported",
    )
}

impl ModelProtocol {
    /// The wire spelling of this protocol on the current `ModelRoute`, which is what a legacy
    /// config named. A value that no longer names a protocol is refused rather than assumed to be
    /// the default: an old config with a retired protocol is not a config that meant the newest
    /// one.
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::AnthropicMessages => "anthropic_messages",
            Self::OpenAiChat => "openai_chat",
            Self::OpenAiResponses => "openai_responses",
            Self::OllamaChat => "ollama_chat",
            Self::GeminiInteractions => "gemini_interactions",
        }
    }
}

/// Every protocol the current domain knows, so a legacy config is checked against the same set
/// rather than a second list that could drift from it.
const CURRENT_MODEL_PROTOCOLS: [ModelProtocol; 6] = [
    ModelProtocol::Legacy,
    ModelProtocol::AnthropicMessages,
    ModelProtocol::OpenAiChat,
    ModelProtocol::OpenAiResponses,
    ModelProtocol::OllamaChat,
    ModelProtocol::GeminiInteractions,
];

fn parse_protocol_wire_name(value: &str) -> Option<ModelProtocol> {
    CURRENT_MODEL_PROTOCOLS
        .into_iter()
        .find(|protocol| protocol.wire_name() == value.trim())
}

/// One entry in the append-only migration history.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyUpcastRecord {
    pub schema: String,
    pub version: SchemaVersion,
    /// One-based and strictly increasing; a replay never consumes a new sequence number.
    pub sequence: u64,
    pub kind: LegacyPayloadKind,
    pub status: LegacyUpcastStatus,
    /// Digest of the bytes this run read. The key a replay is recognised by.
    pub source_digest: String,
    pub report_digest: String,
    pub record_digest: String,
}

impl LegacyUpcastRecord {
    /// Record one run. [`LegacyUpcastHistory::append`] is what derives the sequence number; this
    /// constructor is public so a caller can build a fixture and so a persisted record can be
    /// re-checked, and [`Self::validate`] is what refuses a sequence the history did not reach.
    pub fn new(sequence: u64, report: &LegacyUpcastReport) -> Result<Self, String> {
        let mut record = Self {
            schema: LEGACY_UPCAST_RECORD_SCHEMA.to_owned(),
            version: LEGACY_UPCAST_VERSION,
            sequence,
            kind: report.kind,
            status: report.status,
            source_digest: report.rollback.legacy_source_digest.clone(),
            report_digest: report.report_digest.clone(),
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LEGACY_UPCAST_RECORD_SCHEMA
            || self.version != LEGACY_UPCAST_VERSION
            || self.sequence == 0
            || !LegacyPayloadKind::ALL.contains(&self.kind)
        {
            return Err("legacy_upcast_history_record_invalid".to_owned());
        }
        valid_digest(&self.source_digest, "legacy_upcast_history_source_digest")?;
        valid_digest(&self.report_digest, "legacy_upcast_history_report_digest")?;
        valid_digest(&self.record_digest, "legacy_upcast_history_digest_invalid")?;
        if self.record_digest != self.digest() {
            return Err("legacy_upcast_history_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "sequence": self.sequence,
            "kind": self.kind,
            "status": self.status,
            "source_digest": self.source_digest,
            "report_digest": self.report_digest,
        }))
    }
}

/// What appending a report to the history did.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyUpcastOutcome {
    /// A new history entry was appended.
    Appended,
    /// This exact report was already recorded for this exact source. Nothing was written, so
    /// re-running the migration did not overwrite what it produced the first time.
    AlreadyRecorded,
    /// The same legacy source already produced a *different* report. Refused rather than
    /// rewritten.
    Conflict,
}

/// The append-only record of what every migration run did to which bytes.
///
/// This is a fold over reports, not a store: it holds no payload, opens no file and rewrites
/// nothing. Re-running a migration is idempotent here, which is what makes "migrations may be
/// repeated" safe to say.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyUpcastHistory {
    pub schema: String,
    pub records: Vec<LegacyUpcastRecord>,
}

impl LegacyUpcastHistory {
    pub fn new() -> Self {
        Self {
            schema: LEGACY_UPCAST_HISTORY_SCHEMA.to_owned(),
            records: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LEGACY_UPCAST_HISTORY_SCHEMA
            || self.records.len() > MAX_LEGACY_HISTORY_RECORDS
        {
            return Err("legacy_upcast_history_header_invalid".to_owned());
        }
        let mut sources = BTreeSet::new();
        for (index, record) in self.records.iter().enumerate() {
            record.validate()?;
            if record.sequence != (index as u64) + 1 {
                return Err("legacy_upcast_history_sequence_invalid".to_owned());
            }
            if !sources.insert((record.kind, record.source_digest.as_str())) {
                return Err("legacy_upcast_history_source_duplicate".to_owned());
            }
        }
        Ok(())
    }

    /// Fold one run into the history. A replay of the same report for the same source is a no-op,
    /// and a second different report for that source is a refusal, never an overwrite.
    pub fn append(&mut self, report: &LegacyUpcastReport) -> Result<LegacyUpcastOutcome, String> {
        self.validate()?;
        let source = report.rollback.legacy_source_digest.clone();
        if let Some(existing) = self
            .records
            .iter()
            .find(|record| record.kind == report.kind && record.source_digest == source)
        {
            return if existing.report_digest == report.report_digest {
                Ok(LegacyUpcastOutcome::AlreadyRecorded)
            } else {
                Ok(LegacyUpcastOutcome::Conflict)
            };
        }
        if self.records.len() >= MAX_LEGACY_HISTORY_RECORDS {
            return Err("legacy_upcast_history_exceeded".to_owned());
        }
        let record = LegacyUpcastRecord::new(self.records.len() as u64 + 1, report)?;
        self.records.push(record);
        self.validate()?;
        Ok(LegacyUpcastOutcome::Appended)
    }

    /// Whether these bytes have already been through a migration, and what came out of it.
    pub fn recorded(
        &self,
        kind: LegacyPayloadKind,
        source_digest: &str,
    ) -> Option<&LegacyUpcastRecord> {
        self.records
            .iter()
            .find(|record| record.kind == kind && record.source_digest == source_digest)
    }
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_LEGACY_TEXT
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    // Migration notes are written back into local storage, so they are scanned as a cache value.
    scan_secret_sentinels(SecretScanChannel::Cache, value)
        .map_err(|_| format!("{field}_secret_detected"))
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(format!("{field}_invalid"))
    }
}

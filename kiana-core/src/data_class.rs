//! BQ-25 server-side DataClass derivation and the five-sink admission matrix.
//!
//! The domain contract in `kiana-domain/src/telemetry_separation.rs` already answers "is this value
//! safe for this channel?". It does so for a class the **caller** supplies: `TelemetryChannelCandidate`
//! carries `data_class` and `export_class_ceiling` as caller-set fields. That is the gap this module
//! closes. BQ-25 requires the class conclusion to be *server-derived*, and a caller that may state
//! its own class can state a convenient one. So the ladder is walked here, from the value's own
//! shape and content, and a field that arrives with a class attached is refused outright rather
//! than believed.
//!
//! Three further things live here because they need `kiana-core` and not `kiana-domain`:
//!
//! * **The runtime metric guard.** `MetricCardinalityGuard` is defined in `kiana-core/src/metrics.rs`
//!   with its own forbidden-label list. The domain baseline records, as a limitation, that "the two
//!   lists are not one list and are not checked against each other. A label BQ-25 admits and the
//!   runtime guard rejects is possible." This module actually *runs* that guard, so a source-level
//!   admission and the guard that enforces it at runtime are the same decision.
//! * **The five-sink matrix.** The domain answers per candidate per channel. The card asks for the
//!   whole table at once, because "may this field be a metric label" is only meaningful next to
//!   "may this same field be an event field".
//! * **Server-side derivation of the class**, with a basis recorded so a reader can tell a
//!   `Restricted`-because-it-is-a-prompt from a `Restricted`-because-it-is-a-secret.
//!
//! Nothing here is a second classification vocabulary. [`DataClass`] and [`TelemetryChannel`] are
//! re-exported from the domain contract, the per-sink ceilings and floors are read from
//! [`TelemetryChannelPolicy`], the payload and identifier key tests are [`is_payload_key`] and
//! [`is_high_cardinality_label`], the content test is `redact_text` plus `scan_secret_sentinels`
//! under [`TelemetryChannel::secret_scan_channel`], and the label budget is
//! [`MAX_METRIC_LABEL_VALUES`]. A refusal that the domain contract already names is carried as
//! [`SinkAdmissionRefusal::Shared`] and its reason string is produced by [`TelemetryRefusal`], not
//! restated in this file -- which is what the source guard asserts.
//!
//! This module is a **read-only contract over supplied facts**. It appends no event, stores no
//! state, opens no file, starts no task and contacts no sink. It decides whether a *proposed* field
//! is admissible in a named sink, and [`SinkAdmissionReport::validate_against`] re-derives that
//! decision so an edited outcome cannot be published as if it had been decided here. The
//! ControlPlane remains the only place a command becomes an effect.

use kiana_domain::{
    is_high_cardinality_label, is_payload_key, json_digest, redact_text, scan_secret_sentinels,
    DataClass, EventId, MetricCatalog, MetricPoint, RequestId, SchemaVersion, SecretScanChannel,
    TelemetryChannel, TelemetryChannelGuarantee,
    TelemetryChannelPolicy, TelemetryRefusal, MAX_METRIC_LABEL_VALUES, MAX_METRIC_SERIES,
};
use serde::{Deserialize, Serialize};

use crate::metrics::{MetricCardinalityError, MetricCardinalityGuard};

pub const DATA_CLASS_DERIVATION_SCHEMA: &str = "kiana.data-class-derivation.v1";
pub const SINK_ADMISSION_REPORT_SCHEMA: &str = "kiana.sink-admission-report.v1";
pub const DATA_CLASS_ADMISSION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_SINK_FIELD_KEY: usize = 128;
pub const MAX_SINK_FIELD_VALUE: usize = 256;
pub const MAX_SINK_ADMISSION_FIELDS: usize = 64;

/// The three sinks whose contract is *observation* rather than *record*.
///
/// `Event` and `Receipt` are deliberately absent. An event is a committed fact and a receipt is a
/// business artefact; neither is a substitute for the other three, and neither is telemetry. The
/// separation enforced in `crate::telemetry_separation` is the one among log, metric and trace,
/// which is where the failure actually lives: a value that is fine in a log line is routinely fatal
/// in a metric label.
pub const OBSERVATION_SINKS: [TelemetryChannel; 3] = [
    TelemetryChannel::Log,
    TelemetryChannel::Metric,
    TelemetryChannel::Trace,
];

/// Where a field's class claim came from.
///
/// The variant that exists to be refused is [`CallerSupplied`](Self::CallerSupplied). BQ-25 requires
/// the class conclusion to be derived by the server, so a field that arrives already classified is
/// not re-classified and downgraded -- it is refused, because a caller able to assert a class is a
/// caller able to assert `Public` for a prompt.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassClaimOrigin {
    /// The server derived the class from the value's own shape and content.
    ServerDerived,
    /// The caller asserted the class. Always refused.
    CallerSupplied,
}

impl ClassClaimOrigin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ServerDerived => "server_derived",
            Self::CallerSupplied => "caller_supplied",
        }
    }
}

/// Why the server derived the class it derived.
///
/// Ordered from most to least severe so the first match is the one worth reporting. A prompt is
/// `Restricted` because it is a prompt; a secret is `Restricted` because redaction would still
/// rewrite it. Those are different incidents and an operator reading a metric needs to tell them
/// apart.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassBasis {
    /// `redact_text` still rewrites the value, or a secret sentinel fired under the sink's scan
    /// channel. The value is not safe anywhere, and the sink is irrelevant to that finding.
    SecretContent,
    /// The key names a payload -- a prompt, a raw response, a command, an argument list. No
    /// redaction makes a prompt stop being a prompt.
    PayloadShape,
    /// The value is an opaque digest or handle. The one shape admissible in every sink.
    OpaqueReference,
    /// An identifier or path in a label position: unbounded dimension growth wearing a label.
    HighCardinalityIdentifier,
    /// A bounded, low-cardinality dimension.
    BoundedLabel,
    /// Bounded plain text that is none of the above.
    BoundedText,
}

impl ClassBasis {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SecretContent => "secret_content",
            Self::PayloadShape => "payload_shape",
            Self::OpaqueReference => "opaque_reference",
            Self::HighCardinalityIdentifier => "high_cardinality_identifier",
            Self::BoundedLabel => "bounded_label",
            Self::BoundedText => "bounded_text",
        }
    }
}

/// The class the server derived, and why.
///
/// `class` is the answer; `basis` is the audit trail. A report that states a class without a basis
/// is refused, because "this is `Restricted`" and "this is `Restricted` because it is an API key"
/// are different statements and only the second one is actionable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedClass {
    pub schema: String,
    pub version: SchemaVersion,
    pub key: String,
    pub class: DataClass,
    pub basis: ClassBasis,
    /// Digest of the key and the *redacted* value. The raw value never enters a digest input that
    /// could be replayed, and the digest is what lets a refusal be quoted in an incident without
    /// restating the secret.
    pub value_digest: String,
}

impl DerivedClass {
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "key": self.key,
            "class": self.class,
            "basis": self.basis,
        }))
    }
}

/// A refusal the runtime metric guard raised, in the guard's own vocabulary.
///
/// The guard lives in `kiana-core/src/metrics.rs` and is the thing that actually enforces label
/// cardinality at runtime. Naming its reasons here means an operator sees *which* mechanism
/// refused, rather than a reworded "metric label refused" that hides which of the four rules fired.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum TelemetryRuntimeRefusal {
    /// The guard's forbidden-label list matched. Core-only: the domain contract has no equivalent.
    LabelForbidden(String),
    /// The label already holds as many distinct values as the guard allows.
    LabelValueLimit(String),
    /// The label value is empty, oversized or secret-shaped.
    LabelValueInvalid(String),
    /// The series table is full.
    SeriesLimit,
    /// The point did not validate against the catalog.
    PointInvalid,
}

impl TelemetryRuntimeRefusal {
    pub fn from_guard(error: &MetricCardinalityError) -> Self {
        match error {
            MetricCardinalityError::ForbiddenLabel(label) => Self::LabelForbidden(label.clone()),
            MetricCardinalityError::LabelValueLimit(label) => Self::LabelValueLimit(label.clone()),
            MetricCardinalityError::LabelValueInvalid(label) => {
                Self::LabelValueInvalid(label.clone())
            }
            MetricCardinalityError::SeriesLimit => Self::SeriesLimit,
            MetricCardinalityError::PointInvalid(_) => Self::PointInvalid,
        }
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::LabelForbidden(_) => "telemetry_sink_runtime_label_forbidden",
            Self::LabelValueLimit(_) => "telemetry_sink_runtime_label_value_limit",
            Self::LabelValueInvalid(_) => "telemetry_sink_runtime_label_value_invalid",
            Self::SeriesLimit => "telemetry_sink_runtime_series_limit",
            Self::PointInvalid => "telemetry_sink_runtime_point_invalid",
        }
    }
}

/// Why one sink refused one field.
///
/// [`Shared`](Self::Shared) carries a [`TelemetryRefusal`] and borrows its reason string verbatim.
/// The variants below it are the ones `kiana-core` can reach and `kiana-domain` cannot, and each is
/// prefixed `telemetry_sink_` so a code quoted from this module is never ambiguous with the same
/// finding quoted from the domain contract.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum SinkAdmissionRefusal {
    /// A refusal the domain contract already names, carried without rewording.
    Shared(TelemetryRefusal),
    /// The field arrived with a caller-asserted class. BQ-25 requires the class to be server-derived.
    CallerSuppliedClass,
    /// The metric sink was asked for a decision with no catalog to enforce it against.
    MetricCatalogAbsent,
    /// The live `MetricCardinalityGuard` refused the point.
    Runtime(TelemetryRuntimeRefusal),
}

impl SinkAdmissionRefusal {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Shared(refusal) => refusal.as_str(),
            Self::CallerSuppliedClass => "telemetry_sink_caller_supplied_class",
            Self::MetricCatalogAbsent => "telemetry_sink_metric_catalog_absent",
            Self::Runtime(refusal) => refusal.as_str(),
        }
    }

    /// Whether the refusal is a positive detection, rather than an absence of proof.
    ///
    /// Ordering uses this: a shape or content finding is reported ahead of a weaker
    /// absence-of-proof finding, because "this label is a run id" is the actionable statement and
    /// "we could not establish the guarantee" would bury it. This mirrors the order the domain
    /// contract already uses for the same reason.
    pub const fn is_positive_detection(&self) -> bool {
        match self {
            Self::Shared(refusal) => matches!(
                refusal,
                TelemetryRefusal::UnredactedContent
                    | TelemetryRefusal::DataClassTooHigh
                    | TelemetryRefusal::ExportClassCeilingTooHigh
                    | TelemetryRefusal::HighCardinalityLabel
                    | TelemetryRefusal::PayloadShapeForbidden
                    | TelemetryRefusal::LabelCardinalityExceeded
            ),
            Self::CallerSuppliedClass | Self::MetricCatalogAbsent | Self::Runtime(_) => true,
        }
    }
}

/// One field a producer wants to place in one or more sinks.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SinkField {
    pub key: String,
    pub value: String,
    /// A class the caller asserted. `Some` is refused: the class is the server's to derive.
    pub asserted_class: Option<DataClass>,
    /// The `DataClass` ceiling the caller asserts applies to the receiving systems. This one *is*
    /// read from the caller, because it is a fact about the destination rather than a claim about
    /// the value, and the domain contract already treats it the same way.
    pub export_class_ceiling: DataClass,
    /// True when the value is an opaque digest or handle rather than content.
    pub is_reference: bool,
    /// True when the value occupies a label (dimension) position.
    pub is_label: bool,
    /// Distinct values observed for this label, for the cardinality rule.
    pub observed_label_values: u32,
}

impl SinkField {
    pub fn reference(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            asserted_class: None,
            export_class_ceiling: DataClass::Public,
            is_reference: true,
            is_label: false,
            observed_label_values: 0,
        }
    }

    pub fn label(key: impl Into<String>, value: impl Into<String>, observed_values: u32) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            asserted_class: None,
            export_class_ceiling: DataClass::Public,
            is_reference: false,
            is_label: true,
            observed_label_values: observed_values,
        }
    }

    pub fn text(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            asserted_class: None,
            export_class_ceiling: DataClass::Public,
            is_reference: false,
            is_label: false,
            observed_label_values: 0,
        }
    }

    /// Attach a caller-asserted class. The only way to produce one, and the report refuses it.
    pub fn with_asserted_class(mut self, class: DataClass) -> Self {
        self.asserted_class = Some(class);
        self
    }

    /// Set the declared export ceiling. A fact about the destination, not a claim about the value.
    pub fn with_export_ceiling(mut self, ceiling: DataClass) -> Self {
        self.export_class_ceiling = ceiling;
        self
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.key.trim().is_empty() || self.key.len() > MAX_SINK_FIELD_KEY {
            return Err("sink_field_key_invalid".to_owned());
        }
        if self.value.is_empty() || self.value.len() > MAX_SINK_FIELD_VALUE {
            return Err("sink_field_value_invalid".to_owned());
        }
        if self.is_reference && self.is_label {
            return Err("sink_field_shape_invalid".to_owned());
        }
        Ok(())
    }
}

/// One cell of the matrix: a field, in a sink, with one outcome.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SinkAdmission {
    pub schema: String,
    pub version: SchemaVersion,
    pub key: String,
    pub sink: TelemetryChannel,
    pub admitted: bool,
    pub derived_class: DataClass,
    pub basis: ClassBasis,
    /// Empty when admitted. A stable reason code when refused.
    pub reason: String,
    pub value_digest: String,
}

impl SinkAdmission {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SINK_ADMISSION_REPORT_SCHEMA
            || !self
                .version
                .is_compatible_with(&DATA_CLASS_ADMISSION_VERSION)
            || self.key.trim().is_empty()
            || self.value_digest.is_empty()
        {
            return Err("sink_admission_invalid".to_owned());
        }
        // An admitted cell may not carry a reason and a refused cell may not lack one, so a reader
        // of the status line cannot skip the list.
        if self.admitted == !self.reason.is_empty() {
            return Err("sink_admission_reason_mismatch".to_owned());
        }
        Ok(())
    }
}

/// What each sink's adapter reported about its own redaction.
///
/// `kiana-domain` models this as a map carried inside the safety request. It is split out here so
/// the core report can name a *missing* entry as its own failure: an adapter that never reported is
/// a refusal, not a default.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetryGuarantees {
    pub guarantees: Vec<(TelemetryChannel, TelemetryChannelGuarantee)>,
}

impl TelemetryGuarantees {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn established(mut self, channel: TelemetryChannel) -> Self {
        self.set(channel, TelemetryChannelGuarantee::RedactedAtSink);
        self
    }

    pub fn set(&mut self, channel: TelemetryChannel, guarantee: TelemetryChannelGuarantee) {
        self.guarantees.retain(|(seen, _)| *seen != channel);
        self.guarantees.push((channel, guarantee));
        self.guarantees.sort_by_key(|(channel, _)| channel.rank());
    }

    pub fn get(&self, channel: TelemetryChannel) -> Option<TelemetryChannelGuarantee> {
        self.guarantees
            .iter()
            .find(|(seen, _)| *seen == channel)
            .map(|(_, guarantee)| *guarantee)
    }
}

/// The whole table: every field against every sink it was proposed for.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SinkAdmissionReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub request_id: RequestId,
    /// One cell per field against every sink, in field order then `TelemetryChannel::ALL` order. A
    /// field is never silently dropped from a sink it did not get a decision in.
    pub admissions: Vec<SinkAdmission>,
    /// Keys refused in at least one sink, in field order.
    pub refused_keys: Vec<String>,
    /// The leading refusal, suffixed with the offending key and sink. Empty when nothing refused.
    pub reason: String,
    /// The metric catalog the runtime guard was checked against, when one was supplied.
    pub catalog_digest: String,
    pub report_digest: String,
}

impl SinkAdmissionReport {
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "request_id": self.request_id,
            "admissions": self.admissions,
            "refused_keys": self.refused_keys,
            "catalog_digest": self.catalog_digest,
        }))
    }

    /// Re-derive every cell and refuse a report that disagrees with the derivation.
    ///
    /// The pairing is the same one PD-26 revocation and DEP-20 quiesce use: the report cannot
    /// publish itself. Without this, a caller could hand back an `admitted` cell for a field the
    /// server would have refused, and the digest would still verify, because the digest only proves
    /// the report is internally consistent.
    pub fn validate_against(
        &self,
        request_id: RequestId,
        fields: &[SinkField],
        guarantees: &TelemetryGuarantees,
        catalog: Option<&MetricCatalog>,
    ) -> Result<(), String> {
        if self.schema != SINK_ADMISSION_REPORT_SCHEMA
            || !self
                .version
                .is_compatible_with(&DATA_CLASS_ADMISSION_VERSION)
            || self.request_id != request_id
        {
            return Err("sink_admission_report_header_invalid".to_owned());
        }
        if self.report_digest != self.digest() {
            return Err("sink_admission_report_digest_mismatch".to_owned());
        }
        if fields.len() > MAX_SINK_ADMISSION_FIELDS {
            return Err("sink_admission_field_count_exceeded".to_owned());
        }
        let expected = derive_admissions(fields, guarantees, catalog)?;
        if self.admissions != expected {
            return Err("sink_admission_report_not_derivable".to_owned());
        }
        let mut refused: Vec<String> = Vec::new();
        for cell in expected.iter().filter(|cell| !cell.admitted) {
            if !refused.contains(&cell.key) {
                refused.push(cell.key.clone());
            }
        }
        if self.refused_keys != refused {
            return Err("sink_admission_refused_keys_mismatch".to_owned());
        }
        let leading = expected
            .iter()
            .find(|cell| !cell.admitted)
            .map(|cell| format!("{}:{}:{}", cell.reason, cell.key, cell.sink.as_str()))
            .unwrap_or_default();
        if self.reason != leading {
            return Err("sink_admission_reason_mismatch".to_owned());
        }
        let expected_catalog = catalog.map(|catalog| catalog.catalog_digest.clone()).unwrap_or_default();
        if self.catalog_digest != expected_catalog {
            return Err("sink_admission_catalog_digest_mismatch".to_owned());
        }
        Ok(())
    }
}

/// Derive the class of one field, server-side.
///
/// The order is the contract. `SecretContent` is checked first because it is the finding that holds
/// in every sink, and a field that is both a prompt and an API key is an incident rather than a
/// classification. `PayloadShape` follows for the same reason: no redaction makes a prompt safe, so
/// re-deriving the class of a redacted prompt would be a category error.
pub fn derive_sink_class(field: &SinkField, sink: TelemetryChannel) -> DerivedClass {
    let scan_channel: SecretScanChannel = sink.secret_scan_channel();
    let (class, basis) = if redact_text(&field.value) != field.value
        || scan_secret_sentinels(scan_channel, &field.value).is_err()
    {
        (DataClass::Restricted, ClassBasis::SecretContent)
    } else if is_payload_key(&field.key) {
        (DataClass::Restricted, ClassBasis::PayloadShape)
    } else if field.is_reference {
        (DataClass::Internal, ClassBasis::OpaqueReference)
    } else if field.is_label && is_high_cardinality_label(&field.key) {
        (DataClass::Confidential, ClassBasis::HighCardinalityIdentifier)
    } else if field.is_label {
        (DataClass::Public, ClassBasis::BoundedLabel)
    } else {
        (DataClass::Internal, ClassBasis::BoundedText)
    };
    DerivedClass {
        schema: DATA_CLASS_DERIVATION_SCHEMA.to_owned(),
        version: DATA_CLASS_ADMISSION_VERSION,
        key: field.key.clone(),
        class,
        basis,
        value_digest: json_digest(&serde_json::json!({
            "key": field.key,
            "value": redact_text(&field.value),
        })),
    }
}

/// Run the live `MetricCardinalityGuard` against one point.
///
/// This is the wiring the domain baseline records as missing. The guard's forbidden-label list and
/// the domain contract's identifier rule are two lists; running the guard means a label one of them
/// refuses cannot be admitted by the other.
pub fn runtime_metric_refusal(
    catalog: &MetricCatalog,
    point: &MetricPoint,
) -> Option<TelemetryRuntimeRefusal> {
    let mut guard = MetricCardinalityGuard::new(catalog.clone()).ok()?;
    guard
        .observe(point)
        .err()
        .map(|error| TelemetryRuntimeRefusal::from_guard(&error))
}

/// Decide one field in one sink.
///
/// Eight rules, in this order, and the order is the contract:
///
/// 1. **provenance** -- a caller-asserted class is refused before anything is read
/// 2. **shape** -- a payload key, or an identifier in a sink that admits no identifier label
/// 3. **content** -- the derived basis is `SecretContent`
/// 4. **class** -- the derived class against the sink's ceiling
/// 5. **export ceiling** -- the caller's declared downstream class against the sink's floor
/// 6. **label budget** -- the label's observed value count against the sink's dimension budget
/// 7. **runtime guard** -- the live `MetricCardinalityGuard`, for the metric sink only
/// 8. **guarantee** -- the sink's `RedactedAtSink` / `Unverified` / `Unknown` state
///
/// Provenance is first because every later rule reads a class this module derived; believing the
/// caller's class first would make the rest of the ladder decorative. Guarantee is last because it
/// is the only fact no amount of inspecting the field can establish, and an unestablished guarantee
/// is still a refusal -- silence is not proof.
pub fn admit_field(
    field: &SinkField,
    sink: TelemetryChannel,
    guarantee: Option<TelemetryChannelGuarantee>,
    catalog: Option<&MetricCatalog>,
) -> Result<SinkAdmission, String> {
    field.validate()?;

    let derived = derive_sink_class(field, sink);
    let value_digest = derived.value_digest.clone();
    let refuse = |refusal: SinkAdmissionRefusal| {
        Ok(SinkAdmission {
            schema: SINK_ADMISSION_REPORT_SCHEMA.to_owned(),
            version: DATA_CLASS_ADMISSION_VERSION,
            key: field.key.clone(),
            sink,
            admitted: false,
            derived_class: derived.class,
            basis: derived.basis,
            reason: refusal.as_str().to_owned(),
            value_digest: value_digest.clone(),
        })
    };
    let admit = || {
        Ok(SinkAdmission {
            schema: SINK_ADMISSION_REPORT_SCHEMA.to_owned(),
            version: DATA_CLASS_ADMISSION_VERSION,
            key: field.key.clone(),
            sink,
            admitted: true,
            derived_class: derived.class,
            basis: derived.basis,
            reason: String::new(),
            value_digest: value_digest.clone(),
        })
    };

    // 1. Provenance.
    if field.asserted_class.is_some() {
        return refuse(SinkAdmissionRefusal::CallerSuppliedClass);
    }

    // 2. Shape. A label is a dimension, so an identifier in that position is refused in the sinks
    //    that admit no identifier label -- before any content is read, so the reason names the real
    //    defect rather than a secret that happens to be inside it.
    if field.is_label
        && !TelemetryChannelPolicy::admits_identifier_label(sink)
        && is_high_cardinality_label(&field.key)
    {
        return refuse(SinkAdmissionRefusal::Shared(TelemetryRefusal::HighCardinalityLabel));
    }
    // A metric label is a dimension, so free text is not representable in that position at all.
    if sink == TelemetryChannel::Metric && !field.is_label && !field.is_reference {
        return refuse(SinkAdmissionRefusal::Shared(
            TelemetryRefusal::PayloadShapeForbidden,
        ));
    }
    // A label is a bounded dimension by construction, so `prompt_version` stays a legal dimension.
    if !field.is_label && is_payload_key(&field.key) {
        return refuse(SinkAdmissionRefusal::Shared(
            TelemetryRefusal::PayloadShapeForbidden,
        ));
    }

    // 3. Content. `derive_sink_class` already ran both content tests; reading its basis back is how
    //    the refusal is attributed to content rather than to shape.
    if derived.basis == ClassBasis::SecretContent {
        return refuse(SinkAdmissionRefusal::Shared(TelemetryRefusal::UnredactedContent));
    }

    // 4. Class ceiling.
    if class_rank(derived.class) > class_rank(TelemetryChannelPolicy::ceiling(sink)) {
        return refuse(SinkAdmissionRefusal::Shared(TelemetryRefusal::DataClassTooHigh));
    }

    // 5. Export ceiling against the sink's floor.
    if class_rank(field.export_class_ceiling) > class_rank(TelemetryChannelPolicy::floor(sink)) {
        return refuse(SinkAdmissionRefusal::Shared(
            TelemetryRefusal::ExportClassCeilingTooHigh,
        ));
    }

    // 6. Label budget, read from the same bound the runtime guard enforces.
    if field.is_label && field.observed_label_values as usize > metric_label_budget() {
        return refuse(SinkAdmissionRefusal::Shared(
            TelemetryRefusal::LabelCardinalityExceeded,
        ));
    }

    // 7. The live runtime guard. Absent for every sink but the metric, and a metric decision with
    //    no catalog is refused rather than waved through.
    if sink == TelemetryChannel::Metric {
        let Some(catalog) = catalog else {
            return refuse(SinkAdmissionRefusal::MetricCatalogAbsent);
        };
        let point = metric_point_for(field, catalog)?;
        if let Some(runtime) = runtime_metric_refusal(catalog, &point) {
            return refuse(SinkAdmissionRefusal::Runtime(runtime));
        }
    }

    // 8. Guarantee. A channel that never reported is refused.
    if guarantee != Some(TelemetryChannelGuarantee::RedactedAtSink) {
        return refuse(SinkAdmissionRefusal::Shared(
            TelemetryRefusal::ChannelGuaranteeUnknown,
        ));
    }

    admit()
}

/// Build the point the runtime guard is asked about.
///
/// A label field is checked as a label on a real catalog metric; a reference field is checked as a
/// label-free point, because a dimension is the only thing that can grow without bound.
fn metric_point_for(field: &SinkField, catalog: &MetricCatalog) -> Result<MetricPoint, String> {
    let definition = catalog
        .metric(&field.key)
        .ok_or_else(|| "metric_unregistered".to_owned())?;
    let mut labels = std::collections::BTreeMap::new();
    if field.is_label {
        labels.insert(field.key.clone(), field.value.clone());
    }
    // The point takes its kind, unit and source from the definition rather than from constants here:
    // a point that disagreed with its own catalog entry would be refused by `validate_with_catalog`
    // for the wrong reason, and the operator would be reading a shape error instead of the label
    // decision this module exists to make.
    MetricPoint::new(
        definition.name.clone(),
        definition.kind,
        0.0,
        definition.unit.clone(),
        labels,
        definition.source,
        0,
        Vec::<EventId>::new(),
    )
}

/// Position of a [`DataClass`] on the existing public/internal/confidential/restricted ladder.
///
/// `DataClass` deliberately does not derive `Ord` in `kiana-domain`, so the ordering is expressed
/// once here rather than by a comparison per call site. This is the same private helper the domain
/// contract carries internally; it is restated because it is not exported, and it classifies the
/// *same* four variants in the same order, so it adds no second ladder.
const fn class_rank(class: DataClass) -> u8 {
    match class {
        DataClass::Public => 0,
        DataClass::Internal => 1,
        DataClass::Confidential => 2,
        DataClass::Restricted => 3,
    }
}

/// The upper bound on distinct metric label values this contract will admit.
///
/// Read from the domain constant rather than restated, so a source-level claim can never exceed
/// what the runtime guard already accepts.
pub const fn metric_label_budget() -> usize {
    MAX_METRIC_LABEL_VALUES
}

/// The upper bound on metric series this contract will admit.
pub const fn metric_series_budget() -> usize {
    MAX_METRIC_SERIES
}

fn derive_admissions(
    fields: &[SinkField],
    guarantees: &TelemetryGuarantees,
    catalog: Option<&MetricCatalog>,
) -> Result<Vec<SinkAdmission>, String> {
    let mut admissions = Vec::new();
    for field in fields {
        field.validate()?;
        for sink in TelemetryChannel::ALL {
            admissions.push(admit_field(field, sink, guarantees.get(sink), catalog)?);
        }
    }
    Ok(admissions)
}

/// Evaluate the whole matrix and seal it.
///
/// The five sinks are walked in `TelemetryChannel::ALL` order for every field, so the same facts
/// always produce the same reason string and therefore the same `report_digest` -- which is what
/// makes a refusal quotable in an incident.
pub fn evaluate_sink_admission(
    request_id: RequestId,
    fields: &[SinkField],
    guarantees: &TelemetryGuarantees,
    catalog: Option<&MetricCatalog>,
) -> Result<SinkAdmissionReport, String> {
    if fields.is_empty() || fields.len() > MAX_SINK_ADMISSION_FIELDS {
        return Err("sink_admission_field_count_invalid".to_owned());
    }
    let admissions = derive_admissions(fields, guarantees, catalog)?;
    let mut refused_keys: Vec<String> = Vec::new();
    for cell in admissions.iter().filter(|cell| !cell.admitted) {
        if !refused_keys.contains(&cell.key) {
            refused_keys.push(cell.key.clone());
        }
    }
    let reason = admissions
        .iter()
        .find(|cell| !cell.admitted)
        .map(|cell| format!("{}:{}:{}", cell.reason, cell.key, cell.sink.as_str()))
        .unwrap_or_default();
    let mut report = SinkAdmissionReport {
        schema: SINK_ADMISSION_REPORT_SCHEMA.to_owned(),
        version: DATA_CLASS_ADMISSION_VERSION,
        request_id,
        admissions,
        refused_keys,
        reason,
        catalog_digest: catalog.map(|catalog| catalog.catalog_digest.clone()).unwrap_or_default(),
        report_digest: String::new(),
    };
    report.report_digest = report.digest();
    report.validate_against(request_id, fields, guarantees, catalog)?;
    Ok(report)
}

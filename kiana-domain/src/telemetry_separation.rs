//! BQ-25 per-channel telemetry safety: one `DataClass` decision per egress channel.
//!
//! Telemetry is a *lower* trust channel than the event log. A metric label is indexed, retained
//! and exported to systems that carry none of the redaction the log has, so a value that is
//! defensible inside an `Event` can be a leak inside a `Metric`. This module therefore never asks
//! "is this value safe?" once; it asks "is this value safe *for this channel*", with the channel
//! order fixed so the same facts always produce the same reason.
//!
//! It is a read-only decision over adapter-reported channel facts. It does not emit a metric,
//! write a log line, export a trace span, append a receipt, or redact anything on a caller's
//! behalf — the redaction primitives it consults ([`crate::redact_text`],
//! [`crate::scan_secret_sentinels`], [`RedactionProfile`], [`SecretScanChannel`]) are the existing
//! system and are reused unchanged. A producer that cannot obtain a decision must drop the
//! signal; there is no permissive default and no "redact later" path, because the channels that
//! need this contract are precisely the ones where later means never.

use crate::{
    json_digest, redact_text, scan_secret_sentinels, DataClass, EventCursor, RequestId,
    SchemaVersion, SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const TELEMETRY_CHANNEL_POLICY_SCHEMA: &str = "kiana.telemetry-channel-policy.v1";
pub const TELEMETRY_SAFETY_REQUEST_SCHEMA: &str = "kiana.telemetry-safety-request.v1";
pub const TELEMETRY_CHANNEL_CANDIDATE_SCHEMA: &str = "kiana.telemetry-channel-candidate.v1";
pub const TELEMETRY_SAFETY_REPORT_SCHEMA: &str = "kiana.telemetry-safety-report.v1";
pub const TELEMETRY_SEPARATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

pub const MAX_TELEMETRY_CANDIDATES: usize = 64;
pub const MAX_TELEMETRY_TEXT: usize = 256;
pub const MAX_TELEMETRY_LABEL_VALUE_BYTES: usize = 128;
pub const MAX_TELEMETRY_LABEL_KEYS: usize = 32;
/// Ceiling a single candidate's channel-name set may declare, so one request cannot enumerate an
/// unbounded channel list and be evaluated in a different order each time it is replayed.
pub const MAX_TELEMETRY_CHANNELS: usize = 5;

/// The minimum class a channel will accept without an explicit decision. `Public` is the floor
/// because a label is exported to systems with none of this crate's redaction, so anything above
/// public must be admitted on purpose, not by accident.
pub const MIN_TELEMETRY_DATA_CLASS: DataClass = DataClass::Public;

/// The five telemetry channels the card names, in their fixed evaluation order.
///
/// The order is by trust, and it is a *gate* order rather than a ranking: an item aimed at `Metric`
/// is refused on metric grounds no matter how safe its same value would be as an `Event`, because
/// the metric backend never had the redaction the log has. Reordering the list would change which
/// reason a multi-channel candidate reports, so it is pinned as `ALL` rather than left implicit.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryChannel {
    /// The ordered, replayable fact log. The highest-trust telemetry sink.
    Event,
    /// Structured diagnostic output. Reuses the existing `Log` redaction profile.
    Log,
    /// A recorded span/trace attribute.
    Trace,
    /// A sampled, indexed, retained time series. The lowest-trust telemetry sink: a label here is
    /// a dimension, and a dimension is never a place to put an identifier or a path.
    Metric,
    /// A business receipt. Lowest trust of all, so it is evaluated last.
    Receipt,
}

impl TelemetryChannel {
    pub const ALL: [Self; 5] = [
        Self::Event,
        Self::Log,
        Self::Trace,
        Self::Metric,
        Self::Receipt,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::Log => "log",
            Self::Trace => "trace",
            Self::Metric => "metric",
            Self::Receipt => "receipt",
        }
    }

    /// Position in the fixed evaluation order. `Event` is step zero.
    pub const fn rank(self) -> usize {
        match self {
            Self::Event => 0,
            Self::Log => 1,
            Self::Trace => 2,
            Self::Metric => 3,
            Self::Receipt => 4,
        }
    }

    /// The existing secret-scan channel this telemetry channel is scanned under. Reusing
    /// [`SecretScanChannel`] is the point: a second sink vocabulary would let an adapter pick a
    /// scanner that is weaker than the one the event log already uses.
    pub const fn secret_scan_channel(self) -> SecretScanChannel {
        match self {
            Self::Event => SecretScanChannel::Event,
            Self::Log => SecretScanChannel::Stderr,
            Self::Trace => SecretScanChannel::Transcript,
            Self::Metric => SecretScanChannel::Cache,
            Self::Receipt => SecretScanChannel::Receipt,
        }
    }
}

/// Position of a [`DataClass`] on the existing public/internal/confidential/restricted ladder.
///
/// `DataClass` deliberately does not derive `Ord`, so the ordering is expressed once here rather
/// than by a `match` per comparison. The ladder is the declaration order of the enum, and any new
/// variant would have to be added to this function, which is the point: a new class cannot become
/// comparable by accident.
const fn data_class_rank(class: DataClass) -> u8 {
    match class {
        DataClass::Public => 0,
        DataClass::Internal => 1,
        DataClass::Confidential => 2,
        DataClass::Restricted => 3,
    }
}

/// A telemetry label. A label is a *dimension*, not a payload: a bounded, low-cardinality key whose
/// value is either one of a fixed enumeration or opaque. This is the only shape a metric value may
/// take, and the only way an identifier can appear in telemetry at all — as a digest, not as text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetryLabel {
    pub key: String,
    pub value: String,
    /// Bounded observed distinct-value count for this key. Capped by
    /// [`crate::MAX_METRIC_LABEL_VALUES`], the same bound the metric cardinality guard already
    /// enforces at runtime, so a source-level claim cannot exceed what the runtime accepts.
    pub observed_values: u32,
}

impl TelemetryLabel {
    pub fn new(key: impl Into<String>, value: impl Into<String>, observed_values: u32) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            observed_values,
        }
    }

    /// A single-value label (an enumeration member or a digest-shaped reference).
    pub fn single(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(key, value, 1)
    }

    /// Structural checks only. The *dimension budget* is deliberately not checked here: a label's
    /// acceptable value count is a property of the channel it is aimed at, so the bound lives in
    /// [`TelemetryChannelPolicy::label_budget`] and is applied per channel in [`decide`]. Bounding
    /// it here instead would have made the per-channel budget unreachable, because every candidate
    /// is validated before any channel is considered.
    pub fn validate(&self) -> Result<(), String> {
        safe_text(&self.key, "telemetry_label_key")?;
        if self.value.len() > MAX_TELEMETRY_LABEL_VALUE_BYTES {
            return Err("telemetry_label_value_too_long".to_owned());
        }
        if self.value.as_bytes().contains(&0) {
            return Err("telemetry_label_value_invalid".to_owned());
        }
        if self.observed_values == 0 {
            return Err("telemetry_label_cardinality_invalid".to_owned());
        }
        Ok(())
    }
}

/// What kind of value a candidate is. The kind decides the whole decision path, so a producer
/// cannot reach the text checks by mislabelling a payload as a reference.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryValueKind {
    /// A bounded, already-redacted message or field text.
    Text,
    /// An opaque handle to stored bytes: a digest, a receipt/ledger entry, an error digest.
    Reference,
    /// A low-cardinality label dimension.
    Label,
}

impl TelemetryValueKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Reference => "reference",
            Self::Label => "label",
        }
    }
}

/// One value proposed for one or more telemetry channels.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetryChannelCandidate {
    pub schema: String,
    pub version: SchemaVersion,
    pub key: String,
    pub kind: TelemetryValueKind,
    pub data_class: DataClass,
    /// Present exactly when `kind` is `Text`. Its content is never read by this module: a text
    /// candidate is admitted on the class floor and the redaction guarantee it claims, and it is
    /// refused on shape alone for `Metric`, which has no such guarantee to claim.
    pub text: Option<String>,
    /// Present exactly when `kind` is `Reference`. A reference must be a bounded, secret-free
    /// digest or an opaque handle, because a reference is the one shape that is safe to keep in
    /// every channel including `Metric`.
    pub reference: Option<String>,
    /// Present exactly when `kind` is `Label`.
    pub label: Option<TelemetryLabel>,
    /// Channels this candidate is proposed for, in `TelemetryChannel::ALL` order.
    pub channels: Vec<TelemetryChannel>,
    /// The `DataClass` ceiling the caller asserts applies to the receiving systems. A `Metric`
    /// exporter is assumed to sit at or below this; a candidate cannot lower the ceiling to make
    /// its own class acceptable.
    pub export_class_ceiling: DataClass,
    pub candidate_digest: String,
}

impl TelemetryChannelCandidate {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        key: impl Into<String>,
        kind: TelemetryValueKind,
        data_class: DataClass,
        text: Option<String>,
        reference: Option<String>,
        label: Option<TelemetryLabel>,
        channels: Vec<TelemetryChannel>,
        export_class_ceiling: DataClass,
    ) -> Result<Self, String> {
        let mut candidate = Self {
            schema: TELEMETRY_CHANNEL_CANDIDATE_SCHEMA.to_owned(),
            version: TELEMETRY_SEPARATION_VERSION,
            key: key.into(),
            kind,
            data_class,
            text,
            reference,
            label,
            channels,
            export_class_ceiling,
            candidate_digest: String::new(),
        };
        candidate.candidate_digest = candidate.digest();
        candidate.validate()?;
        Ok(candidate)
    }

    /// A bounded, already-redacted text value. Refusable on shape for `Metric` and above the
    /// class floor for the rest.
    pub fn text(
        key: impl Into<String>,
        data_class: DataClass,
        text: impl Into<String>,
        channels: Vec<TelemetryChannel>,
    ) -> Result<Self, String> {
        Self::new(
            key,
            TelemetryValueKind::Text,
            data_class,
            Some(text.into()),
            None,
            None,
            channels,
            MIN_TELEMETRY_DATA_CLASS,
        )
    }

    /// An opaque digest/reference handle. The only shape admissible in every channel, because it
    /// carries no reversible content.
    pub fn reference(
        key: impl Into<String>,
        data_class: DataClass,
        reference: impl Into<String>,
        channels: Vec<TelemetryChannel>,
    ) -> Result<Self, String> {
        Self::new(
            key,
            TelemetryValueKind::Reference,
            data_class,
            None,
            Some(reference.into()),
            None,
            channels,
            MIN_TELEMETRY_DATA_CLASS,
        )
    }

    /// A low-cardinality label dimension.
    pub fn label(
        key: impl Into<String>,
        label: TelemetryLabel,
        channels: Vec<TelemetryChannel>,
    ) -> Result<Self, String> {
        Self::new(
            key,
            TelemetryValueKind::Label,
            MIN_TELEMETRY_DATA_CLASS,
            None,
            None,
            Some(label),
            channels,
            MIN_TELEMETRY_DATA_CLASS,
        )
    }

    /// Whether this candidate is proposed for a channel that is *not* the event log.
    ///
    /// The card's whole point is that the same value needs a per-channel decision, so this is the
    /// predicate that makes `Event`-safe and `Metric`-unsafe expressible without a second system.
    pub fn targets_telemetry(&self) -> bool {
        self.channels
            .iter()
            .any(|channel| *channel != TelemetryChannel::Event)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TELEMETRY_CHANNEL_CANDIDATE_SCHEMA
            || !self
                .version
                .is_compatible_with(&TELEMETRY_SEPARATION_VERSION)
        {
            return Err("telemetry_candidate_schema_invalid".to_owned());
        }
        safe_text(&self.key, "telemetry_candidate_key")?;
        if self.channels.is_empty() || self.channels.len() > MAX_TELEMETRY_CHANNELS {
            return Err("telemetry_candidate_channel_count_invalid".to_owned());
        }
        let mut previous: Option<TelemetryChannel> = None;
        for channel in &self.channels {
            if let Some(earlier) = previous {
                if *channel <= earlier {
                    return Err("telemetry_candidate_channels_out_of_order".to_owned());
                }
            }
            previous = Some(*channel);
        }
        // The producer declares the shape; `validate` refuses any candidate whose declared shape
        // and carried payload disagree, so a payload cannot smuggle itself past the kind rules.
        let (text, reference, label) = (&self.text, &self.reference, &self.label);
        let carried = [text.is_some(), reference.is_some(), label.is_some()]
            .iter()
            .filter(|present| **present)
            .count();
        if carried != 1 {
            return Err("telemetry_candidate_payload_invalid".to_owned());
        }
        match (self.kind, text, reference, label) {
            (TelemetryValueKind::Text, Some(value), None, None) => {
                if value.len() > MAX_TELEMETRY_TEXT {
                    return Err("telemetry_candidate_text_too_long".to_owned());
                }
                safe_text(value, "telemetry_candidate_text")?;
            }
            (TelemetryValueKind::Reference, None, Some(value), None) => {
                safe_text(value, "telemetry_candidate_reference")?;
            }
            (TelemetryValueKind::Label, None, None, Some(value)) => {
                value.validate()?;
            }
            _ => return Err("telemetry_candidate_kind_mismatch".to_owned()),
        }
        valid_digest(&self.candidate_digest, "telemetry_candidate_digest")?;
        if self.candidate_digest != self.digest() {
            return Err("telemetry_candidate_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "key": self.key,
            "kind": self.kind,
            "data_class": self.data_class,
            "text": self.text,
            "reference": self.reference,
            "label": self.label,
            "channels": self.channels,
            "export_class_ceiling": self.export_class_ceiling,
        }))
    }
}

/// Why a value may not reach a channel. The variants are ordered by how badly getting them wrong
/// misleads an operator, and [`derive`] checks them in exactly that order so the reported reason is
/// the first rule violated rather than whichever test happened to run last.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryRefusal {
    /// An unrecoverable shape: a UUID, a filesystem path, a URL, a prompt, a raw provider response,
    /// an invoice account secret, an API key, or anything else `redact_text` would rewrite.
    /// Detected positively, so it outranks every absence-of-proof below it.
    UnredactedContent,
    /// The value's class is above the receiving channel's floor. Checked before the class ceiling
    /// because "this is Confidential and a metric will never hold Confidential" is the actionable
    /// statement, while the ceiling is the weaker defence behind it.
    DataClassTooHigh,
    /// The producer's declared export ceiling is itself above the channel floor, so the channel's
    /// downstream cannot be shown to be safe even if this one value were.
    ExportClassCeilingTooHigh,
    /// A high-cardinality identifier used as a metric label. Unbounded dimension growth in
    /// disguise, and the specific failure the card names first for metrics.
    HighCardinalityLabel,
    /// A text or label value aimed at a channel that only admits bounded enumerations. A metric
    /// label is a dimension: a payload in that position is an index of secrets.
    PayloadShapeForbidden,
    /// A label the channel's dimension budget cannot hold.
    LabelCardinalityExceeded,
    /// The adapter could not establish the channel's redaction guarantee. Silence is not proof that
    /// the channel is safe, so this is a refusal rather than a pass.
    ChannelGuaranteeUnknown,
}

impl TelemetryRefusal {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnredactedContent => "unredacted_content",
            Self::DataClassTooHigh => "data_class_too_high",
            Self::ExportClassCeilingTooHigh => "export_class_ceiling_too_high",
            Self::HighCardinalityLabel => "high_cardinality_label",
            Self::PayloadShapeForbidden => "payload_shape_forbidden",
            Self::LabelCardinalityExceeded => "label_cardinality_exceeded",
            Self::ChannelGuaranteeUnknown => "channel_guarantee_unknown",
        }
    }
}

/// The per-channel class policy. This is the whole BQ-25 class decision, in one table, and it is
/// expressed as two numbers per channel rather than a second classification vocabulary: the
/// [`DataClass`] a channel can hold (`ceiling`) and the narrowest class it will take without an
/// explicit decision (`floor`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TelemetryChannelPolicy;

impl TelemetryChannelPolicy {
    /// The highest [`DataClass`] one channel will hold.
    ///
    /// `Event`, `Log` and `Trace` share the highest ceiling because they share the existing
    /// redaction pipeline: a value redacted for the log is redacted for the trace, so a second
    /// opinion there would be a second system rather than a stricter rule. `Metric` is capped at
    /// `Internal` because a metric backend retains and indexes what it is given and typically has
    /// none of this crate's redaction. `Receipt` is capped at `Confidential` because a receipt is a
    /// business artefact rather than a diagnostic.
    pub const fn ceiling(channel: TelemetryChannel) -> DataClass {
        match channel {
            TelemetryChannel::Event | TelemetryChannel::Log | TelemetryChannel::Trace => {
                DataClass::Restricted
            }
            TelemetryChannel::Metric => DataClass::Internal,
            TelemetryChannel::Receipt => DataClass::Confidential,
        }
    }

    /// The narrowest [`DataClass`] one channel admits without an explicit decision.
    ///
    /// `Receipt` is one step higher than the rest because it is a business artefact: an
    /// unreviewed `Public`-class row in a receipt is a statement about someone's money, not a
    /// diagnostic, so it has to be put there on purpose.
    pub const fn floor(channel: TelemetryChannel) -> DataClass {
        match channel {
            TelemetryChannel::Event
            | TelemetryChannel::Log
            | TelemetryChannel::Trace
            | TelemetryChannel::Metric => DataClass::Public,
            TelemetryChannel::Receipt => DataClass::Internal,
        }
    }

    /// Whether a high-cardinality identifier may be used as a dimension in this channel at all.
    ///
    /// `Metric` and `Trace` are dimension-shaped channels: they index and retain what they are
    /// given, so no identifier may become a label there and the same value that is legal as an
    /// `Event` field is refused as a `Metric` label. `Event`, `Log` and `Receipt` record facts
    /// rather than dimensions, so identity is legal in them. That asymmetry is the whole reason the
    /// decision is per channel.
    /// The largest number of distinct values one label may claim in this channel.
    ///
    /// Every channel shares the runtime guard's bound, because every one of them indexes or
    /// retains what it is given; the bound is [`crate::MAX_METRIC_LABEL_VALUES`] so a source-level
    /// claim can never exceed what the metric cardinality guard already accepts at runtime.
    pub const fn label_budget(_channel: TelemetryChannel) -> usize {
        crate::MAX_METRIC_LABEL_VALUES
    }

    pub const fn admits_identifier_label(channel: TelemetryChannel) -> bool {
        match channel {
            TelemetryChannel::Event | TelemetryChannel::Log | TelemetryChannel::Receipt => true,
            TelemetryChannel::Metric | TelemetryChannel::Trace => false,
        }
    }
}

/// What the adapter reported about one channel's redaction guarantee.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryChannelGuarantee {
    /// The channel runs the same `RedactionProfile` and secret sentinel scan before the value is
    /// stored, so a value admitted here is redacted at the sink and not only at the producer.
    RedactedAtSink,
    /// The channel stores whatever it is given. This is the state a raw metric or trace exporter
    /// is in, and it is why telemetry is treated as a lower trust channel than the log.
    Unverified,
    /// The adapter could not establish the guarantee. Treated exactly like `Unverified`.
    Unknown,
}

impl TelemetryChannelGuarantee {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RedactedAtSink => "redacted_at_sink",
            Self::Unverified => "unverified",
            Self::Unknown => "unknown",
        }
    }

    pub const fn is_established(self) -> bool {
        matches!(self, Self::RedactedAtSink)
    }
}

/// The bounded decision for one value in one channel.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetryChannelDecision {
    pub schema: String,
    pub version: SchemaVersion,
    pub key: String,
    pub channel: TelemetryChannel,
    pub kind: TelemetryValueKind,
    pub data_class: DataClass,
    pub admitted: bool,
    /// The first rule violated, empty when admitted. Never carries the offending value.
    pub reason: String,
    /// Digest of the value that was evaluated, so two channels' decisions can be correlated without
    /// either of them carrying the value. Empty for text values, which are not addressable.
    pub value_digest: String,
    pub decision_digest: String,
}

impl TelemetryChannelDecision {
    pub fn admitted(&self) -> bool {
        self.admitted
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TELEMETRY_CHANNEL_POLICY_SCHEMA
            || !self
                .version
                .is_compatible_with(&TELEMETRY_SEPARATION_VERSION)
            || self.channel.rank() >= MAX_TELEMETRY_CHANNELS
        {
            return Err("telemetry_decision_schema_invalid".to_owned());
        }
        safe_text(&self.key, "telemetry_decision_key")?;
        if self.admitted && !self.reason.is_empty() {
            return Err("telemetry_decision_admitted_with_reason".to_owned());
        }
        if !self.admitted && self.reason.is_empty() {
            return Err("telemetry_decision_refused_without_reason".to_owned());
        }
        if self.reason.contains('\n') || self.reason.len() > 160 {
            return Err("telemetry_decision_reason_invalid".to_owned());
        }
        if !self.value_digest.is_empty() {
            valid_digest(&self.value_digest, "telemetry_decision_value_digest")?;
        }
        valid_digest(&self.decision_digest, "telemetry_decision_digest")?;
        if self.decision_digest != self.digest() {
            return Err("telemetry_decision_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "key": self.key,
            "channel": self.channel,
            "kind": self.kind,
            "data_class": self.data_class,
            "admitted": self.admitted,
            "reason": self.reason,
            "value_digest": self.value_digest,
        }))
    }
}

/// Decide one candidate for one channel.
///
/// The order below is the whole policy and it is fixed:
///
/// 1. the declared shape must be legal for this channel (`payload_shape_forbidden`),
/// 2. the content must already be redacted and secret-free (`unredacted_content`),
/// 3. a label must be a low-cardinality dimension (`high_cardinality_label`),
/// 4. the value's class must be at or below the channel's ceiling (`data_class_too_high`),
/// 5. the declared export ceiling must be at or below the channel's floor
///    (`export_class_ceiling_too_high`),
/// 6. a label's observed cardinality must fit the channel's budget
///    (`label_cardinality_exceeded`),
/// 7. the channel's redaction guarantee must be established (`channel_guarantee_unknown`).
///
/// Rules 1 and 2 run first because they are positive detections: a UUID in a metric label is a
/// leak that is already visible in the candidate, and reporting `data_class_too_high` for it would
/// bury that. Rule 7 is last because every earlier rule is decidable from the candidate alone,
/// while the guarantee is the one fact only the adapter can establish; an unestablished guarantee
/// is still a refusal, never a silent pass.
pub fn evaluate_candidate(
    candidate: &TelemetryChannelCandidate,
    channel: TelemetryChannel,
    guarantee: TelemetryChannelGuarantee,
) -> Result<TelemetryChannelDecision, String> {
    candidate.validate()?;
    if !candidate.channels.contains(&channel) {
        return Err("telemetry_candidate_channel_not_requested".to_owned());
    }
    let (admitted, reason, value_digest) = decide(candidate, channel, guarantee);
    let mut decision = TelemetryChannelDecision {
        schema: TELEMETRY_CHANNEL_POLICY_SCHEMA.to_owned(),
        version: TELEMETRY_SEPARATION_VERSION,
        key: candidate.key.clone(),
        channel,
        kind: candidate.kind,
        data_class: candidate.data_class,
        admitted,
        reason: reason.to_owned(),
        value_digest,
        decision_digest: String::new(),
    };
    decision.decision_digest = decision.digest();
    decision.validate()?;
    Ok(decision)
}

fn decide(
    candidate: &TelemetryChannelCandidate,
    channel: TelemetryChannel,
    guarantee: TelemetryChannelGuarantee,
) -> (bool, &'static str, String) {
    let value_digest = || -> String {
        match (&candidate.kind, &candidate.reference, &candidate.label) {
            (TelemetryValueKind::Reference, Some(reference), _) => {
                json_digest(&serde_json::json!({
                    "reference": reference,
                }))
            }
            (TelemetryValueKind::Label, _, Some(label)) => json_digest(&serde_json::json!({
                "label": label,
            })),
            _ => String::new(),
        }
    };

    // 1. Shape. A metric label is a dimension: a text value is not representable there at all, and
    //    an identifier is not a dimension anywhere. Both are refused on the declared shape, before
    //    any content is read, so the reason names the real defect rather than a secret that happens
    //    to be inside it.
    if candidate.kind == TelemetryValueKind::Label
        && !TelemetryChannelPolicy::admits_identifier_label(channel)
        && candidate
            .label
            .as_ref()
            .is_some_and(|label| is_high_cardinality_label(&label.key))
    {
        return (
            false,
            TelemetryRefusal::HighCardinalityLabel.as_str(),
            value_digest(),
        );
    }
    if channel == TelemetryChannel::Metric && candidate.kind == TelemetryValueKind::Text {
        return (
            false,
            TelemetryRefusal::PayloadShapeForbidden.as_str(),
            value_digest(),
        );
    }
    // A payload-shaped key is refused on the key, in *every* channel, and no redaction makes it
    // safe: redacting a prompt leaves a prompt. This is the only content rule that does not go
    // through the sentinel scanner, because the scanner looks for secret shapes and a prompt has
    // none — it is simply not a telemetry value. A `Label` candidate is exempt: a label is a
    // bounded dimension by construction, and keys like `prompt_version` are legitimate dimensions.
    if !matches!(candidate.kind, TelemetryValueKind::Label) && is_payload_key(&candidate.key) {
        return (
            false,
            TelemetryRefusal::PayloadShapeForbidden.as_str(),
            value_digest(),
        );
    }

    // 2. Content. The existing redaction system, used unchanged: a candidate that `redact_text`
    //    would still rewrite has not been redacted, and one that the sentinel scan still rejects
    //    carries a secret shape. The scan runs under the channel's existing
    //    `SecretScanChannel`, so a value is held to the same scanner the sink already uses.
    if let Some(refusal) = content_refusal(candidate, channel) {
        return (false, refusal.as_str(), value_digest());
    }

    // 3. The value's own class against the channel's ceiling.
    if data_class_rank(candidate.data_class)
        > data_class_rank(TelemetryChannelPolicy::ceiling(channel))
    {
        return (
            false,
            TelemetryRefusal::DataClassTooHigh.as_str(),
            value_digest(),
        );
    }

    // 4. The caller's declared export ceiling against the channel's floor.
    if data_class_rank(candidate.export_class_ceiling)
        > data_class_rank(TelemetryChannelPolicy::floor(channel))
    {
        return (
            false,
            TelemetryRefusal::ExportClassCeilingTooHigh.as_str(),
            value_digest(),
        );
    }

    // 5. A dimension has a bounded number of values; a label claiming more cannot be admitted as
    //    a dimension, and `TelemetryLabel::validate` has already bounded the claim itself.
    if let Some(label) = &candidate.label {
        if label.observed_values as usize > TelemetryChannelPolicy::label_budget(channel) {
            return (
                false,
                TelemetryRefusal::LabelCardinalityExceeded.as_str(),
                value_digest(),
            );
        }
    }

    // 6. Silence is not proof.
    if !guarantee.is_established() {
        return (
            false,
            TelemetryRefusal::ChannelGuaranteeUnknown.as_str(),
            value_digest(),
        );
    }

    (true, "", value_digest())
}
// 3. The value's own class against the channel's ceiling.

fn content_refusal(
    candidate: &TelemetryChannelCandidate,
    channel: TelemetryChannel,
) -> Option<TelemetryRefusal> {
    let scan_channel = channel.secret_scan_channel();
    match candidate.kind {
        TelemetryValueKind::Text => {
            let text = candidate.text.as_deref()?;
            if redact_text(text) != text {
                return Some(TelemetryRefusal::UnredactedContent);
            }
            if scan_secret_sentinels(scan_channel, text).is_err() {
                return Some(TelemetryRefusal::UnredactedContent);
            }
            None
        }
        TelemetryValueKind::Reference => {
            let reference = candidate.reference.as_deref()?;
            if redact_text(reference) != reference {
                return Some(TelemetryRefusal::UnredactedContent);
            }
            if scan_secret_sentinels(scan_channel, reference).is_err() {
                return Some(TelemetryRefusal::UnredactedContent);
            }
            None
        }
        TelemetryValueKind::Label => {
            let label = candidate.label.as_ref()?;
            for value in [label.key.as_str(), label.value.as_str()] {
                if redact_text(value) != value {
                    return Some(TelemetryRefusal::UnredactedContent);
                }
                if scan_secret_sentinels(scan_channel, value).is_err() {
                    return Some(TelemetryRefusal::UnredactedContent);
                }
            }
            None
        }
    }
}

/// Whether a candidate key names a payload rather than a telemetry value.
///
/// A payload is a body, an instruction or an argument list. Unlike a secret, it has no sentinel
/// shape, so the shared scanner cannot see it: `redact_text` rewrites an API key and leaves a
/// prompt alone. Such a key is therefore refused in every channel on the key alone, which is the
/// only way "the prompt was not redacted" can never be the answer.
pub fn is_payload_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("prompt")
        || lower.contains("raw_response")
        || lower.contains("raw_request")
        || lower.contains("raw_body")
        || lower.contains("raw_output")
        || lower.contains("tool_args")
        || lower.contains("tool_input")
        || lower.contains("arguments")
        || lower.contains("command")
        || lower.contains("shell")
        || lower.contains("transcript")
}

/// Whether a label key names a high-cardinality identifier rather than a dimension.
///
/// A run id, request id or filesystem path used as a label is unbounded dimension growth wearing a
/// label's clothes, so the key itself is enough evidence; the value is not consulted. This list is
/// the same shape as the runtime forbidden-label list in the metric cardinality guard, kept here as
/// a *source* rule so the decision does not depend on a guard having run first.
pub fn is_high_cardinality_label(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.ends_with("_id")
        || lower.ends_with("_ids")
        || lower.ends_with("_uuid")
        || lower.ends_with("_path")
        || lower.ends_with("_dir")
        || lower.ends_with("_ref")
        || lower.ends_with("_url")
        || lower.ends_with("_uri")
        || lower.contains("request_id")
        || lower.contains("run_id")
        || lower.contains("session_id")
        || lower.contains("trace_id")
        || lower.contains("span_id")
        || lower.contains("step_id")
        || lower.contains("turn_id")
        || lower.contains("event_id")
        || lower.contains("artifact_id")
        || lower.contains("invocation_id")
        || lower.contains("execution_id")
        || lower.contains("project_id")
        || lower.contains("user_id")
        || lower.contains("principal_id")
        || lower.contains("file")
        || lower.contains("path")
        || lower.contains("prompt")
        || lower.contains("raw_response")
        || lower.contains("raw_request")
        || lower.contains("command")
        || lower.contains("argument")
        || lower.contains("body")
}

/// One bounded request covering the candidates a producer wants to emit and the guarantees the
/// adapters reported.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetrySafetyRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub request_id: RequestId,
    pub candidates: Vec<TelemetryChannelCandidate>,
    /// Guarantee per channel. A channel absent from this map is treated as
    /// [`TelemetryChannelGuarantee::Unknown`], never as a pass.
    pub guarantees: BTreeMap<TelemetryChannel, TelemetryChannelGuarantee>,
    pub source_cursor: EventCursor,
    pub request_digest: String,
}

impl TelemetrySafetyRequest {
    pub fn new(
        request_id: RequestId,
        candidates: Vec<TelemetryChannelCandidate>,
        guarantees: BTreeMap<TelemetryChannel, TelemetryChannelGuarantee>,
        source_cursor: EventCursor,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: TELEMETRY_SAFETY_REQUEST_SCHEMA.to_owned(),
            version: TELEMETRY_SEPARATION_VERSION,
            request_id,
            candidates,
            guarantees,
            source_cursor,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    /// The guarantee for a channel. An unreported channel is `Unknown`, so an adapter that never
    /// spoke cannot be read as an adapter that spoke and cleared the channel.
    pub fn guarantee(&self, channel: TelemetryChannel) -> TelemetryChannelGuarantee {
        self.guarantees
            .get(&channel)
            .copied()
            .unwrap_or(TelemetryChannelGuarantee::Unknown)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TELEMETRY_SAFETY_REQUEST_SCHEMA
            || !self
                .version
                .is_compatible_with(&TELEMETRY_SEPARATION_VERSION)
            || self.request_id.as_uuid().is_nil()
            || self.source_cursor == 0
        {
            return Err("telemetry_request_schema_invalid".to_owned());
        }
        if self.candidates.is_empty() || self.candidates.len() > MAX_TELEMETRY_CANDIDATES {
            return Err("telemetry_request_candidate_limit".to_owned());
        }
        if self.guarantees.len() > MAX_TELEMETRY_CHANNELS {
            return Err("telemetry_request_guarantee_limit".to_owned());
        }
        let mut keys = std::collections::BTreeSet::new();
        for candidate in &self.candidates {
            candidate.validate()?;
            if !keys.insert((candidate.key.as_str(), candidate.kind)) {
                return Err("telemetry_request_duplicate_candidate".to_owned());
            }
        }
        valid_digest(&self.request_digest, "telemetry_request_digest")?;
        if self.request_digest != self.digest() {
            return Err("telemetry_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "request_id": self.request_id,
            "candidates": self.candidates,
            "guarantees": self.guarantees,
            "source_cursor": self.source_cursor,
        }))
    }
}

/// The ordered outcome of one safety request.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetrySafetyStatus {
    /// Every candidate was admitted in every channel it requested.
    Admitted,
    /// At least one candidate was refused in at least one channel.
    Refused,
}

impl TelemetrySafetyStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admitted => "admitted",
            Self::Refused => "refused",
        }
    }
}

/// The report for one telemetry safety request: a per-channel decision for every candidate.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetrySafetyReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub request_id: RequestId,
    pub status: TelemetrySafetyStatus,
    /// One decision per (candidate, requested channel), in request order then
    /// `TelemetryChannel::ALL` order. A candidate is not silently dropped: it appears in every
    /// channel it asked for, refused or not.
    pub decisions: Vec<TelemetryChannelDecision>,
    /// The candidates that were refused somewhere, by key, in request order. Empty when admitted.
    pub refused_keys: Vec<String>,
    /// The leading rule violated, suffixed with the offending key and channel. Empty when admitted.
    pub reason: String,
    pub remediation: String,
    pub request_digest: String,
    pub report_digest: String,
}

impl TelemetrySafetyReport {
    pub fn evaluate(request: &TelemetrySafetyRequest) -> Result<Self, String> {
        request.validate()?;
        let (status, decisions, refused_keys, reason, remediation) = derive(request);
        let mut report = Self {
            schema: TELEMETRY_SAFETY_REPORT_SCHEMA.to_owned(),
            version: TELEMETRY_SEPARATION_VERSION,
            request_id: request.request_id,
            status,
            decisions,
            refused_keys,
            reason,
            remediation,
            request_digest: request.request_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(request)?;
        Ok(report)
    }

    pub fn validate_against(&self, request: &TelemetrySafetyRequest) -> Result<(), String> {
        request.validate()?;
        let (status, decisions, refused_keys, reason, remediation) = derive(request);
        if self.schema != TELEMETRY_SAFETY_REPORT_SCHEMA
            || !self
                .version
                .is_compatible_with(&TELEMETRY_SEPARATION_VERSION)
            || self.request_id != request.request_id
            || self.status != status
            || self.decisions != decisions
            || self.refused_keys != refused_keys
            || self.reason != reason
            || self.remediation != remediation
            || self.request_digest != request.request_digest
        {
            return Err("telemetry_report_binding_invalid".to_owned());
        }
        // A report that refuses something may not also call itself admitted, and one that admits
        // everything may not carry a refusal list. Either pairing would let a caller read the
        // status line and skip the list.
        if self.status == TelemetrySafetyStatus::Admitted
            && (!self.refused_keys.is_empty() || !self.reason.is_empty())
        {
            return Err("telemetry_report_admitted_with_refusal".to_owned());
        }
        if self.status == TelemetrySafetyStatus::Refused
            && (self.refused_keys.is_empty() || self.reason.is_empty())
        {
            return Err("telemetry_report_refused_without_reason".to_owned());
        }
        if self.decisions.is_empty() {
            return Err("telemetry_report_no_decisions".to_owned());
        }
        valid_digest(&self.report_digest, "telemetry_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("telemetry_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Whether every value may leave the producer on the channel it asked for.
    pub fn admitted(&self) -> bool {
        self.status == TelemetrySafetyStatus::Admitted
    }

    /// The decision for one key in one channel, if the request asked for it.
    pub fn decision(
        &self,
        key: &str,
        channel: TelemetryChannel,
    ) -> Option<&TelemetryChannelDecision> {
        self.decisions
            .iter()
            .find(|decision| decision.key == key && decision.channel == channel)
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "request_id": self.request_id,
            "status": self.status,
            "decisions": self.decisions,
            "refused_keys": self.refused_keys,
            "reason": self.reason,
            "remediation": self.remediation,
            "request_digest": self.request_digest,
        }))
    }
}

type Derived = (
    TelemetrySafetyStatus,
    Vec<TelemetryChannelDecision>,
    Vec<String>,
    String,
    String,
);

/// Decide in a fixed order so the reported reason is the first rule violated, deterministically,
/// for the same facts. The first violation in request order wins; within a candidate, channels
/// are walked in [`TelemetryChannel::ALL`] order, so a value that is unsafe everywhere reports its
/// `Event` reason before its `Metric` reason only if the `Event` decision is the one that failed.
fn derive(request: &TelemetrySafetyRequest) -> Derived {
    let mut decisions = Vec::new();
    let mut refused_keys = Vec::new();
    let mut first: Option<(TelemetryChannelDecision, &'static str)> = None;

    for candidate in &request.candidates {
        let mut candidate_refused = false;
        for channel in TelemetryChannel::ALL {
            if !candidate.channels.contains(&channel) {
                continue;
            }
            let guarantee = request.guarantee(channel);
            let decision = evaluate_candidate(candidate, channel, guarantee)
                .expect("validated candidate produces a valid decision");
            if !decision.admitted {
                candidate_refused = true;
                if first.is_none() {
                    first = Some((decision.clone(), remediation(&decision)));
                }
            }
            decisions.push(decision);
        }
        if candidate_refused {
            refused_keys.push(candidate.key.clone());
        }
    }

    match first {
        Some((decision, remediation)) => (
            TelemetrySafetyStatus::Refused,
            decisions,
            refused_keys,
            format!(
                "telemetry_value_refused:{}:{}:{}",
                decision.reason,
                decision.channel.as_str(),
                decision.key
            ),
            remediation.to_owned(),
        ),
        None => (
            TelemetrySafetyStatus::Admitted,
            decisions,
            refused_keys,
            String::new(),
            String::new(),
        ),
    }
}

fn remediation(decision: &TelemetryChannelDecision) -> &'static str {
    match decision.reason.as_str() {
        "unredacted_content" => {
            "run the value through the existing redaction profile for this channel, or replace it with a digest reference"
        }
        "data_class_too_high" => {
            "downgrade the value to a lower data class, or move it to a channel whose ceiling is high enough"
        }
        "export_class_ceiling_too_high" => {
            "declare an export ceiling at or below this channel's floor, or stop exporting to it"
        }
        "high_cardinality_label" => {
            "an identifier or path is not a dimension; aggregate it into a bounded label and keep the identifier in the event log"
        }
        "payload_shape_forbidden" => {
            "a metric label is a dimension, not a payload; emit a counter, or carry the value as a digest reference"
        }
        "label_cardinality_exceeded" => {
            "reduce the label's value set, or move the dimension to a channel with a larger budget"
        },
        _ => "establish the channel's redaction guarantee before emitting into it",
    }
}

/// Bounded, secret-free text. Rejects the shapes that make a value unusable as a channel key or
/// reference before any content check: a UUID, a filesystem path or a URL.
fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > MAX_TELEMETRY_TEXT {
        return Err(format!("{field}_invalid"));
    }
    if value.contains(['\0', '\r', '\n', ' ', '\t']) {
        return Err(format!("{field}_invalid"));
    }
    if value.contains("..") || value.contains("://") {
        return Err(format!("{field}_invalid"));
    }
    // A UUID is the shape every high-cardinality runtime identifier takes, so it is refused here
    // as a key or reference outright. Identity belongs in the event log and in `trace_id` /
    // `span_id` fields, which are typed ids rather than free text, and it is never a dimension.
    if uuid::Uuid::parse_str(value).is_ok() {
        return Err(format!("{field}_not_reference"));
    }
    Ok(())
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

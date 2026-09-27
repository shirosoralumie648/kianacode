//! BQ-25 source guard.
//!
//! The contract in `kiana-domain/src/telemetry_separation.rs` is a *source* contract, so this guard
//! pins the things a future edit could silently undo: the per-channel decision vocabulary, the
//! reuse of the existing redaction primitives, and the absence of any authority a read-only
//! decision must never acquire. It reads source text only; it starts nothing, writes nothing and
//! asserts nothing about runtime behaviour.

/// Markers that must literally appear in the contract source.
const REQUIRED_MARKERS: &[&str] = &[
    // The five channels the card names, and their fixed order.
    "pub const ALL: [Self; 5] = [",
    "Self::Event,\n        Self::Log,\n        Self::Trace,\n        Self::Metric,\n        Self::Receipt,",
    // The per-channel class policy: a ceiling and a floor per channel, not one global class.
    "pub const fn ceiling(channel: TelemetryChannel) -> DataClass {",
    "pub const fn floor(channel: TelemetryChannel) -> DataClass {",
    "pub const fn label_budget(_channel: TelemetryChannel) -> usize {",
    "pub const fn admits_identifier_label(channel: TelemetryChannel) -> bool {",
    // Each channel maps onto the existing scanner rather than a second sink vocabulary.
    "pub const fn secret_scan_channel(self) -> SecretScanChannel {",
    "SecretScanChannel::Event",
    "SecretScanChannel::Stderr",
    "SecretScanChannel::Transcript",
    "SecretScanChannel::Cache",
    "SecretScanChannel::Receipt",
    // The existing redaction primitives, reused rather than reimplemented.
    "redact_text",
    "scan_secret_sentinels",
    "json_digest",
    // Every card rejection has a named reason.
    "TelemetryRefusal::UnredactedContent",
    "TelemetryRefusal::DataClassTooHigh",
    "TelemetryRefusal::ExportClassCeilingTooHigh",
    "TelemetryRefusal::HighCardinalityLabel",
    "TelemetryRefusal::PayloadShapeForbidden",
    "TelemetryRefusal::LabelCardinalityExceeded",
    "TelemetryRefusal::ChannelGuaranteeUnknown",
    // High-cardinality identifiers and payload keys are refused, not merely documented.
    "pub fn is_high_cardinality_label(key: &str) -> bool {",
    "pub fn is_payload_key(key: &str) -> bool {",
    // Silence is not proof.
    "TelemetryChannelGuarantee::Unknown",
    "pub const fn is_established(self) -> bool {",
    // The reducer shape: fixed-order evaluate plus validate_against.
    "pub fn evaluate(request: &TelemetrySafetyRequest) -> Result<Self, String> {",
    "pub fn validate_against(&self, request: &TelemetrySafetyRequest) -> Result<(), String> {",
    "fn derive(request: &TelemetrySafetyRequest) -> Derived {",
    // Digest sealing on every constructor.
    "candidate.candidate_digest = candidate.digest();",
    "request.request_digest = request.digest();",
    "report.report_digest = report.digest();",
    "decision.decision_digest = decision.digest();",
    // Schema constants and bounds.
    "pub const TELEMETRY_CHANNEL_POLICY_SCHEMA: &str = \"kiana.telemetry-channel-policy.v1\";",
    "pub const TELEMETRY_SAFETY_REQUEST_SCHEMA: &str = \"kiana.telemetry-safety-request.v1\";",
    "pub const TELEMETRY_CHANNEL_CANDIDATE_SCHEMA: &str = \"kiana.telemetry-channel-candidate.v1\";",
    "pub const TELEMETRY_SAFETY_REPORT_SCHEMA: &str = \"kiana.telemetry-safety-report.v1\";",
    "pub const TELEMETRY_SEPARATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);",
    "pub const MAX_TELEMETRY_CANDIDATES: usize = 64;",
    "pub const MAX_TELEMETRY_CHANNELS: usize = 5;",
    // The honest ceiling: an unreported channel is Unknown, and a decision carries no value.
    "fn safe_text(value: &str, field: &str) -> Result<(), String> {",
];

/// Authority a read-only telemetry decision must never acquire.
const FORBIDDEN: &[&str] = &[
    // No execution, no dispatch, no scheduling.
    "tokio::spawn",
    "std::process::Command",
    "reqwest",
    // No control-plane or broker authority: this is a decision, not an action.
    "CapabilityBroker",
    "EventStore",
    "ControlPlane",
    "ModelCallPermit",
    "commit_transition",
    // No network egress of its own: the decision does not ship telemetry.
    "TcpStream",
    "hyper::",
    // No silent redaction: the decision reports a refusal, it does not rewrite the caller's value
    // and hand back a "cleaned" one. `redact_text` may only appear as a predicate, never as an
    // assignment, which is asserted separately below.
    "let mut redacted =",
    "redacted.push_str",
];

#[test]
fn bq25_telemetry_separation_contract_reuses_the_existing_vocabulary() {
    let domain = include_str!("../../kiana-domain/src/telemetry_separation.rs");
    let redaction = include_str!("../../kiana-domain/src/redaction.rs");
    let observability = include_str!("../../kiana-domain/src/observability.rs");

    for marker in REQUIRED_MARKERS {
        assert!(
            domain.contains(marker),
            "BQ-25 source marker missing: {marker}"
        );
    }
    for forbidden in FORBIDDEN {
        assert!(
            !domain.contains(forbidden),
            "BQ-25 authority widened: {forbidden}"
        );
    }

    // The redaction primitives this contract depends on are the existing ones, unchanged. If
    // `redact_text` or `scan_secret_sentinels` were renamed or dropped, this fails rather than
    // letting a second, weaker redaction system grow in beside it.
    for marker in [
        "pub fn redact_text(text: &str) -> String {",
        "pub fn scan_secret_sentinels(",
        "pub enum SecretScanChannel {",
        "pub enum RedactionSignal {",
        "pub struct RedactionProfile {",
    ] {
        assert!(
            redaction.contains(marker),
            "redaction vocabulary moved: {marker}"
        );
    }

    // The metric cardinality bound is the one the runtime guard already enforces, not a new
    // number, so a source claim cannot exceed what the runtime accepts.
    assert!(
        observability.contains("pub const MAX_METRIC_LABEL_VALUES: usize = 64;"),
        "BQ-25 must reuse the runtime metric label bound"
    );
    assert!(
        domain.contains("crate::MAX_METRIC_LABEL_VALUES"),
        "BQ-25 must read the runtime metric label bound, not a private copy"
    );
}

#[test]
fn bq25_telemetry_separation_is_not_a_second_redaction_system() {
    let domain = include_str!("../../kiana-domain/src/telemetry_separation.rs");

    // The contract decides; it does not redact. Every use of `redact_text` is a comparison against
    // the original string, which is what turns "already redacted" into a fact. If one of these
    // ever became an assignment, the module would be mutating caller data.
    for shape in [
        "if redact_text(text) != text {",
        "if redact_text(reference) != reference {",
        "if redact_text(value) != value {",
    ] {
        assert!(
            domain.contains(shape),
            "BQ-25 lost the compare-not-redact shape: {shape}"
        );
    }
    for assignment in [
        "= redact_text(",
        "redact_text(&mut",
        "redact_text(value).to_string()",
    ] {
        assert!(
            !domain.contains(assignment),
            "BQ-25 assigns a redacted value instead of comparing: {assignment}"
        );
    }

    // No redaction profile is constructed here: the contract consumes the guarantees adapters
    // already established and never mints one on a sink's behalf.
    for forbidden in [
        "RedactionProfile::new",
        "RedactionProfile::for_signal",
        "encode_bounded_value",
        "encode_bounded_text",
        "StreamingRedactor",
    ] {
        assert!(
            !domain.contains(forbidden),
            "BQ-25 grew a redaction system: {forbidden}"
        );
    }
}

#[test]
fn bq25_telemetry_separation_does_not_claim_runtime_telemetry_behaviour() {
    let domain = include_str!("../../kiana-domain/src/telemetry_separation.rs");

    // The module is a decision, so the words that would claim a shipped pipeline must not appear
    // as behaviour. These are honest refusals: the guard fails if the slice starts asserting an
    // exporter, a collector or a live backend without the fixtures to back it.
    for absent in [
        "exported to a collector",
        "exporter is configured",
        "in production this",
        "guarantees at runtime",
        "verified end to end",
    ] {
        assert!(
            !domain.contains(absent),
            "BQ-25 claims unproven behaviour: {absent}"
        );
    }

    // What the module *does* claim is stated as source-level, in the header comment, so a reader
    // is told the ceiling before reading a single rule.
    for required in [
        "read-only decision over adapter-reported channel facts",
        "does not emit a metric",
    ] {
        assert!(
            domain.contains(required),
            "BQ-25 header must state its ceiling: {required}"
        );
    }
}

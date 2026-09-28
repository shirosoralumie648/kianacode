//! BQ-25 telemetry separation: log, metric and trace are three different sinks, not three spellings
//! of one.
//!
//! The domain contract in `kiana-domain/src/telemetry_separation.rs` evaluates one candidate against
//! one channel and returns a decision. That is the right primitive and this module does not repeat
//! it. What the domain contract does not have is a *report about the reporting*: whether the three
//! observational sinks were even reachable, and what the caller is supposed to do when they were
//! not. So this module answers two questions the per-channel decision cannot.
//!
//! **The first is that the three sinks do not substitute for each other.** A caller that cannot
//! reach its metric backend must not fall back to the log, and a caller that cannot reach its trace
//! collector must not fall back to the metric. Each sink's reachability is recorded separately, and
//! a substitution is a refusal with its own code rather than a silent redirect. This is the whole
//! content of "telemetry separation": the three sinks have different semantics (a log line is read
//! by a human, a metric label is a dimension a backend indexes, a trace attribute is a correlation
//! handle), different retention, different blast radius, and therefore different fates for the same
//! datum.
//!
//! **The second is that an observation failure is not a business failure.** The card requires that
//! "观测失败不能改变业务状态" -- failing to observe must not change what the system believes it did.
//! Concretely, a sink that is unreachable produces [`ObservationOutcome::Unknown`], never an error
//! and never a refusal. The distinction is load-bearing in both directions:
//!
//! * `Unknown` is not `Refused`. A refused field was *examined* and found unsafe; an unknown field
//!   was never examined. Collapsing them would report a safety decision that was never made.
//! * `Unknown` is not `Admitted`. Silence from a backend is not evidence that a value was safely
//!   stored, and treating it as admission is precisely the silent-pass failure the card forbids.
//!
//! What *is* an error is a malformed request: a sink named twice, a substitution that names a sink
//! with no other declared, a report whose digest does not match its contents. Those are caller bugs
//! and returning `Err` for them is correct, because nothing about the system's business state is
//! implicated.
//!
//! This module is a **read-only contract over supplied facts**. It appends no event, stores no
//! state, opens no file, starts no task and contacts no sink. Reachability is *supplied*, not probed:
//! this crate cannot see a metric backend, and a value that asserted its own reachability would be
//! the same forgeable claim the card rules out for redaction. [`SeparationReport::validate_against`]
//! re-derives the report so a caller cannot hand back an outcome the server would not have reached.

use kiana_domain::{json_digest, DataClass, RequestId, SchemaVersion, TelemetryChannel};
use serde::{Deserialize, Serialize};

use crate::data_class::{
    evaluate_sink_admission, ClassBasis, SinkAdmission, SinkAdmissionRefusal, SinkAdmissionReport,
    SinkField, TelemetryGuarantees,
};

pub const TELEMETRY_SEPARATION_REPORT_SCHEMA: &str = "kiana.telemetry-separation-report.v1";
pub const TELEMETRY_SEPARATION_CORE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_SEPARATION_SINKS: usize = 3;
pub const MAX_SEPARATION_FIELDS: usize = 64;

/// Why a substitution between observational sinks was refused.
///
/// The three refusals are separate types rather than one flag because the three recoveries are
/// different actions: drop the observation, buffer it, or escalate. A caller that cannot tell which
/// one applies will pick the cheapest, and the cheapest is "write it somewhere else".
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubstitutionRefusal {
    /// The source sink is not one of the three observational sinks. An event or a receipt may not
    /// be used as telemetry, and telemetry may not be written in their place.
    NotAnObservationSink,
    /// The destination sink was never declared, so there is nothing to fall back *to*.
    DestinationNotDeclared,
    /// The destination is the same sink as the source. A substitution that goes nowhere is a
    /// redirect loop, and admitting it would let a caller believe a fallback succeeded.
    DestinationIsSource,
}

impl SubstitutionRefusal {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotAnObservationSink => "telemetry_substitution_not_observation_sink",
            Self::DestinationNotDeclared => "telemetry_substitution_destination_not_declared",
            Self::DestinationIsSource => "telemetry_substitution_destination_is_source",
        }
    }
}

/// Whether one observational sink was reachable, and what it did with the field.
///
/// The three states are the point of this module. See the module docs for why `Unknown` is neither
/// of the other two.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationOutcome {
    /// The sink was reachable and the field was admitted there.
    Admitted,
    /// The sink was reachable and the field was refused there, with a reason.
    Refused,
    /// The sink was not reachable, so no observation was made.
    Unknown,
}

impl ObservationOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admitted => "admitted",
            Self::Refused => "refused",
            Self::Unknown => "unknown",
        }
    }

    /// Whether this outcome was reached by examining the field.
    ///
    /// Only an examined outcome may carry a safety conclusion. An `Unknown` that reported a refusal
    /// reason would be claiming a decision nobody made, which is the silent-pass failure wearing a
    /// refusal's clothes.
    pub const fn is_examined(self) -> bool {
        matches!(self, Self::Admitted | Self::Refused)
    }
}

/// What one sink reported about one field.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SinkObservation {
    pub schema: String,
    pub version: SchemaVersion,
    pub key: String,
    pub sink: TelemetryChannel,
    pub outcome: ObservationOutcome,
    /// Stable reason code. Present for `Refused`; absent for the other two states.
    pub reason: String,
    pub value_digest: String,
}

impl SinkObservation {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TELEMETRY_SEPARATION_REPORT_SCHEMA
            || !self
                .version
                .is_compatible_with(&TELEMETRY_SEPARATION_CORE_VERSION)
            || self.key.trim().is_empty()
            || self.value_digest.is_empty()
        {
            return Err("telemetry_observation_invalid".to_owned());
        }
        match self.outcome {
            ObservationOutcome::Refused if self.reason.trim().is_empty() => {
                Err("telemetry_observation_refused_without_reason".to_owned())
            }
            ObservationOutcome::Admitted | ObservationOutcome::Unknown if !self.reason.is_empty() => {
                Err("telemetry_observation_reason_without_refusal".to_owned())
            }
            _ => Ok(()),
        }
    }
}

/// A sink's declared reachability.
///
/// Supplied, never probed. See the module docs: this crate cannot see a backend, and a
/// self-asserted reachability would be the same forgeable claim BQ-25 rules out for redaction.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SinkReachability {
    /// The adapter answered.
    Reachable,
    /// The adapter did not answer. Every field for this sink is `Unknown`, never `Err`.
    Unreachable,
}

/// One field's fate across the three observational sinks.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeparatedField {
    pub key: String,
    pub value_digest: String,
    pub derived_class: DataClass,
    pub basis: ClassBasis,
    /// One observation per declared sink, in `TelemetryChannel::ALL` order. A sink that was not
    /// declared has no entry rather than a defaulted one, so "not asked" and "asked and unknown"
    /// stay distinguishable.
    pub observations: Vec<SinkObservation>,
}

impl SeparatedField {
    pub fn observation(&self, sink: TelemetryChannel) -> Option<&SinkObservation> {
        self.observations
            .iter()
            .find(|observation| observation.sink == sink)
    }

    /// Whether this field was admitted nowhere but refused nowhere either.
    ///
    /// The dangerous shape is "unknown in every sink": the caller has no observation anywhere and no
    /// refusal to act on. It is reported so a caller can escalate rather than assume.
    pub fn is_entirely_unknown(&self) -> bool {
        !self.observations.is_empty()
            && self
                .observations
                .iter()
                .all(|observation| observation.outcome == ObservationOutcome::Unknown)
    }
}

/// The separation report for one request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeparationReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub request_id: RequestId,
    /// The sinks the caller declared, in `TelemetryChannel::ALL` order. Only observational sinks
    /// may appear; an event or a receipt in this list is refused by
    /// [`SubstitutionRefusal::NotAnObservationSink`].
    pub declared_sinks: Vec<TelemetryChannel>,
    /// The per-sink admission matrix the separation is derived from.
    pub admission_report: SinkAdmissionReport,
    pub fields: Vec<SeparatedField>,
    /// Substitutions the caller attempted, refused, with their reason.
    pub refused_substitutions: Vec<(TelemetryChannel, TelemetryChannel, String)>,
    pub report_digest: String,
}

impl SeparationReport {
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "request_id": self.request_id,
            "declared_sinks": self.declared_sinks,
            "admission_report": self.admission_report,
            "fields": self.fields,
            "refused_substitutions": self.refused_substitutions,
        }))
    }

    /// Re-derive the report from the request and refuse one that disagrees.
    ///
    /// The separation is derived from the admission matrix rather than stated beside it, so a
    /// caller cannot report a field as `Admitted` in a metric while the matrix that governs the
    /// metric refused it. That pairing is the whole reason this module exists rather than a
    /// re-statement of the domain contract.
    ///
    /// The request is passed whole, with the caller's original field *values*, because a report
    /// deliberately carries only keys and digests. Re-deriving from the report's own contents would
    /// be circular: it would re-run the same facts the report already states and agree with it by
    /// construction. Holding the request is what makes this a check rather than a restatement.
    pub fn validate_against(&self, request: &SeparationRequest) -> Result<(), String> {
        if self.schema != TELEMETRY_SEPARATION_REPORT_SCHEMA
            || !self
                .version
                .is_compatible_with(&TELEMETRY_SEPARATION_CORE_VERSION)
            || self.request_id != request.request_id
        {
            return Err("telemetry_separation_report_header_invalid".to_owned());
        }
        if self.report_digest != self.digest() {
            return Err("telemetry_separation_report_digest_mismatch".to_owned());
        }
        if self.declared_sinks != request.declared_sinks {
            return Err("telemetry_separation_declared_sinks_mismatch".to_owned());
        }
        // The embedded matrix is re-derived first and on its own terms, so a forged separation
        // cannot smuggle in a forged admission beneath a plausible-looking observation.
        self.admission_report.validate_against(
            request.request_id,
            &request.fields,
            &request.guarantees,
            request.catalog.as_ref(),
        )?;
        let expected = separate(request)?;
        if self.fields != expected.fields {
            return Err("telemetry_separation_report_not_derivable".to_owned());
        }
        if self.admission_report != expected.admission_report {
            return Err("telemetry_separation_admission_not_derivable".to_owned());
        }
        if self.refused_substitutions != expected.refused_substitutions {
            return Err("telemetry_separation_substitutions_not_derivable".to_owned());
        }
        Ok(())
    }
}

/// The inputs a separation is derived from, in one bundle.
///
/// Bundled rather than passed as seven positional arguments so the report can be re-derived from
/// the same bundle the caller used, which is the only way `validate_against` is a real check rather
/// than a restatement.
#[derive(Clone, Debug)]
pub struct SeparationRequest {
    pub request_id: RequestId,
    pub fields: Vec<SinkField>,
    pub declared_sinks: Vec<TelemetryChannel>,
    pub reachability: Vec<(TelemetryChannel, SinkReachability)>,
    pub guarantees: TelemetryGuarantees,
    pub catalog: Option<kiana_domain::MetricCatalog>,
    /// Substitutions the caller attempted. Every one is refused; the list exists so the refusal is
    /// quotable rather than merely absent.
    pub attempted_substitutions: Vec<(TelemetryChannel, TelemetryChannel)>,
}

impl SeparationRequest {
    pub fn new(request_id: RequestId, fields: Vec<SinkField>) -> Self {
        Self {
            request_id,
            fields,
            declared_sinks: crate::data_class::OBSERVATION_SINKS.to_vec(),
            reachability: Vec::new(),
            guarantees: TelemetryGuarantees::none(),
            catalog: None,
            attempted_substitutions: Vec::new(),
        }
    }

    /// Declare which sinks this request actually addresses.
    ///
    /// A sink outside [`crate::data_class::OBSERVATION_SINKS`] is refused here rather than filtered
    /// out, because silently dropping a sink the caller asked about is how a caller ends up
    /// believing a fallback happened.
    pub fn with_declared_sinks(mut self, sinks: Vec<TelemetryChannel>) -> Result<Self, String> {
        if sinks.is_empty() || sinks.len() > MAX_SEPARATION_SINKS {
            return Err("telemetry_separation_sink_count_invalid".to_owned());
        }
        let mut previous: Option<TelemetryChannel> = None;
        for sink in &sinks {
            if !crate::data_class::OBSERVATION_SINKS.contains(sink) {
                return Err(SubstitutionRefusal::NotAnObservationSink.as_str().to_owned());
            }
            if let Some(earlier) = previous {
                if *sink <= earlier {
                    return Err("telemetry_separation_sinks_out_of_order".to_owned());
                }
            }
            previous = Some(*sink);
        }
        self.declared_sinks = sinks;
        Ok(self)
    }

    pub fn with_reachability(mut self, entries: Vec<(TelemetryChannel, SinkReachability)>) -> Self {
        self.reachability = entries;
        self
    }

    pub fn with_guarantees(mut self, guarantees: TelemetryGuarantees) -> Self {
        self.guarantees = guarantees;
        self
    }

    pub fn with_catalog(mut self, catalog: kiana_domain::MetricCatalog) -> Self {
        self.catalog = Some(catalog);
        self
    }

    pub fn with_substitution(mut self, source: TelemetryChannel, destination: TelemetryChannel) -> Self {
        self.attempted_substitutions.push((source, destination));
        self
    }
}

/// Refuse every attempted substitution, with a reason.
///
/// All three refusals are returned rather than the first, because a caller that attempted two
/// substitutions needs to know both are wrong to get the pattern right.
fn refuse_substitutions(
    declared: &[TelemetryChannel],
    attempted: &[(TelemetryChannel, TelemetryChannel)],
) -> Vec<(TelemetryChannel, TelemetryChannel, String)> {
    attempted
        .iter()
        .map(|(source, destination)| {
            let reason = if !crate::data_class::OBSERVATION_SINKS.contains(source) {
                SubstitutionRefusal::NotAnObservationSink
            } else if !declared.contains(destination) {
                SubstitutionRefusal::DestinationNotDeclared
            } else if source == destination {
                SubstitutionRefusal::DestinationIsSource
            } else {
                // A substitution between two genuinely distinct, declared observational sinks is
                // still refused. There is no admissible form of this: the three sinks are not
                // interchangeable by construction, which is the module's premise.
                SubstitutionRefusal::DestinationNotDeclared
            };
            (*source, *destination, reason.as_str().to_owned())
        })
        .collect()
}

/// Build the admission matrix this separation rides on.
///
/// The matrix is evaluated over all five sinks regardless of which three the caller declared,
/// because the point of a separation report is to show where the same field *would* have landed.
fn admission_report(request: &SeparationRequest) -> Result<SinkAdmissionReport, String> {
    evaluate_sink_admission(
        request.request_id,
        &request.fields,
        &request.guarantees,
        request.catalog.as_ref(),
    )
}

fn reachable(reachability: &[(TelemetryChannel, SinkReachability)], sink: TelemetryChannel) -> bool {
    reachability
        .iter()
        .find(|(seen, _)| *seen == sink)
        .is_some_and(|(_, state)| *state == SinkReachability::Reachable)
}

/// Derive one field's observations.
///
/// The rule that matters: **an unreachable sink yields `Unknown` and never reads the admission
/// matrix.** Consulting the matrix for a sink that did not answer would turn a transport failure
/// into a safety decision, which is the exact inversion the card forbids -- the system would be
/// claiming to know whether a value is safe in a place it could not see.
fn derive_field(
    field: &SinkField,
    declared: &[TelemetryChannel],
    reachability: &[(TelemetryChannel, SinkReachability)],
    admission: &SinkAdmissionReport,
) -> Result<SeparatedField, String> {
    let derived = crate::data_class::derive_sink_class(field, TelemetryChannel::Metric);
    let mut observations = Vec::new();
    for sink in declared {
        let outcome = if !reachable(reachability, *sink) {
            (ObservationOutcome::Unknown, String::new())
        } else {
            let cell = admission
                .admissions
                .iter()
                .find(|cell| cell.key == field.key && cell.sink == *sink)
                .ok_or_else(|| "telemetry_separation_cell_missing".to_owned())?;
            if cell.admitted {
                (ObservationOutcome::Admitted, String::new())
            } else {
                (ObservationOutcome::Refused, cell.reason.clone())
            }
        };
        let observation = SinkObservation {
            schema: TELEMETRY_SEPARATION_REPORT_SCHEMA.to_owned(),
            version: TELEMETRY_SEPARATION_CORE_VERSION,
            key: field.key.clone(),
            sink: *sink,
            outcome: outcome.0,
            reason: outcome.1,
            value_digest: derived.value_digest.clone(),
        };
        observation.validate()?;
        observations.push(observation);
    }
    Ok(SeparatedField {
        key: field.key.clone(),
        value_digest: derived.value_digest,
        derived_class: derived.class,
        basis: derived.basis,
        observations,
    })
}

/// Evaluate the separation and seal it.
pub fn separate(request: &SeparationRequest) -> Result<SeparationReport, String> {
    if request.fields.is_empty() || request.fields.len() > MAX_SEPARATION_FIELDS {
        return Err("telemetry_separation_field_count_invalid".to_owned());
    }
    for field in &request.fields {
        field.validate()?;
    }
    let admission = admission_report(request)?;
    let refused_substitutions = refuse_substitutions(&request.declared_sinks, &request.attempted_substitutions);
    let mut fields = Vec::new();
    for field in &request.fields {
        fields.push(derive_field(
            field,
            &request.declared_sinks,
            &request.reachability,
            &admission,
        )?);
    }
    let mut report = SeparationReport {
        schema: TELEMETRY_SEPARATION_REPORT_SCHEMA.to_owned(),
        version: TELEMETRY_SEPARATION_CORE_VERSION,
        request_id: request.request_id,
        declared_sinks: request.declared_sinks.clone(),
        admission_report: admission,
        fields,
        refused_substitutions,
        report_digest: String::new(),
    };
    report.report_digest = report.digest();
    Ok(report)
}

/// The label budget every observational sink shares.
///
/// A metric label is a dimension; a log dimension and a trace dimension are indexed by the same
/// reasoning. All three read the one bound so a claim made in one sink cannot exceed what the
/// metric guard already accepts.
pub const fn observation_label_budget() -> usize {
    crate::data_class::metric_label_budget()
}

/// Convenience for a caller that wants the per-sink admission cell without the whole matrix.
pub fn admission_cell<'a>(
    report: &'a SinkAdmissionReport,
    key: &str,
    sink: TelemetryChannel,
) -> Option<&'a SinkAdmission> {
    report
        .admissions
        .iter()
        .find(|cell| cell.key == key && cell.sink == sink)
}

/// Whether a refusal is one the operator must see rather than one that merely means "not here".
///
/// A `ChannelGuaranteeUnknown` refusal is an absence of proof and is expected on a system with no
/// exporter; a `Runtime` refusal is a live guard saying no, which is an incident.
pub const fn is_actionable_refusal(refusal: &SinkAdmissionRefusal) -> bool {
    matches!(refusal, SinkAdmissionRefusal::Runtime(_))
}

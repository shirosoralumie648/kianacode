//! BQ-25 failure-first fixtures for DataClass derivation, the five-sink admission matrix and
//! telemetry separation.
//!
//! Every test before the last two names one item from the card's rejected-first column: an API key, a
//! prompt, a raw response, an invoice secret, a path or a high-cardinality identifier reaching a
//! metric; a caller bringing its own redaction conclusion; an observation failure being laundered
//! into a business outcome. Nothing here opens a sink, writes an event or contacts a backend: the
//! modules under test are read-only decisions over supplied facts, so these are contract fixtures
//! and not runtime evidence.
//!
//! The order of the file is the order of the argument. Each of the four classes the card names --
//! secrets, payloads, identifiers, and the caller's own conclusion -- is shown to be refused in the
//! sink that would actually be harmed, the metric label budget and the runtime guard are shown to be
//! the same bound, and only then does a digest and a low-cardinality label correlate across all five
//! sinks.

use kiana_core::{
    admit_field, admission_cell, derive_sink_class, evaluate_sink_admission, separate,
    ClassBasis, ClassClaimOrigin, DerivedClass, ObservationOutcome, SeparationReport,
    SeparationRequest, SinkAdmissionRefusal, SinkAdmissionReport, SinkField, SinkObservation,
    SinkReachability, SubstitutionRefusal, TelemetryGuarantees, MAX_SINK_ADMISSION_FIELDS,
};
use kiana_domain::{
    DataClass, MetricCatalog, MetricDefinition, RequestId, TelemetryChannel,
    TelemetryChannelGuarantee, TelemetryRefusal,
};

const ALL_SINKS_REACHABLE: [(TelemetryChannel, SinkReachability); 3] = [
    (TelemetryChannel::Log, SinkReachability::Reachable),
    (TelemetryChannel::Metric, SinkReachability::Reachable),
    (TelemetryChannel::Trace, SinkReachability::Reachable),
];

/// A catalog with one registered gauge, so the runtime guard has something real to check against.
fn catalog() -> MetricCatalog {
    let mut definition = MetricDefinition::gauge("kiana.bq25.probe_total", "1");
    definition.allowed_labels = vec!["status".to_owned()];
    MetricCatalog::new(vec![definition]).expect("BQ-25 catalog")
}

/// Every sink established. A guarantee the adapter never reported is a separate refusal, tested on
/// its own, so the rest of the file can isolate one defect at a time.
fn all_established() -> TelemetryGuarantees {
    TelemetryGuarantees::none()
        .established(TelemetryChannel::Event)
        .established(TelemetryChannel::Log)
        .established(TelemetryChannel::Trace)
        .established(TelemetryChannel::Metric)
        .established(TelemetryChannel::Receipt)
}

fn request() -> RequestId {
    RequestId::new()
}

// ---------------------------------------------------------------------------------------------
// 1. API key. Rejected on content, in every sink, including the ones that would "only" hold a log.
// ---------------------------------------------------------------------------------------------

#[test]
fn an_api_key_is_refused_in_every_sink_and_the_sentinel_never_appears() {
    let field = SinkField::text("provider_error", "request failed: api_key=sk-live-abcd1234efgh");
    for sink in TelemetryChannel::ALL {
        let cell = admit_field(
            &field,
            sink,
            Some(TelemetryChannelGuarantee::RedactedAtSink),
            Some(&catalog()),
        )
        .expect("BQ-25 admission");
        assert!(!cell.admitted, "api key admitted into {sink:?}");
        assert_eq!(
            cell.reason,
            TelemetryRefusal::UnredactedContent.as_str(),
            "api key refused in {sink:?} for the wrong reason"
        );
        assert!(
            !cell.value_digest.contains("sk-live-abcd1234efgh"),
            "the sentinel reached the digest input: {}",
            cell.value_digest
        );
    }
}

#[test]
fn an_invoice_credential_is_refused_in_the_receipt_sink_and_its_reference_is_not() {
    let secret = SinkField::reference("invoice_auth", "client_secret=inv-live-9911zzz");
    let cell = admit_field(
        &secret,
        TelemetryChannel::Receipt,
        Some(TelemetryChannelGuarantee::RedactedAtSink),
        None,
    )
    .expect("BQ-25 admission");
    assert!(!cell.admitted);
    assert_eq!(cell.reason, TelemetryRefusal::UnredactedContent.as_str());

    // The admissible alternative: an opaque reference, which carries no reversible content.
    let opaque = SinkField::reference("invoice_ref", "invoice:inv_2026_09_28_0001");
    let cell = admit_field(
        &opaque,
        TelemetryChannel::Receipt,
        Some(TelemetryChannelGuarantee::RedactedAtSink),
        None,
    )
    .expect("BQ-25 admission");
    assert!(cell.admitted, "an opaque invoice ref was refused: {}", cell.reason);
    assert_eq!(cell.basis, ClassBasis::OpaqueReference);
}

// ---------------------------------------------------------------------------------------------
// 2. Prompt and raw response. Rejected on shape, because no redaction makes them telemetry.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_prompt_is_refused_on_shape_in_every_sink_and_redaction_does_not_help() {
    let field = SinkField::text("prompt", "You are a helpful assistant. Please summarize the repo.");
    for sink in TelemetryChannel::ALL {
        let cell = admit_field(
            &field,
            sink,
            Some(TelemetryChannelGuarantee::RedactedAtSink),
            Some(&catalog()),
        )
        .expect("BQ-25 admission");
        assert!(!cell.admitted, "a prompt was admitted into {sink:?}");
        assert_eq!(cell.reason, TelemetryRefusal::PayloadShapeForbidden.as_str());
    }
    // A prompt has no sentinel shape, so the shared scanner cannot see it. The refusal has to come
    // from the key, or "the prompt was not redacted" would be the only defence and it is false.
    let derived = derive_sink_class(&field, TelemetryChannel::Log);
    assert_eq!(derived.basis, ClassBasis::PayloadShape);
    assert_ne!(derived.basis, ClassBasis::SecretContent);
}

#[test]
fn a_raw_provider_response_is_refused_in_every_channel() {
    let field = SinkField::text("raw_response", "{\"choices\":[{\"text\":\"...\"}]}");
    for sink in TelemetryChannel::ALL {
        let cell = admit_field(
            &field,
            sink,
            Some(TelemetryChannelGuarantee::RedactedAtSink),
            Some(&catalog()),
        )
        .expect("BQ-25 admission");
        assert!(!cell.admitted, "a raw response was admitted into {sink:?}");
        assert_eq!(cell.reason, TelemetryRefusal::PayloadShapeForbidden.as_str());
    }
}

#[test]
fn a_prompt_version_dimension_is_still_a_legal_label() {
    // The payload rule exempts labels, because `prompt_version` is a bounded dimension and refusing
    // it would push a producer toward `prompt_text` to work around the contract.
    let field = SinkField::label("prompt_version", "v3", 2);
    let cell = admit_field(
        &field,
        TelemetryChannel::Log,
        Some(TelemetryChannelGuarantee::RedactedAtSink),
        None,
    )
    .expect("BQ-25 admission");
    assert!(cell.admitted, "prompt_version was refused: {}", cell.reason);
    assert_eq!(cell.basis, ClassBasis::BoundedLabel);
}

// ---------------------------------------------------------------------------------------------
// 3. Identifier and path in a metric label. The card's first-listed metric rejection.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_run_id_is_refused_as_a_metric_label_and_admitted_as_an_event_field() {
    let field = SinkField::label("run_id", "01J8ZQ4T7F3K2M9N0P5R6S8V1W", 4_096);

    let metric = admit_field(
        &field,
        TelemetryChannel::Metric,
        Some(TelemetryChannelGuarantee::RedactedAtSink),
        Some(&catalog()),
    )
    .expect("BQ-25 admission");
    assert!(!metric.admitted, "a run_id became a metric dimension");
    assert_eq!(metric.reason, TelemetryRefusal::HighCardinalityLabel.as_str());

    // The same value, the same class derivation, a different sink, a different fate. This pairing
    // is the entire content of "telemetry separation".
    let event = admit_field(
        &field,
        TelemetryChannel::Event,
        Some(TelemetryChannelGuarantee::RedactedAtSink),
        None,
    )
    .expect("BQ-25 admission");
    assert!(event.admitted, "a run_id was refused as an event field: {}", event.reason);
    assert_eq!(event.derived_class, metric.derived_class);
    assert_eq!(event.basis, metric.basis);
}

#[test]
fn a_filesystem_path_is_refused_in_metric_and_trace_but_not_in_the_log() {
    let field = SinkField::text("file_path", "/home/user/project/src/secret_module.rs");
    for sink in [TelemetryChannel::Metric, TelemetryChannel::Trace] {
        let cell = admit_field(
            &field,
            sink,
            Some(TelemetryChannelGuarantee::RedactedAtSink),
            Some(&catalog()),
        )
        .expect("BQ-25 admission");
        assert!(!cell.admitted, "a path was admitted into {sink:?}");
    }
    let log = admit_field(
        &field,
        TelemetryChannel::Log,
        Some(TelemetryChannelGuarantee::RedactedAtSink),
        None,
    )
    .expect("BQ-25 admission");
    assert!(log.admitted, "a path was refused in the log: {}", log.reason);
}

#[test]
fn free_text_is_not_representable_as_a_metric_dimension() {
    // Not because the text is secret -- it is clean -- but because a metric label is a dimension and
    // a sentence is not one. This is the rule that catches a leak the sentinel scanner cannot see.
    let field = SinkField::text("outcome", "the operator approved the correction");
    let cell = admit_field(
        &field,
        TelemetryChannel::Metric,
        Some(TelemetryChannelGuarantee::RedactedAtSink),
        Some(&catalog()),
    )
    .expect("BQ-25 admission");
    assert!(!cell.admitted);
    assert_eq!(cell.reason, TelemetryRefusal::PayloadShapeForbidden.as_str());
}

// ---------------------------------------------------------------------------------------------
// 4. The caller's own conclusion. BQ-25 requires the class to be server-derived.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_caller_asserted_class_is_refused_rather_than_believed() {
    // The most dangerous input in the file: a prompt that the producer has helpfully labelled
    // `Public`. Believing it would let the whole ladder be bypassed with one field.
    let field = SinkField::text("prompt", "system: you are a helpful assistant")
        .with_asserted_class(DataClass::Public);
    for sink in TelemetryChannel::ALL {
        let cell = admit_field(
            &field,
            sink,
            Some(TelemetryChannelGuarantee::RedactedAtSink),
            Some(&catalog()),
        )
        .expect("BQ-25 admission");
        assert!(!cell.admitted, "a caller-asserted class was honoured in {sink:?}");
        assert_eq!(
            cell.reason,
            SinkAdmissionRefusal::CallerSuppliedClass.as_str(),
            "refused in {sink:?} for the wrong reason"
        );
    }
    assert_eq!(ClassClaimOrigin::CallerSupplied.as_str(), "caller_supplied");
}

#[test]
fn the_derived_class_is_the_systems_not_the_callers() {
    let field = SinkField::text("prompt", "you are a helpful assistant");
    let derived: DerivedClass = derive_sink_class(&field, TelemetryChannel::Log);
    assert_eq!(derived.class, DataClass::Restricted);
    assert_eq!(derived.basis, ClassBasis::PayloadShape);

    let label = SinkField::label("status", "ok", 3);
    assert_eq!(
        derive_sink_class(&label, TelemetryChannel::Metric).class,
        DataClass::Public
    );
    let reference = SinkField::reference("trace_ref", "trace:01J8ZQ");
    assert_eq!(
        derive_sink_class(&reference, TelemetryChannel::Metric).basis,
        ClassBasis::OpaqueReference
    );
}

// ---------------------------------------------------------------------------------------------
// 5. The metric label budget, and the runtime guard that enforces it.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_label_over_the_budget_is_refused_and_the_budget_is_the_guards_own() {
    let budget = kiana_core::metric_label_budget();
    assert_eq!(budget, kiana_domain::MAX_METRIC_LABEL_VALUES);

    let over = SinkField::label("status", "ok", budget as u32 + 1);
    let cell = admit_field(
        &over,
        TelemetryChannel::Metric,
        Some(TelemetryChannelGuarantee::RedactedAtSink),
        Some(&catalog()),
    )
    .expect("BQ-25 admission");
    assert!(!cell.admitted);
    assert_eq!(cell.reason, TelemetryRefusal::LabelCardinalityExceeded.as_str());

    // Exactly at the budget is admitted. An off-by-one here would silently drop a legitimate
    // dimension, which is how a cardinality guard starts getting disabled.
    let at = SinkField::label("status", "ok", budget as u32);
    let cell = admit_field(
        &at,
        TelemetryChannel::Metric,
        Some(TelemetryChannelGuarantee::RedactedAtSink),
        Some(&catalog()),
    )
    .expect("BQ-25 admission");
    assert!(cell.admitted, "a label exactly at the budget was refused: {}", cell.reason);
}

#[test]
fn a_metric_decision_with_no_catalog_is_refused_rather_than_assumed_safe() {
    // The runtime guard cannot run without a catalog, and "we could not check" is not "it is fine".
    let field = SinkField::label("status", "ok", 1);
    let cell = admit_field(
        &field,
        TelemetryChannel::Metric,
        Some(TelemetryChannelGuarantee::RedactedAtSink),
        None,
    )
    .expect("BQ-25 admission");
    assert!(!cell.admitted);
    assert_eq!(
        cell.reason,
        SinkAdmissionRefusal::MetricCatalogAbsent.as_str()
    );
}

#[test]
fn the_live_runtime_guard_refuses_a_label_the_source_rules_would_admit() {
    // `secret_ref` is not a payload key and not a high-cardinality identifier by the domain rules, so
    // the source-level rules admit it. The guard's own forbidden-label list does not. This is the
    // two-lists gap the domain baseline records, closed from the only crate that can close it.
    let field = SinkField::label("secret_ref", "vault://capability", 2);
    let mut definition = MetricDefinition::gauge("kiana.bq25.leak_total", "1");
    definition.allowed_labels = vec!["secret_ref".to_owned()];
    let leaky_catalog = MetricCatalog::new(vec![definition]).expect("BQ-25 catalog");

    let cell = admit_field(
        &field,
        TelemetryChannel::Metric,
        Some(TelemetryChannelGuarantee::RedactedAtSink),
        Some(&leaky_catalog),
    )
    .expect("BQ-25 admission");
    assert!(!cell.admitted, "a secret-bearing label reached a metric");
    match &cell.reason[..] {
        reason if reason
            == SinkAdmissionRefusal::Runtime(
                kiana_core::TelemetryRuntimeRefusal::LabelForbidden("secret_ref".to_owned()),
            )
            .as_str() => {}
        other => panic!("expected the runtime guard to refuse, got {other}"),
    }
    assert!(kiana_core::is_actionable_refusal(&SinkAdmissionRefusal::Runtime(
        kiana_core::TelemetryRuntimeRefusal::LabelForbidden("secret_ref".to_owned()),
    )));
}

// ---------------------------------------------------------------------------------------------
// 6. Guarantees. Silence is not proof.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_sink_that_never_reported_its_guarantee_is_refused() {
    let field = SinkField::reference("trace_ref", "trace:01J8ZQ");
    let cell = admit_field(
        &field,
        TelemetryChannel::Trace,
        None,
        None,
    )
    .expect("BQ-25 admission");
    assert!(!cell.admitted, "an unreported channel was treated as safe");
    assert_eq!(cell.reason, TelemetryRefusal::ChannelGuaranteeUnknown.as_str());
}

#[test]
fn an_explicitly_unverified_exporter_is_refused_even_for_a_clean_digest() {
    let field = SinkField::reference("trace_ref", "trace:01J8ZQ");
    let cell = admit_field(
        &field,
        TelemetryChannel::Trace,
        Some(TelemetryChannelGuarantee::Unverified),
        None,
    )
    .expect("BQ-25 admission");
    assert!(!cell.admitted, "an unverified exporter was treated as redacting at the sink");
    assert_eq!(cell.reason, TelemetryRefusal::ChannelGuaranteeUnknown.as_str());
}

// ---------------------------------------------------------------------------------------------
// 7. The export ceiling, which is about the destination rather than the value.
// ---------------------------------------------------------------------------------------------

#[test]
fn an_export_ceiling_above_the_sink_floor_is_refused() {
    // A Confidential downstream cannot be shown safe from a Public-class producer, even though the
    // value itself is clean. The floor is the weaker defence behind the class ceiling.
    let field = SinkField::text("operator_note", "reviewed by the on-call engineer")
        .with_export_ceiling(DataClass::Confidential);
    let cell = admit_field(
        &field,
        TelemetryChannel::Log,
        Some(TelemetryChannelGuarantee::RedactedAtSink),
        None,
    )
    .expect("BQ-25 admission");
    assert!(!cell.admitted);
    assert_eq!(cell.reason, TelemetryRefusal::ExportClassCeilingTooHigh.as_str());
}

// ---------------------------------------------------------------------------------------------
// 8. The matrix and its re-derivation.
// ---------------------------------------------------------------------------------------------

#[test]
fn the_report_cannot_publish_its_own_outcome() {
    let fields = vec![SinkField::label("status", "ok", 2)];
    let guarantees = all_established();
    let catalog = catalog();
    let report = evaluate_sink_admission(request(), &fields, &guarantees, Some(&catalog))
        .expect("BQ-25 report");

    // A hand-edited "admitted" cell for a field the server refuses.
    let mut forged = report.clone();
    let cell = forged
        .admissions
        .iter_mut()
        .find(|cell| cell.sink == TelemetryChannel::Metric)
        .expect("metric cell");
    cell.admitted = true;
    cell.reason = String::new();
    assert_eq!(
        forged
            .validate_against(request(), &fields, &guarantees, Some(&catalog))
            .expect_err("a forged report was accepted"),
        "sink_admission_report_digest_mismatch"
    );

    // Re-sealed but still forged: the digest is consistent, the derivation is not.
    let mut resealed = forged;
    resealed.report_digest = resealed.digest();
    assert_eq!(
        resealed
            .validate_against(request(), &fields, &guarantees, Some(&catalog))
            .expect_err("a re-sealed forged report was accepted"),
        "sink_admission_report_not_derivable"
    );
}

#[test]
fn an_admitted_cell_may_not_carry_a_reason_and_a_refused_cell_may_not_lack_one() {
    let fields = vec![SinkField::label("status", "ok", 2)];
    let guarantees = all_established();
    let report =
        evaluate_sink_admission(request(), &fields, &guarantees, Some(&catalog())).expect("report");
    for cell in &report.admissions {
        cell.validate().expect("a sealed cell failed its own validation");
    }

    let mut contradictory = report.clone();
    let cell = contradictory.admissions.first_mut().expect("cell");
    cell.reason = "unredacted_content".to_owned();
    assert_eq!(
        cell.validate().expect_err("a contradictory cell validated"),
        "sink_admission_reason_mismatch"
    );
}

#[test]
fn an_empty_or_oversized_field_set_is_refused() {
    let guarantees = all_established();
    assert_eq!(
        evaluate_sink_admission(request(), &[], &guarantees, None)
            .expect_err("an empty field set was accepted"),
        "sink_admission_field_count_invalid"
    );
    let fields: Vec<SinkField> = (0..=MAX_SINK_ADMISSION_FIELDS)
        .map(|index| SinkField::reference("ref", format!("ref:{index}")))
        .collect();
    assert_eq!(
        evaluate_sink_admission(request(), &fields, &guarantees, None)
            .expect_err("an oversized field set was accepted"),
        "sink_admission_field_count_invalid"
    );
}

// ---------------------------------------------------------------------------------------------
// 9. Telemetry separation. The three observational sinks, and the Unknown that is neither of the
//    other two outcomes.
// ---------------------------------------------------------------------------------------------

#[test]
fn an_unreachable_sink_is_unknown_and_never_a_business_outcome() {
    // The load-bearing test of the module. The metric backend is down. The field must not be
    // reported as refused (nobody examined it) and must not be reported as admitted (silence is
    // not evidence). It is unknown, and the log -- a different sink -- is unaffected.
    let fields = vec![SinkField::label("status", "ok", 2)];
    let request = SeparationRequest::new(request(), fields.clone())
        .with_reachability(vec![
            (TelemetryChannel::Log, SinkReachability::Reachable),
            (TelemetryChannel::Metric, SinkReachability::Unreachable),
            (TelemetryChannel::Trace, SinkReachability::Reachable),
        ])
        .with_guarantees(all_established())
        .with_catalog(catalog());
    let report = separate(&request).expect("BQ-25 separation");

    let field = &report.fields[0];
    assert_eq!(
        field.observation(TelemetryChannel::Metric).map(|o| o.outcome),
        Some(ObservationOutcome::Unknown)
    );
    assert_eq!(
        field.observation(TelemetryChannel::Log).map(|o| o.outcome),
        Some(ObservationOutcome::Admitted)
    );
    // An unknown outcome carries no reason: a refusal reason on an unexamined field would be a
    // safety decision that was never made.
    let unknown = field.observation(TelemetryChannel::Metric).expect("metric");
    assert!(unknown.reason.is_empty());
    unknown.validate().expect("unknown observation");
    assert!(!unknown.outcome.is_examined());
}

#[test]
fn an_unreachable_sink_never_manufactures_a_safety_decision() {
    // The inversion this guards: if an unreachable sink consulted the admission matrix, a transport
    // failure would be reported as a refusal -- the system claiming to know whether a value is safe
    // in a place it could not see.
    let fields = vec![SinkField::text("prompt", "you are a helpful assistant")];
    let request = SeparationRequest::new(request(), fields)
        .with_reachability(vec![
            (TelemetryChannel::Log, SinkReachability::Reachable),
            (TelemetryChannel::Metric, SinkReachability::Unreachable),
            (TelemetryChannel::Trace, SinkReachability::Unreachable),
        ])
        .with_guarantees(all_established())
        .with_catalog(catalog());
    let report = separate(&request).expect("BQ-25 separation");

    let field = &report.fields[0];
    // Unreachable: unknown, with no reason, even though the matrix would have refused it.
    assert_eq!(
        field.observation(TelemetryChannel::Metric).map(|o| o.outcome),
        Some(ObservationOutcome::Unknown)
    );
    // Reachable: refused, with the matrix's reason.
    assert_eq!(
        field.observation(TelemetryChannel::Log).map(|o| o.outcome),
        Some(ObservationOutcome::Refused)
    );
    assert_eq!(
        field.observation(TelemetryChannel::Log).map(|o| o.reason.as_str()),
        Some(TelemetryRefusal::PayloadShapeForbidden.as_str())
    );
    assert!(!field.is_entirely_unknown());
}

#[test]
fn a_field_unknown_in_every_sink_is_reported_as_such() {
    // "No observation anywhere and no refusal to act on" is the shape that gets mistaken for
    // success, so it is named explicitly rather than left to be inferred.
    let fields = vec![SinkField::reference("trace_ref", "trace:01J8ZQ")];
    let request = SeparationRequest::new(request(), fields)
        .with_reachability(vec![
            (TelemetryChannel::Log, SinkReachability::Unreachable),
            (TelemetryChannel::Metric, SinkReachability::Unreachable),
            (TelemetryChannel::Trace, SinkReachability::Unreachable),
        ])
        .with_guarantees(all_established());
    let report = separate(&request).expect("BQ-25 separation");
    assert!(report.fields[0].is_entirely_unknown());
}

#[test]
fn a_substitution_between_sinks_is_refused_with_a_reason() {
    // The metric backend is down, so the caller tries to write the value to the log instead. The
    // refusal is recorded rather than silently dropped, because a caller that believes its fallback
    // worked will keep retrying it.
    let fields = vec![SinkField::label("status", "ok", 2)];
    let request = SeparationRequest::new(request(), fields)
        .with_reachability(ALL_SINKS_REACHABLE.to_vec())
        .with_guarantees(all_established())
        .with_catalog(catalog())
        .with_substitution(TelemetryChannel::Metric, TelemetryChannel::Log);
    let report = separate(&request).expect("BQ-25 separation");
    assert_eq!(
        report.refused_substitutions,
        vec![(
            TelemetryChannel::Metric,
            TelemetryChannel::Log,
            SubstitutionRefusal::DestinationNotDeclared.as_str().to_owned(),
        )]
    );
}

#[test]
fn a_substitution_out_of_an_event_or_receipt_is_refused_by_name() {
    // An event is a committed fact. It is not a telemetry sink and may not be used as one, so this
    // is refused on the source rather than on the destination.
    let request = SeparationRequest::new(request(), vec![SinkField::reference("r", "ref:1")])
        .with_substitution(TelemetryChannel::Event, TelemetryChannel::Log);
    let report = separate(&request).expect("BQ-25 separation");
    assert_eq!(
        report.refused_substitutions[0].2,
        SubstitutionRefusal::NotAnObservationSink.as_str()
    );
}

#[test]
fn declaring_a_record_sink_as_observational_is_refused() {
    // Silently filtering the sink out would leave the caller believing it had a fallback.
    let request = SeparationRequest::new(request(), vec![SinkField::reference("r", "ref:1")])
        .with_declared_sinks(vec![
            TelemetryChannel::Log,
            TelemetryChannel::Event,
            TelemetryChannel::Trace,
        ])
        .expect_err("an event sink was accepted as observational");
    assert_eq!(request, SubstitutionRefusal::NotAnObservationSink.as_str());
}

#[test]
fn a_separation_report_cannot_publish_its_own_outcome() {
    let fields = vec![SinkField::label("status", "ok", 2)];
    let request = SeparationRequest::new(request(), fields)
        .with_reachability(vec![
            (TelemetryChannel::Log, SinkReachability::Reachable),
            (TelemetryChannel::Metric, SinkReachability::Unreachable),
            (TelemetryChannel::Trace, SinkReachability::Reachable),
        ])
        .with_guarantees(all_established())
        .with_catalog(catalog());
    let report = separate(&request).expect("BQ-25 separation");
    report.validate_against(&request).expect("a sealed report failed re-derivation");

    // A forged "admitted" for the sink that was down, re-sealed so the digest agrees.
    let mut forged = report.clone();
    let observation = forged.fields[0]
        .observations
        .iter_mut()
        .find(|observation| observation.sink == TelemetryChannel::Metric)
        .expect("metric observation");
    observation.outcome = ObservationOutcome::Admitted;
    forged.report_digest = forged.digest();
    assert_eq!(
        forged
            .validate_against(&request)
            .expect_err("a forged separation was accepted"),
        "telemetry_separation_report_not_derivable"
    );
}

#[test]
fn a_refused_observation_may_not_omit_its_reason() {
    let observation = SinkObservation {
        schema: kiana_core::TELEMETRY_SEPARATION_REPORT_SCHEMA.to_owned(),
        version: kiana_core::TELEMETRY_SEPARATION_CORE_VERSION,
        key: "status".to_owned(),
        sink: TelemetryChannel::Metric,
        outcome: ObservationOutcome::Refused,
        reason: String::new(),
        value_digest: "sha256:0".to_owned(),
    };
    assert_eq!(
        observation.validate().expect_err("a reasonless refusal validated"),
        "telemetry_observation_refused_without_reason"
    );
}

// ---------------------------------------------------------------------------------------------
// 10. The success paths, last.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_digest_and_a_low_cardinality_label_correlate_across_all_five_sinks() {
    let fields = vec![
        SinkField::reference("cost_digest", "sha256:1111111111111111111111111111111111111111111111111111111111111111"),
        SinkField::label("status", "ok", 3),
    ];
    let guarantees = all_established();
    let catalog = catalog();
    let report: SinkAdmissionReport =
        evaluate_sink_admission(request(), &fields, &guarantees, Some(&catalog))
            .expect("BQ-25 report");

    assert!(report.refused_keys.is_empty(), "unexpected refusals: {:?}", report.refused_keys);
    assert_eq!(report.admissions.len(), 2 * TelemetryChannel::ALL.len());
    for cell in &report.admissions {
        assert!(cell.admitted, "{} into {} refused: {}", cell.key, cell.sink.as_str(), cell.reason);
    }

    // The same value digest on both sides of the event/metric boundary. This is what "digest/ref and
    // low-cardinality labels correlate" means operationally: a number in a metric can be joined back
    // to a fact in the event log without the value ever having been readable in the metric.
    let event = admission_cell(&report, "cost_digest", TelemetryChannel::Event).expect("event cell");
    let metric = admission_cell(&report, "cost_digest", TelemetryChannel::Metric).expect("metric cell");
    assert_eq!(event.value_digest, metric.value_digest);
    assert_eq!(event.derived_class, DataClass::Internal);
    report
        .validate_against(request(), &fields, &guarantees, Some(&catalog))
        .expect("a sealed report failed re-derivation");
}

#[test]
fn a_separation_over_reachable_sinks_reports_one_outcome_per_sink() {
    let fields = vec![SinkField::label("status", "ok", 2)];
    let request = SeparationRequest::new(request(), fields)
        .with_reachability(ALL_SINKS_REACHABLE.to_vec())
        .with_guarantees(all_established())
        .with_catalog(catalog());
    let report: SeparationReport = separate(&request).expect("BQ-25 separation");

    assert_eq!(report.declared_sinks.len(), 3);
    let field = &report.fields[0];
    assert_eq!(field.observations.len(), 3);
    for observation in &field.observations {
        assert_eq!(observation.outcome, ObservationOutcome::Admitted);
        assert!(observation.outcome.is_examined());
    }
    report.validate_against(&request).expect("re-derivation");
    // The three sinks share one dimension bound, read from the metric guard's own constant.
    assert_eq!(
        kiana_core::observation_label_budget(),
        kiana_domain::MAX_METRIC_LABEL_VALUES
    );
}

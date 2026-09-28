//! BQ-25 source guard: the same value has a different fate in the event log, a log line, a metric
//! label, a trace attribute and a receipt, and the class that decides which is derived by the
//! server rather than asserted by the caller.
//!
//! These are source-text assertions, not behavioural ones. The absence check is the load-bearing
//! half: both modules claim to be read-only contracts over supplied values, and the only thing that
//! keeps that claim honest is that neither file can reach a file, a process, a socket, a clock or
//! the event log.

#[test]
fn bq25_data_class_pins_derivation_and_the_refusals() {
    let source = include_str!("../src/data_class.rs");

    // The derivation is server-side by construction. `CallerSupplied` exists to be refused, which is
    // only meaningful if the variant is still present -- deleting it would quietly turn a refusal
    // into an accept.
    for marker in [
        "ClassClaimOrigin",
        "CallerSupplied",
        "ClassBasis",
        "DerivedClass",
        "derive_sink_class",
        "SinkAdmissionRefusal",
        "CallerSuppliedClass",
        "MetricCatalogAbsent",
        "LabelForbidden",
        "LabelValueLimit",
        "SeriesLimit",
        "HighCardinalityIdentifier",
        "BoundedLabel",
        "OpaqueReference",
        "SecretContent",
        "PayloadShape",
        "redact_text",
        "scan_secret_sentinels",
    ] {
        assert!(source.contains(marker), "BQ-25 data_class lost {marker}");
    }

    // Cardinality is a budget, not a style preference, and both the metric and the observation
    // side read the same bound so a claim in one sink cannot exceed the other.
    for marker in [
        "metric_label_budget",
        "metric_series_budget",
        "MAX_METRIC_LABEL_VALUES",
        "MAX_METRIC_SERIES",
        "MAX_SINK_FIELD_KEY",
        "MAX_SINK_FIELD_VALUE",
        "MAX_SINK_ADMISSION_FIELDS",
    ] {
        assert!(source.contains(marker), "BQ-25 data_class lost {marker}");
    }

    // The three sinks where the separation actually bites. Event and receipt are not in this list
    // on purpose: they are governed by the append-only schemas, not by the telemetry contract.
    for marker in [
        "OBSERVATION_SINKS",
        "TelemetryChannel::Log",
        "TelemetryChannel::Metric",
        "TelemetryChannel::Trace",
    ] {
        assert!(source.contains(marker), "BQ-25 data_class lost {marker}");
    }
}

#[test]
fn bq25_separation_keeps_log_metric_and_trace_distinct() {
    let source = include_str!("../src/telemetry_separation.rs");

    for marker in [
        "SeparationRequest",
        "SeparationReport",
        "SeparatedField",
        "SinkObservation",
        "SinkReachability",
        "ObservationOutcome",
        "SubstitutionRefusal",
        "NotAnObservationSink",
        "DestinationNotDeclared",
        "DestinationIsSource",
        "observation_label_budget",
        "admission_cell",
        "is_actionable_refusal",
        "Unknown",
    ] {
        assert!(source.contains(marker), "BQ-25 telemetry_separation lost {marker}");
    }

    // An observation that could not be made stays Unknown. Without this the three sinks would
    // collapse into "whatever the log happened to have", which is the failure the card names.
    assert!(
        source.contains("ObservationOutcome::Unknown"),
        "BQ-25 must keep an unobservable sink Unknown rather than defaulting it"
    );
}

#[test]
fn bq25_observation_failure_does_not_change_business_state() {
    let data_class = include_str!("../src/data_class.rs");
    let separation = include_str!("../src/telemetry_separation.rs");

    // `is_actionable_refusal` is what keeps a telemetry refusal from turning into a business
    // error. If it disappears, every metric budget overflow would start failing the operation
    // that produced it, which is exactly "observability failure changes business state".
    assert!(
        separation.contains("is_actionable_refusal"),
        "BQ-25 lost the actionable/non-actionable refusal split"
    );
    // A refused field is dropped from the sink, not promoted into an error the caller must handle.
    assert!(
        data_class.contains("SinkAdmissionRefusal"),
        "BQ-25 lost the per-field admission refusal"
    );
}

#[test]
fn bq25_modules_stay_read_only_contracts() {
    for (name, source) in [
        ("data_class", include_str!("../src/data_class.rs")),
        ("telemetry_separation", include_str!("../src/telemetry_separation.rs")),
    ] {
        for forbidden in [
            "std::fs",
            "File::",
            "Command::",
            "std::process",
            "TcpStream",
            "reqwest",
            "tokio",
            "spawn",
            "thread::sleep",
            "SystemTime",
            "Instant::now",
            "EventStore",
            "append_event",
        ] {
            assert!(
                !source.contains(forbidden),
                "BQ-25 {name} gained a side-effect token: {forbidden}"
            );
        }
    }
}

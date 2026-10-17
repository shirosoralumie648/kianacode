use kiana_core::{
    exportable_status, foreign_parent_link, otlp_export_digest, otlp_export_json,
    ForeignTraceContext, LocalTraceExporter, TraceExportConfig, TraceExportDisposition,
    TraceExportError, TraceSamplingDecision, TraceSamplingReason, MAX_FOREIGN_ANNOTATIONS,
    MAX_FOREIGN_CONTEXT_BYTES, MAX_FOREIGN_KEY_BYTES, MAX_FOREIGN_VALUE_BYTES,
    OTLP_MAPPING_LIMITATIONS, OTLP_MAPPING_SCHEMA,
};
use kiana_domain::{
    json_digest, EventId, ExecutionStatus, RedactionProfile, RedactionSignal, RunId, RunReceipt,
    SessionId, SpanLinkKind, TraceStatus, TraceSummary,
};
use serde_json::json;
fn summary() -> TraceSummary {
    TraceSummary::new(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        Some("bbbbbbbbbbbbbbbb".to_owned()),
        TraceStatus::Ok,
        1,
        1,
        1,
        vec![EventId::new()],
    )
    .unwrap()
}

fn parent() -> String {
    format!("00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-cccccccccccccccc-01")
}

#[test]
fn invalid_foreign_parent_is_rejected_and_never_changes_sampling_or_authority() {
    let link = foreign_parent_link(&parent()).unwrap();
    assert_eq!(link.relationship, SpanLinkKind::ForeignParent);
    assert!(matches!(
        foreign_parent_link("00-00000000000000000000000000000000-cccccccccccccccc-01"),
        Err(TraceExportError::ParentInvalid(_))
    ));

    let exporter = LocalTraceExporter::new(TraceExportConfig {
        enabled: true,
        sampled: true,
        capacity: 2,
    })
    .unwrap();
    assert!(matches!(
        exporter.export_summary(&summary(), Some("not-a-traceparent"), true),
        Err(TraceExportError::ParentInvalid(_))
    ));
    assert!(exporter.records().is_empty());
}

#[test]
fn sampled_out_or_disabled_exporter_has_no_records() {
    let exporter = LocalTraceExporter::noop();
    assert_eq!(
        exporter
            .export_summary(&summary(), Some(&parent()), true)
            .unwrap(),
        TraceExportDisposition::SampledOut
    );
    assert!(exporter.records().is_empty());

    let exporter = LocalTraceExporter::new(TraceExportConfig {
        enabled: true,
        sampled: true,
        capacity: 2,
    })
    .unwrap();
    assert_eq!(
        exporter
            .export_summary(&summary(), Some(&parent()), false)
            .unwrap(),
        TraceExportDisposition::SampledOut
    );
    assert!(exporter.records().is_empty());
}

#[test]
fn local_export_is_bounded_digest_bound_and_jsonl_secret_free() {
    let exporter = LocalTraceExporter::new(TraceExportConfig {
        enabled: true,
        sampled: true,
        capacity: 1,
    })
    .unwrap();
    let mut trace = summary();
    trace
        .attributes
        .insert("authorization".to_owned(), "Bearer raw-secret".to_owned());
    trace
        .attributes
        .insert("component".to_owned(), "core".to_owned());
    trace.summary_digest = trace.digest();
    let receipt = match exporter
        .export_summary(&trace, Some(&parent()), true)
        .unwrap()
    {
        TraceExportDisposition::Exported(receipt) => receipt,
        TraceExportDisposition::SampledOut => panic!("enabled exporter should export"),
    };
    assert_eq!(receipt.sequence, 1);
    assert_eq!(exporter.records().len(), 1);
    let jsonl = exporter.jsonl().unwrap();
    assert!(jsonl.ends_with('\n'));
    assert!(jsonl.contains("\"component\":\"core\""));
    assert!(!jsonl.contains("raw-secret"));
    assert!(!jsonl.contains("authorization"));
    assert_eq!(exporter.flush(), 1);
    assert_eq!(exporter.shutdown(), 1);
    assert_eq!(
        exporter.export_summary(&summary(), Some(&parent()), true),
        Err(TraceExportError::Closed)
    );
    assert_eq!(exporter.reopen(), 2);
    assert!(matches!(
        exporter.export_summary(&summary(), Some(&parent()), true),
        Err(TraceExportError::CapacityExceeded)
    ));
}

#[test]
fn exporter_rejects_invalid_trace_summary_and_preserves_record_capacity() {
    let exporter = LocalTraceExporter::new(TraceExportConfig {
        enabled: true,
        sampled: true,
        capacity: 1,
    })
    .unwrap();
    let mut invalid = summary();
    invalid.trace_id = "trace-not-w3c".to_owned();
    assert!(matches!(
        exporter.export_summary(&invalid, Some(&parent()), true),
        Err(TraceExportError::SummaryInvalid(_))
    ));
    assert!(exporter.records().is_empty());
}

fn enabled_exporter() -> LocalTraceExporter {
    LocalTraceExporter::new(TraceExportConfig {
        enabled: true,
        sampled: true,
        capacity: 1,
    })
    .unwrap()
}

#[test]
fn oa03_trace_export_rejects_secrets_in_allowed_attribute_values() {
    for key in ["component", "provider_id"] {
        for value in [
            "Authorization: Bearer oa03-allowed-value-sentinel",
            "api_key=oa03-allowed-value-sentinel",
        ] {
            let exporter = enabled_exporter();
            let mut trace = summary();
            trace.attributes.insert(key.to_owned(), value.to_owned());
            trace.summary_digest = trace.digest();
            trace.validate().unwrap();
            let original = trace.clone();

            let error = exporter.export_summary(&trace, None, true).unwrap_err();
            assert!(matches!(error, TraceExportError::RecordInvalid(_)));
            assert!(!error.to_string().contains("oa03-allowed-value-sentinel"));
            assert_eq!(trace, original, "export must not rewrite the sealed source");
            assert!(exporter.records().is_empty());
            assert_eq!(exporter.jsonl().unwrap(), "");

            // A refused signal consumes neither a record slot nor a sequence.
            let receipt = exporter.export_summary(&summary(), None, true).unwrap();
            assert!(
                matches!(receipt, TraceExportDisposition::Exported(receipt) if receipt.sequence == 1)
            );
        }
    }
}

#[test]
fn oa03_trace_export_rejects_nul_without_publishing_or_rewriting_source() {
    let exporter = enabled_exporter();
    let mut trace = summary();
    trace
        .attributes
        .insert("component".to_owned(), "core\0private".to_owned());
    trace.summary_digest = trace.digest();
    trace.validate().unwrap();
    let original = trace.clone();

    let error = exporter.export_summary(&trace, None, true).unwrap_err();
    assert!(matches!(error, TraceExportError::RecordInvalid(_)));
    assert!(!error.to_string().contains("private"));
    assert_eq!(trace, original);
    assert!(exporter.records().is_empty());
    assert_eq!(exporter.jsonl().unwrap(), "");
}

#[test]
fn oa03_trace_export_preserves_valid_unicode_at_attribute_size_boundary() {
    let exporter = enabled_exporter();
    let mut trace = summary();
    // Four UTF-8 bytes per character: exactly the existing 256-byte limit.
    let value = "🦀".repeat(kiana_domain::MAX_ATTRIBUTE_VALUE_BYTES / 4);
    trace
        .attributes
        .insert("component".to_owned(), value.clone());
    trace
        .attributes
        .insert("provider_id".to_owned(), "local-fixture".to_owned());
    trace.summary_digest = trace.digest();
    let original = trace.clone();

    let receipt = exporter.export_summary(&trace, None, true).unwrap();
    assert!(matches!(receipt, TraceExportDisposition::Exported(receipt) if receipt.sequence == 1));
    assert_eq!(trace, original);
    let records = exporter.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].attributes, trace.attributes);
    assert_eq!(records[0].export_digest, records[0].digest());
    let serialized = exporter.jsonl().unwrap();
    let record: kiana_domain::TraceExportSpan = serde_json::from_str(serialized.trim()).unwrap();
    record.validate().unwrap();
    assert_eq!(record.attributes.get("component"), Some(&value));
}

#[test]
fn oa03_trace_export_rejects_oversize_allowed_attribute_without_consuming_capacity() {
    let exporter = enabled_exporter();
    let mut trace = summary();
    trace.attributes.insert(
        "component".to_owned(),
        "x".repeat(kiana_domain::MAX_ATTRIBUTE_VALUE_BYTES + 1),
    );
    trace.summary_digest = trace.digest();
    let original = trace.clone();
    assert!(matches!(
        exporter.export_summary(&trace, None, true),
        Err(TraceExportError::SummaryInvalid(_))
    ));
    assert_eq!(trace, original);
    assert!(exporter.records().is_empty());
    assert_eq!(exporter.jsonl().unwrap(), "");
    assert!(matches!(
        exporter.export_summary(&summary(), None, true).unwrap(),
        TraceExportDisposition::Exported(receipt) if receipt.sequence == 1
    ));
}

fn foreign_parent() -> String {
    "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned()
}

fn refused_baggage(baggage: &str) -> TraceExportError {
    ForeignTraceContext::from_ingress(None, None, Some(baggage)).unwrap_err()
}

fn foreign_context_code(error: &TraceExportError) -> Option<&str> {
    match error {
        TraceExportError::ForeignContextInvalid(code) => Some(code.as_str()),
        _ => None,
    }
}

fn foreign_authority_key(error: &TraceExportError) -> Option<&str> {
    match error {
        TraceExportError::ForeignAuthorityKey(key) => Some(key.as_str()),
        _ => None,
    }
}

fn run_receipt() -> RunReceipt {
    RunReceipt::new(
        RunId::new(),
        SessionId::new("oa14-session"),
        None,
        json_digest(&json!({"project": "oa14"})),
        ExecutionStatus::Completed,
        None,
        1,
        vec![EventId::new()],
        RedactionProfile::for_signal(RedactionSignal::Audit).profile_digest,
        "implemented",
        "source",
        json_digest(&json!({"result": "ok"})),
        Vec::new(),
    )
    .unwrap()
}

#[test]
fn oa14_exportable_status_and_config_preserve_projection_semantics() {
    assert_eq!(exportable_status(TraceStatus::Ok), TraceStatus::Ok);
    assert_eq!(exportable_status(TraceStatus::Error), TraceStatus::Error);
    assert_eq!(
        exportable_status(TraceStatus::Degraded),
        TraceStatus::Degraded
    );
    assert_eq!(
        exportable_status(TraceStatus::Unknown),
        TraceStatus::Unknown
    );

    let config = TraceExportConfig {
        enabled: true,
        sampled: true,
        capacity: 3,
    };
    let exporter = LocalTraceExporter::new(config).unwrap();
    assert_eq!(exporter.config(), config);
    assert_eq!(TraceExportConfig::default().capacity, 1_024);
}

#[test]
fn oa14_sampling_decision_is_server_owned() {
    let disabled = LocalTraceExporter::noop();
    assert_eq!(
        disabled.sampling_decision(true),
        TraceSamplingDecision::NotSampled(TraceSamplingReason::ExporterDisabled)
    );
    assert!(!disabled.sampling_decision(true).sampled());

    let unsampled = LocalTraceExporter::new(TraceExportConfig {
        enabled: true,
        sampled: false,
        capacity: 1,
    })
    .unwrap();
    assert_eq!(
        unsampled.sampling_decision(true).reason(),
        Some(TraceSamplingReason::LocalSamplingOff)
    );

    let exporter = enabled_exporter();
    assert!(!exporter.sampling_decision(false).sampled());
    assert!(exporter.sampling_decision(true).sampled());
    assert_eq!(exporter.sampling_decision(true).reason(), None);
    assert_eq!(
        TraceSamplingReason::ExporterDisabled.code(),
        "exporter_disabled"
    );
    assert_eq!(
        TraceSamplingReason::LocalSamplingOff.code(),
        "local_sampling_off"
    );
    assert_eq!(
        TraceSamplingReason::CallerDeclined.code(),
        "caller_declined"
    );

    let parent = foreign_parent();
    let sampled_parent = ForeignTraceContext::from_ingress(Some(&parent), None, None).unwrap();
    assert_eq!(sampled_parent.parent_sampled(), Some(true));
    let decision = unsampled
        .export_summary_with_context(&summary(), Some(&sampled_parent), true)
        .unwrap();
    assert_eq!(decision, TraceExportDisposition::SampledOut);

    let quiet = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00";
    let quiet_parent = ForeignTraceContext::from_ingress(Some(quiet), None, None).unwrap();
    assert_eq!(quiet_parent.parent_sampled(), Some(false));
    let decision = exporter
        .export_summary_with_context(&summary(), Some(&quiet_parent), true)
        .unwrap();
    assert!(matches!(decision, TraceExportDisposition::Exported(_)));
}

#[test]
fn oa14_foreign_tracestate_and_baggage_are_inert_digests() {
    let parent = foreign_parent();
    let context = ForeignTraceContext::from_ingress(
        Some(&parent),
        Some("vendor=alpha,vendor2=beta"),
        Some("lane=gamma"),
    )
    .unwrap();
    assert_eq!(context.trace_state.len(), 2);
    assert_eq!(context.baggage.len(), 1);
    assert_eq!(context.trace_state[0].key, "vendor");
    assert_eq!(
        context.trace_state[0].value_digest,
        json_digest(&json!({"key": "vendor", "value": "alpha"}))
    );
    assert_eq!(
        context.baggage[0].value_digest,
        json_digest(&json!({"key": "lane", "value": "gamma"}))
    );
    let debug = format!("{context:?}");
    assert!(!debug.contains("alpha"));
    assert!(!debug.contains("beta"));
    assert!(!debug.contains("gamma"));

    let links = context.span_links();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].relationship, SpanLinkKind::ForeignParent);
    assert_eq!(links[0].span.span_id.as_str(), "00f067aa0ba902b7");
    assert_eq!(
        links[0].span.trace_id.as_str(),
        "4bf92f3577b34da6a3ce929d0e0e4736"
    );

    let exporter = enabled_exporter();
    let disposition = exporter
        .export_summary_with_context(&summary(), Some(&context), true)
        .unwrap();
    assert!(matches!(disposition, TraceExportDisposition::Exported(_)));
    let records = exporter.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].attributes, summary().attributes);
    let parent_span = records[0].parent_span_id.as_ref().unwrap();
    assert_eq!(parent_span.as_str(), "00f067aa0ba902b7");
    let jsonl = exporter.jsonl().unwrap();
    assert!(!jsonl.contains("alpha"));
    assert!(!jsonl.contains("gamma"));
}

#[test]
fn oa14_foreign_context_refuses_malformed_secret_and_authority_members() {
    let authority = "gateway.actor=operator";
    let error = ForeignTraceContext::from_ingress(None, Some(authority), None).unwrap_err();
    assert_eq!(error.code(), "trace_export_foreign_authority_key");
    assert_eq!(foreign_authority_key(&error), Some("actor"));
    assert!(!error.to_string().contains("operator"));

    let error = refused_baggage("Vendor=alpha");
    assert_eq!(
        foreign_context_code(&error),
        Some("trace_export_foreign_key_invalid")
    );

    let error = refused_baggage("novalue");
    assert_eq!(
        foreign_context_code(&error),
        Some("trace_export_foreign_member_invalid")
    );

    let error = refused_baggage(" =alpha");
    assert_eq!(
        foreign_context_code(&error),
        Some("trace_export_foreign_key_invalid")
    );

    let oversized = "x".repeat(MAX_FOREIGN_CONTEXT_BYTES + 1);
    let error = refused_baggage(&oversized);
    assert_eq!(
        foreign_context_code(&error),
        Some("trace_export_foreign_context_size")
    );

    let too_many = (0..=MAX_FOREIGN_ANNOTATIONS)
        .map(|index| format!("k{index}=v"))
        .collect::<Vec<_>>()
        .join(",");
    let error = refused_baggage(&too_many);
    assert_eq!(
        foreign_context_code(&error),
        Some("trace_export_foreign_annotation_limit")
    );

    let long_key = format!("{}=v", "k".repeat(MAX_FOREIGN_KEY_BYTES + 1));
    let error = refused_baggage(&long_key);
    assert_eq!(
        foreign_context_code(&error),
        Some("trace_export_foreign_key_invalid")
    );

    let long_value = format!("vendor={}", "v".repeat(MAX_FOREIGN_VALUE_BYTES + 1));
    let error = refused_baggage(&long_value);
    assert_eq!(
        foreign_context_code(&error),
        Some("trace_export_foreign_value_invalid")
    );

    let secret = "vendor=sk-oa14foreignsecret";
    let error = refused_baggage(secret);
    assert_eq!(
        foreign_context_code(&error),
        Some("trace_export_foreign_secret_sentinel")
    );

    let changed = "vendor=api_key=oa14foreignchanged";
    let error = refused_baggage(changed);
    assert_eq!(
        foreign_context_code(&error),
        Some("trace_export_foreign_annotation_changed")
    );

    for error in [
        refused_baggage("Vendor=alpha"),
        refused_baggage("novalue"),
        refused_baggage(secret),
        refused_baggage(changed),
    ] {
        let debug = format!("{error:?}");
        assert!(!debug.contains("oa14foreignsecret"));
        assert!(!debug.contains("oa14foreignchanged"));
    }
}

#[test]
fn oa14_otlp_mapping_is_deterministic_local_and_authority_free() {
    let exporter = enabled_exporter();
    let disposition = exporter
        .export_summary(&summary(), Some(&foreign_parent()), true)
        .unwrap();
    assert!(matches!(disposition, TraceExportDisposition::Exported(_)));
    let records = exporter.records();

    let mapping = otlp_export_json(&records).unwrap();
    assert_eq!(
        mapping["mappingVersion"].as_str(),
        Some(OTLP_MAPPING_SCHEMA)
    );
    assert_eq!(
        mapping["mappingLimitations"],
        json!(OTLP_MAPPING_LIMITATIONS)
    );
    let digest = otlp_export_digest(&records).unwrap();
    assert_eq!(digest, json_digest(&mapping));
    assert_eq!(digest, otlp_export_digest(&records).unwrap());

    let span = &mapping["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
    assert_eq!(span["traceId"].as_str(), Some(records[0].trace_id.as_str()));
    assert_eq!(span["spanId"].as_str(), Some(records[0].span_id.as_str()));
    assert_eq!(span["parentSpanId"].as_str(), Some("00f067aa0ba902b7"));
    assert_eq!(span["name"].as_str(), Some("kiana.trace"));
    assert_eq!(span["kind"].as_u64(), Some(1));
    assert_eq!(span["flags"].as_u64(), Some(1));
    assert_eq!(span["status"]["code"].as_u64(), Some(1));
    assert!(span.get("actor").is_none());
    assert!(span.get("authority").is_none());
    assert!(span.get("receipt").is_none());

    let attributes = span["attributes"].as_array().unwrap();
    let digest_attribute = attributes
        .iter()
        .find(|attribute| attribute["key"] == "kiana.export.digest")
        .unwrap();
    assert_eq!(
        digest_attribute["value"]["stringValue"].as_str(),
        Some(records[0].export_digest.as_str())
    );

    let parent = foreign_parent();
    let context =
        ForeignTraceContext::from_ingress(Some(&parent), Some("vendor=alpha"), Some("lane=gamma"))
            .unwrap();
    let annotated = enabled_exporter();
    let disposition = annotated
        .export_summary_with_context(&summary(), Some(&context), true)
        .unwrap();
    assert!(matches!(disposition, TraceExportDisposition::Exported(_)));
    let mapping = otlp_export_json(&annotated.records()).unwrap();
    let rendered = mapping.to_string();
    assert!(!rendered.contains("alpha"));
    assert!(!rendered.contains("gamma"));

    let empty = otlp_export_json(&[]).unwrap();
    assert_eq!(empty["mappingVersion"].as_str(), Some(OTLP_MAPPING_SCHEMA));
    assert_eq!(
        empty["resourceSpans"][0]["scopeSpans"][0]["spans"],
        json!([])
    );
    assert_eq!(otlp_export_digest(&[]).unwrap(), json_digest(&empty));
}

#[test]
fn oa14_otlp_mapping_rejects_oversize_and_tampered_records() {
    let exporter = enabled_exporter();
    let disposition = exporter.export_summary(&summary(), None, true).unwrap();
    assert!(matches!(disposition, TraceExportDisposition::Exported(_)));
    let record = exporter.records().remove(0);
    otlp_export_json(&[record.clone()]).unwrap();

    let oversized = vec![record.clone(); 4_097];
    assert_eq!(
        otlp_export_json(&oversized).unwrap_err(),
        TraceExportError::CapacityExceeded
    );
    assert_eq!(
        otlp_export_digest(&oversized).unwrap_err(),
        TraceExportError::CapacityExceeded
    );

    let mut tampered = record.clone();
    tampered.export_digest.push('0');
    let error = otlp_export_json(&[tampered]).unwrap_err();
    assert_eq!(error.code(), "trace_export_record_invalid");
}

#[test]
fn oa14_exporter_failure_and_shutdown_never_change_receipt_facts() {
    let receipt = run_receipt();
    let original = receipt.clone();
    let digest = receipt.receipt_digest.clone();

    let exporter = enabled_exporter();
    let disposition = exporter
        .export_summary(&summary(), Some(&foreign_parent()), true)
        .unwrap();
    assert!(matches!(disposition, TraceExportDisposition::Exported(_)));
    assert_eq!(exporter.records().len(), 1);
    assert_eq!(exporter.shutdown(), 0);
    assert_eq!(
        exporter.export_summary(&summary(), Some(&foreign_parent()), true),
        Err(TraceExportError::Closed)
    );
    assert_eq!(exporter.reopen(), 1);
    assert_eq!(
        exporter.export_summary(&summary(), Some(&foreign_parent()), true),
        Err(TraceExportError::CapacityExceeded)
    );
    assert_eq!(exporter.records().len(), 1);
    assert_eq!(exporter.drain_jsonl().unwrap().lines().count(), 1);

    let stats = exporter.stats();
    assert_eq!(stats.exported_total, 1);
    assert_eq!(stats.sampled_out_total, 0);
    assert_eq!(stats.rejected_total, 2);
    assert_eq!(stats.flush_sequence, 1);
    assert_eq!(stats.depth, 0);
    assert_eq!(stats.capacity, 1);
    assert!(!stats.closed);
    assert_eq!(
        stats.last_rejection_reason.as_deref(),
        Some("trace_export_capacity_exceeded")
    );

    receipt.validate().unwrap();
    assert_eq!(receipt, original);
    assert_eq!(receipt.receipt_digest, digest);
}

#[test]
fn oa14_exporter_stats_never_echo_refused_content() {
    let exporter = enabled_exporter();
    let error = exporter
        .export_summary(&summary(), Some("oa14-not-a-traceparent"), true)
        .unwrap_err();
    assert!(matches!(error, TraceExportError::ParentInvalid(_)));
    let reason = exporter.stats().last_rejection_reason.unwrap_or_default();
    assert_eq!(reason, "trace_export_parent_invalid");

    let mut trace = summary();
    trace
        .attributes
        .insert("component".to_owned(), "sk-oa14statssecret".to_owned());
    trace.summary_digest = trace.digest();
    let error = exporter.export_summary(&trace, None, true).unwrap_err();
    assert!(matches!(error, TraceExportError::RecordInvalid(_)));
    assert!(!error.to_string().contains("oa14statssecret"));

    let stats = exporter.stats();
    assert_eq!(stats.exported_total, 0);
    assert_eq!(stats.sampled_out_total, 0);
    assert_eq!(stats.rejected_total, 2);
    assert_eq!(stats.depth, 0);
    let reason = stats.last_rejection_reason.unwrap_or_default();
    assert!(!reason.contains("oa14statssecret"));
    assert_eq!(reason, "trace_export_record_invalid");
}

#[test]
fn oa14_drain_jsonl_releases_capacity_and_keeps_sequence() {
    let exporter = LocalTraceExporter::new(TraceExportConfig {
        enabled: true,
        sampled: true,
        capacity: 2,
    })
    .unwrap();
    let first = exporter.export_summary(&summary(), None, true).unwrap();
    let second = exporter.export_summary(&summary(), None, true).unwrap();
    assert!(matches!(first, TraceExportDisposition::Exported(receipt) if receipt.sequence == 1));
    assert!(matches!(second, TraceExportDisposition::Exported(receipt) if receipt.sequence == 2));

    let jsonl = exporter.jsonl().unwrap();
    assert_eq!(jsonl.lines().count(), 2);
    assert_eq!(exporter.drain_jsonl().unwrap(), jsonl);
    assert_eq!(exporter.jsonl().unwrap(), "");
    assert!(exporter.records().is_empty());
    assert_eq!(exporter.stats().depth, 0);
    assert_eq!(exporter.stats().exported_total, 2);

    let third = exporter.export_summary(&summary(), None, true).unwrap();
    assert!(matches!(third, TraceExportDisposition::Exported(receipt) if receipt.sequence == 3));
}

#[test]
fn oa14_trace_export_module_has_no_authority_or_io_reach() {
    let source = include_str!("../src/trace_export.rs");
    for forbidden in [
        "EventStore",
        "ControlPlane",
        "CapabilityBroker",
        "RunReceipt",
        "CommandReceipt",
        "authority_epoch",
        "RequestContext",
        "std::fs",
        "reqwest",
        "tokio::net",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }
    for required in [
        "records: Vec<TraceExportSpan>",
        "const MAX_EXPORT_RECORDS: usize = 4_096;",
        "pub fn sampling_decision",
        "pub fn export_summary_with_context",
        "pub fn drain_jsonl",
        "pub fn otlp_export_json",
        "fn record_failure(",
        "state.rejected_total = state.rejected_total.saturating_add(1);",
    ] {
        assert!(source.contains(required), "{required}");
    }
}

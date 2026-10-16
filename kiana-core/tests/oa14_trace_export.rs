use kiana_core::{
    foreign_parent_link, LocalTraceExporter, TraceExportConfig, TraceExportDisposition,
    TraceExportError,
};
use kiana_domain::{EventId, SpanLinkKind, TraceStatus, TraceSummary};
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

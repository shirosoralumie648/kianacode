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

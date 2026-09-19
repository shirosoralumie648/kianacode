use kiana_domain::{EvalCaseId, EvalSuiteId};
use kiana_quality::{
    capture_golden_trace, CaptureError, CaptureSource, GoldenTraceCaptureRequest, CAPTURE_SCHEMA,
};
use serde_json::json;
use std::collections::BTreeMap;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn request(source: CaptureSource, destination_ref: &str) -> GoldenTraceCaptureRequest {
    GoldenTraceCaptureRequest {
        schema: CAPTURE_SCHEMA.to_owned(),
        source,
        destination_ref: destination_ref.to_owned(),
        suite_id: EvalSuiteId::new(),
        case_id: EvalCaseId::new(),
        source_snapshot: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            .to_owned(),
        input_hash: DIGEST.to_owned(),
        target_versions: BTreeMap::from([("runtime".to_owned(), "v1".to_owned())]),
        event_cursor_start: 1,
        event_cursor_end: 1,
        normalized_events: vec![json!({"kind": "run.completed", "status": "ok"})],
        artifact_hashes: vec![DIGEST.to_owned()],
        receipt_hash: Some(DIGEST.to_owned()),
        normalization_version: "eq20.digest.v1".to_owned(),
        created_at_unix_ms: 100,
        expires_at_unix_ms: Some(200),
        provenance_ref: "fixture:source-1".to_owned(),
    }
}

#[test]
fn explicit_fixture_capture_creates_a_new_typed_trace() {
    let (trace, receipt) = capture_golden_trace(
        request(
            CaptureSource::Fixture {
                fixture_ref: "eval/source-1".to_owned(),
                fixture_digest: DIGEST.to_owned(),
            },
            "golden/run-1/v2",
        ),
        &[],
    )
    .expect("capture from explicit fixture");
    assert!(trace.source_run_id.is_none());
    assert_eq!(receipt.destination_ref, "golden/run-1/v2");
    assert_eq!(receipt.trace_id, trace.trace_id);
    trace.validate().unwrap();
}

#[test]
fn explicit_run_capture_binds_source_run_and_never_overwrites_destination() {
    let first_request = request(
        CaptureSource::Run {
            run_id: kiana_domain::RunId::new(),
            source_event_digest: DIGEST.to_owned(),
            receipt_digest: Some(DIGEST.to_owned()),
        },
        "golden/run-2/v1",
    );
    let (first, _) = capture_golden_trace(first_request, &[]).expect("first capture");
    assert!(first.source_run_id.is_some());

    let collision = request(
        CaptureSource::Fixture {
            fixture_ref: "eval/source-2".to_owned(),
            fixture_digest: DIGEST.to_owned(),
        },
        "golden/run-2/v1",
    );
    assert_eq!(
        capture_golden_trace(collision, &["golden/run-2/v1".to_owned()]).unwrap_err(),
        CaptureError::DestinationExists
    );
}

#[test]
fn missing_or_invalid_source_metadata_fails_closed() {
    let invalid = request(
        CaptureSource::Fixture {
            fixture_ref: String::new(),
            fixture_digest: "not-a-digest".to_owned(),
        },
        "golden/run-3/v1",
    );
    assert!(matches!(
        invalid.validate(&[]),
        Err(CaptureError::SourceInvalid(_))
    ));

    let invalid_destination = request(
        CaptureSource::Fixture {
            fixture_ref: "eval/source-3".to_owned(),
            fixture_digest: DIGEST.to_owned(),
        },
        "\n",
    );
    assert_eq!(
        invalid_destination.validate(&[]).unwrap_err(),
        CaptureError::DestinationInvalid
    );
}

use kiana_domain::{
    AggregationVerification, EventId, ReceiptAggregation, SchemaVersion,
    RECEIPT_AGGREGATION_SCHEMA, RECEIPT_AGGREGATION_VERSION,
};
use serde_json::json;

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

#[test]
fn aggregation_is_strict_and_keeps_estimate_separate() {
    let aggregation = ReceiptAggregation::new(
        4,
        vec![EventId::new()],
        2,
        1,
        Some(10),
        Some(20),
        false,
        None,
        false,
        vec!["src/main.rs".to_owned()],
        3,
        vec![digest('a')],
        vec![digest('b')],
        AggregationVerification::Complete,
    )
    .unwrap();
    assert_eq!(aggregation.schema, RECEIPT_AGGREGATION_SCHEMA);
    assert_eq!(aggregation.version, RECEIPT_AGGREGATION_VERSION);
    assert!(aggregation.validate().is_ok());
    assert_eq!(
        ReceiptAggregation::from_json(&aggregation.to_json().unwrap()).unwrap(),
        aggregation
    );

    let mut unknown = aggregation.to_json().unwrap();
    unknown["raw_usage"] = json!("must-not-become-cost");
    assert_eq!(
        ReceiptAggregation::from_json(&unknown).unwrap_err(),
        "receipt_aggregation_decode_failed"
    );

    let mut estimated_without_value = aggregation.clone();
    estimated_without_value.cost_estimated = true;
    estimated_without_value.aggregation_digest = estimated_without_value.digest();
    assert_eq!(
        estimated_without_value.validate().unwrap_err(),
        "receipt_aggregation_header_invalid"
    );
}

#[test]
fn aggregation_rejects_absolute_or_tampered_paths_and_versions() {
    let mut aggregation = ReceiptAggregation::new(
        1,
        vec![EventId::new()],
        0,
        0,
        None,
        None,
        false,
        None,
        false,
        vec!["relative.txt".to_owned()],
        0,
        Vec::new(),
        Vec::new(),
        AggregationVerification::Unknown,
    )
    .unwrap();
    aggregation.files_changed = vec!["/outside.txt".to_owned()];
    aggregation.aggregation_digest = aggregation.digest();
    assert_eq!(
        aggregation.validate().unwrap_err(),
        "receipt_aggregation_header_invalid"
    );
    aggregation.files_changed = vec!["relative.txt".to_owned()];
    aggregation.version = SchemaVersion::new(2, 0);
    aggregation.aggregation_digest = aggregation.digest();
    assert_eq!(
        aggregation.validate().unwrap_err(),
        "receipt_aggregation_header_invalid"
    );
}

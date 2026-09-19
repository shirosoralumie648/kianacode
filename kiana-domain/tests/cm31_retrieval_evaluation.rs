use kiana_domain::{
    json_digest, ndcg_at_k, recall_at_k, reciprocal_rank, RetrievalEvaluationReport,
    RetrievalQualityEvidence, RetrievalQualityMetrics, RetrievalSafetyMetrics,
};
use serde_json::json;

fn digest(value: &str) -> String {
    json_digest(&json!({"value": value}))
}

#[test]
fn retrieval_eval_reports_quality_and_safety_separately() {
    let expected = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
    let observed = vec!["b".to_owned(), "a".to_owned(), "x".to_owned()];
    let quality = RetrievalQualityMetrics::new(
        recall_at_k(&expected, &observed, 2),
        reciprocal_rank(&expected, &observed),
        ndcg_at_k(&expected, &observed, 3),
        0.75,
        0.9,
        0.0,
        120,
        0.0,
    )
    .unwrap();
    let safety = RetrievalSafetyMetrics::new(0, 1, 0, 0);
    let report = RetrievalEvaluationReport::new(
        digest("fixture"),
        digest("algorithm"),
        digest("embedding"),
        RetrievalQualityEvidence::FixtureDeterminism,
        true,
        false,
        quality,
        safety,
        vec!["semantic_quality_not_measured".to_owned()],
    )
    .unwrap();
    report.validate().unwrap();
    assert!(!report.safety_passed());
    assert!(!report.semantic_quality_measured);
    assert!(report.quality.mrr > 0.0);
}

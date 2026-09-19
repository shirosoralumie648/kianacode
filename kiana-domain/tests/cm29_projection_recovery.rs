use kiana_domain::{
    json_digest, ProjectionLagStatus, ProjectionLagView, UnknownMutationReconciliation,
};
use serde_json::json;

fn digest(value: &str) -> String {
    json_digest(&json!({"value": value}))
}

#[test]
fn projection_lag_is_visible() {
    let lag = ProjectionLagView::new(
        10,
        Some(7),
        3,
        9,
        Some("projection_cursor_lagging".to_owned()),
    )
    .unwrap();
    assert_eq!(lag.status, ProjectionLagStatus::Pending);
    assert!(lag.projection_pending);
    assert!(!lag.read_consistent());
    lag.validate().unwrap();

    let unknown = ProjectionLagView::new(
        10,
        None,
        3,
        9,
        Some("projection_cursor_unobserved".to_owned()),
    )
    .unwrap();
    assert_eq!(unknown.status, ProjectionLagStatus::Unknown);
    assert!(unknown.projection_pending);
}

#[test]
fn unknown_mutation_is_not_retried_with_new_id() {
    let reconciliation =
        UnknownMutationReconciliation::new("mutation:original", digest("request")).unwrap();
    reconciliation
        .reconcile("mutation:original", &digest("request"))
        .unwrap();
    assert_eq!(
        reconciliation
            .reconcile("mutation:new", &digest("request"))
            .unwrap_err(),
        "unknown_mutation_new_id_forbidden"
    );
    assert_eq!(
        reconciliation
            .reconcile("mutation:original", &digest("different"))
            .unwrap_err(),
        "unknown_mutation_request_digest_mismatch"
    );
}

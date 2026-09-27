use kiana_domain::{
    BillingQueryCursor, BillingQueryKind, BillingQueryRequest, BillingQueryResponse,
    BillingRollupTotals, Freshness, ProjectId, BILLING_QUERY_SCHEMA,
};
use serde_json::json;

const BOUNDARY: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PROJECTION: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn request(cursor: Option<BillingQueryCursor>) -> BillingQueryRequest {
    BillingQueryRequest {
        schema: BILLING_QUERY_SCHEMA.to_owned(),
        kind: BillingQueryKind::BudgetSummary,
        project_id: ProjectId::new(),
        data_boundary_digest: BOUNDARY.to_owned(),
        source_cursor: Some(10),
        after_cursor: cursor.as_ref().map_or(0, |value| value.after_cursor),
        limit: 10,
        cursor,
        read_only: true,
    }
}

#[test]
fn request_and_cursor_bind_boundary_source_and_page() {
    let cursor = BillingQueryCursor::new(10, 3, BOUNDARY, PROJECTION).unwrap();
    request(Some(cursor)).validate().unwrap();

    let mut invalid = request(None);
    invalid.read_only = false;
    assert_eq!(invalid.validate(), Err("billing_query_request_invalid"));

    let mut invalid = request(None);
    invalid.source_cursor = None;
    invalid.after_cursor = 1;
    assert_eq!(invalid.validate(), Err("billing_query_request_invalid"));

    let mut invalid = request(None);
    invalid.after_cursor = 11;
    assert_eq!(invalid.validate(), Err("billing_query_request_invalid"));
}

#[test]
fn response_keeps_freshness_unknown_and_export_digest_explicit() {
    let project_id = ProjectId::new();
    let mut response = BillingQueryResponse::new(
        BillingQueryKind::CostExport,
        project_id,
        BOUNDARY,
        10,
        PROJECTION,
        Freshness::Current,
        0,
        BillingRollupTotals::default(),
        4,
        2,
        Some(PROJECTION.to_owned()),
        None,
    )
    .unwrap();
    response.validate().unwrap();

    response.freshness = Freshness::Unknown;
    response.response_digest = response.digest();
    response.validate().unwrap();

    let mut value = serde_json::to_value(response).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<BillingQueryResponse>(value).is_err());
}

#[test]
fn non_export_response_cannot_smuggle_export_digest() {
    let result = BillingQueryResponse::new(
        BillingQueryKind::BudgetSummary,
        ProjectId::new(),
        BOUNDARY,
        10,
        PROJECTION,
        Freshness::Current,
        0,
        BillingRollupTotals::default(),
        1,
        0,
        Some(PROJECTION.to_owned()),
        None,
    );
    assert_eq!(
        result.unwrap_err(),
        "billing_query_export_digest_unexpected"
    );
}

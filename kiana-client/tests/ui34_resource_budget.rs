use kiana_client::{
    evaluate_budget, UiBudgetDecision, UiBudgetError, UiBudgetSurface, UiResourceBudget,
    UiResourceUsage, UI_RESOURCE_BUDGET_SCHEMA,
};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn budget() -> UiResourceBudget {
    UiResourceBudget {
        schema: UI_RESOURCE_BUDGET_SCHEMA.to_owned(),
        surface: UiBudgetSurface::Feed,
        max_bytes: 100,
        max_items: 10,
        max_sessions: 4,
        max_queue_depth: 8,
    }
}

fn usage(
    bytes: u64,
    items: u64,
    sessions: u64,
    queue_depth: u64,
    pending_items: u64,
    unknown_items: u64,
) -> UiResourceUsage {
    UiResourceUsage {
        schema: UI_RESOURCE_BUDGET_SCHEMA.to_owned(),
        bytes,
        items,
        sessions,
        queue_depth,
        pending_items,
        unknown_items,
    }
}

#[test]
fn fixture_declares_budget_and_protected_item_contract() {
    let value: Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ui34-resource-budget.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(value["schema"], "kiana.ui-resource-budget-fixture.v1");
    assert!(value["denied"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains("silent")));
}

#[test]
fn budget_accepts_degrades_and_rejects_explicitly() {
    assert_eq!(
        evaluate_budget(&budget(), &usage(10, 2, 1, 1, 1, 0)).unwrap(),
        UiBudgetDecision::Accept
    );
    assert!(matches!(
        evaluate_budget(&budget(), &usage(10, 12, 1, 1, 1, 1)).unwrap(),
        UiBudgetDecision::Degraded { .. }
    ));
    assert!(matches!(
        evaluate_budget(&budget(), &usage(101, 2, 1, 1, 0, 0)).unwrap(),
        UiBudgetDecision::Reject { .. }
    ));
    assert_eq!(
        evaluate_budget(&budget(), &usage(10, 10, 1, 1, 10, 1)),
        Err(UiBudgetError::ProtectedItemsExceedLimit)
    );
}

#[test]
fn invalid_budget_and_usage_fail_closed() {
    let mut invalid_budget = budget();
    invalid_budget.max_bytes = 0;
    assert_eq!(
        evaluate_budget(&invalid_budget, &usage(1, 1, 1, 1, 0, 0)),
        Err(UiBudgetError::LimitInvalid)
    );
    let invalid_usage = usage(1, 1, 1, 1, 2, 0);
    assert_eq!(
        evaluate_budget(&budget(), &invalid_usage),
        Err(UiBudgetError::UsageInvalid)
    );
}

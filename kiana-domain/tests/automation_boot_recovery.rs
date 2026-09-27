use kiana_domain::{
    AutomationBootRecoveryFact, AutomationBootRecoveryState, AUTOMATION_BOOT_RECOVERY_SCHEMA,
};
use serde_json::json;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn fact(state: AutomationBootRecoveryState) -> AutomationBootRecoveryFact {
    let mut value = AutomationBootRecoveryFact {
        schema: AUTOMATION_BOOT_RECOVERY_SCHEMA.to_owned(),
        source_cursor: 10,
        projection_cursor: 10,
        projection_generation: 2,
        authority_epoch: 4,
        pending_unknown_count: 0,
        stale_index: false,
        state,
        reconcile_refs: Vec::new(),
        recovery_digest: String::new(),
    };
    value.recovery_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "source_cursor": value.source_cursor,
        "projection_cursor": value.projection_cursor,
        "projection_generation": value.projection_generation,
        "authority_epoch": value.authority_epoch,
        "pending_unknown_count": value.pending_unknown_count,
        "stale_index": value.stale_index,
        "state": value.state,
        "reconcile_refs": value.reconcile_refs,
    }));
    value
}

#[test]
fn ready_requires_current_projection_without_unknowns() {
    assert!(fact(AutomationBootRecoveryState::Ready).validate().is_ok());
    let mut reconcile = fact(AutomationBootRecoveryState::ReconcileRequired);
    reconcile.pending_unknown_count = 1;
    reconcile.reconcile_refs = vec!["reconcile:one".to_owned()];
    reconcile.recovery_digest = kiana_domain::json_digest(&json!({
        "schema": reconcile.schema,
        "source_cursor": reconcile.source_cursor,
        "projection_cursor": reconcile.projection_cursor,
        "projection_generation": reconcile.projection_generation,
        "authority_epoch": reconcile.authority_epoch,
        "pending_unknown_count": reconcile.pending_unknown_count,
        "stale_index": reconcile.stale_index,
        "state": reconcile.state,
        "reconcile_refs": reconcile.reconcile_refs,
    }));
    assert!(reconcile.validate().is_ok());
}

#[test]
fn cursor_regression_and_unknown_fields_fail_closed() {
    let mut invalid = fact(AutomationBootRecoveryState::Ready);
    invalid.projection_cursor = 11;
    invalid.recovery_digest = D.to_owned();
    assert_eq!(invalid.validate(), Err("automation_boot_recovery_invalid"));
    let mut value = serde_json::to_value(fact(AutomationBootRecoveryState::Ready)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationBootRecoveryFact>(value).is_err());
}

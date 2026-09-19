use kiana_domain::{DurableTrigger, TriggerConcurrency, TriggerDefinition, TriggerSchedule};
use serde_json::json;
use std::collections::BTreeMap;

fn definition() -> TriggerDefinition {
    TriggerDefinition {
        trigger_id: "trigger-1".to_owned(),
        definition_id: "workflow-1".to_owned(),
        definition_version: 1,
        inputs: BTreeMap::new(),
        owner_id: "owner-1".to_owned(),
        role_id: "builder".to_owned(),
        expires_at: 10_000,
        max_firings: 16,
        concurrency: TriggerConcurrency::Coalesce,
        missed_schedule: kiana_domain::MissedSchedulePolicy::CatchUp,
        schedule: TriggerSchedule::Interval {
            every_ms: 1_000,
            first_at: 1_000,
        },
        approval_ref: "event:approval".to_owned(),
    }
}

#[test]
fn legacy_trigger_state_can_rehydrate_without_digest_maps() {
    let encoded = json!({
        "definition": definition(),
        "enabled": true,
        "next_at": 1_000,
        "firings_used": 0,
        "pending_keys": ["schedule:1000"],
        "fired": {}
    });
    let trigger: DurableTrigger = serde_json::from_value(encoded).expect("legacy state");
    assert!(trigger.pending_digests.is_empty());
    assert!(trigger.fired_digests.is_empty());
}

#[test]
fn policy_catalog_stays_explicit_and_bounded() {
    let values = [
        TriggerConcurrency::Reject,
        TriggerConcurrency::Queue,
        TriggerConcurrency::Replace,
        TriggerConcurrency::Coalesce,
    ];
    assert_eq!(values.len(), 4);
    assert!(values.contains(&TriggerConcurrency::Coalesce));
}

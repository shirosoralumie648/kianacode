use kiana_domain::{MissedSchedulePolicy, TriggerConcurrency, TriggerDefinition, TriggerSchedule};
use serde_json::json;
use std::collections::BTreeMap;

fn trigger(schedule: TriggerSchedule) -> TriggerDefinition {
    TriggerDefinition {
        trigger_id: "aut04-trigger".to_owned(),
        definition_id: "aut03-definition".to_owned(),
        definition_version: 1,
        inputs: BTreeMap::from([("input".to_owned(), json!("bounded"))]),
        owner_id: "local-user".to_owned(),
        role_id: "sponsor".to_owned(),
        expires_at: 100_000,
        max_firings: 4,
        concurrency: TriggerConcurrency::Reject,
        missed_schedule: MissedSchedulePolicy::Skip,
        schedule,
        approval_ref: "event:approval".to_owned(),
    }
}

#[test]
fn manual_event_and_interval_trigger_shapes_are_bounded_and_digestable() {
    for schedule in [
        TriggerSchedule::Manual,
        TriggerSchedule::Event {
            kind: "company.ProjectApproved".to_owned(),
        },
        TriggerSchedule::Interval {
            every_ms: 1_000,
            first_at: 2_000,
        },
    ] {
        let trigger = trigger(schedule);
        trigger.validate_shape(1_000).unwrap();
        assert!(trigger.digest().starts_with("sha256:"));
    }
}

#[test]
fn trigger_shape_rejects_untrusted_source_payload_and_expiry() {
    let mut malformed = trigger(TriggerSchedule::Event {
        kind: "bad\nsource".to_owned(),
    });
    assert_eq!(
        malformed.validate_shape(1_000).unwrap_err(),
        "trigger_event_kind_invalid"
    );
    malformed = trigger(TriggerSchedule::Interval {
        every_ms: 999,
        first_at: 2_000,
    });
    assert_eq!(
        malformed.validate_shape(1_000).unwrap_err(),
        "trigger_schedule_invalid"
    );
    malformed = trigger(TriggerSchedule::Manual);
    malformed.expires_at = 1_000;
    assert_eq!(
        malformed.validate_shape(1_000).unwrap_err(),
        "trigger_definition_shape_invalid"
    );
    malformed = trigger(TriggerSchedule::Manual);
    malformed.approval_ref = "not-an-event".to_owned();
    assert_eq!(
        malformed.validate_shape(1_000).unwrap_err(),
        "trigger_definition_shape_invalid"
    );

    let mut encoded = serde_json::to_value(trigger(TriggerSchedule::Manual)).unwrap();
    encoded["unexpected"] = json!(true);
    assert!(serde_json::from_value::<TriggerDefinition>(encoded).is_err());
}

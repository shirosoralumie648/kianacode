use kiana_core::{
    classify_notification, plan_notification_policy, NotificationPolicyConfig,
    NotificationPolicyRoute, NotificationPriority, NotificationUrgency,
    NOTIFICATION_POLICY_PLANNER_SCHEMA, NOTIFICATION_PRIORITY_SCHEMA,
};
use kiana_domain::{
    HumanAction, HumanInboxItem, HumanInboxKind, NotificationChannel, QuietHoursUtc,
};
use serde_json::{json, Value};

fn item(kind: HumanInboxKind, due_at_unix_ms: u64, expires_at_unix_ms: u64) -> HumanInboxItem {
    HumanInboxItem {
        item_id: "notification:nm17".to_owned(),
        kind,
        title: "Server-owned attention".to_owned(),
        source_ref: "event:event-nm17".to_owned(),
        run_id: None,
        detail: json!({
            "source_event_id":"event-nm17",
            "source_cursor":17,
            "due_at_unix_ms":due_at_unix_ms,
            "expires_at_unix_ms":expires_at_unix_ms,
            "notification":{"recipient_id":"principal-nm17","expires_at_unix_ms":expires_at_unix_ms},
        }),
        actions: vec![HumanAction {
            id: "review".to_owned(),
            label: "Review".to_owned(),
            command: "notification.review".to_owned(),
            arguments: json!({}),
            required_fields: Vec::new(),
        }],
    }
}

fn config(now_unix_ms: u64) -> NotificationPolicyConfig {
    NotificationPolicyConfig {
        now_unix_ms,
        reminder_interval_ms: 60_000,
        primary_channel: NotificationChannel::Web,
        fallback_channel: Some(NotificationChannel::Cli),
        quiet_hours: Some(QuietHoursUtc::new(22 * 60, 7 * 60).unwrap()),
    }
}

#[test]
fn critical_and_unknown_bypass_digest_and_keep_escalation_evidence() {
    let source = item(HumanInboxKind::Incident, 86_460_000, 86_520_000);
    let priority = classify_notification(&source).unwrap();
    assert_eq!(priority.urgency, NotificationUrgency::Critical);
    let decision = plan_notification_policy(&source, &priority, true, &config(86_400_000)).unwrap();
    assert_eq!(decision.schema, NOTIFICATION_POLICY_PLANNER_SCHEMA);
    assert_eq!(decision.route, NotificationPolicyRoute::Immediate);
    assert!(!decision.quiet_suppressed);
    assert_eq!(decision.next_action, "query_original");
    let escalation = decision.escalation.as_ref().unwrap();
    assert_eq!(escalation.owner_id, "principal-nm17");
    assert_eq!(escalation.source_event_ids, vec!["event-nm17"]);
    assert_eq!(escalation.next_action, "query_original");
    assert_eq!(escalation.reason, "unknown_requires_reconciliation");
    decision.validate().unwrap();
}

#[test]
fn high_policy_is_immediate_or_same_day_without_effect() {
    let source = item(HumanInboxKind::Approval, 86_460_000, 86_520_000);
    let priority = classify_notification(&source).unwrap();
    let decision =
        plan_notification_policy(&source, &priority, false, &config(86_400_000)).unwrap();
    assert_eq!(decision.route, NotificationPolicyRoute::SameDayReminder);
    assert!(decision.quiet_suppressed);
    assert!(decision.reminder_at_unix_ms.unwrap() > 86_400_000);
    assert_eq!(decision.primary_channel, NotificationChannel::Web);
    assert_eq!(decision.fallback_channel, Some(NotificationChannel::Cli));
}

#[test]
fn medium_is_digest_and_low_can_be_suppressed_without_reminder() {
    let medium_item = item(HumanInboxKind::Question, 86_500_000, 86_600_000);
    let medium_priority = classify_notification(&medium_item).unwrap();
    let medium = plan_notification_policy(
        &medium_item,
        &medium_priority,
        false,
        &NotificationPolicyConfig {
            quiet_hours: None,
            ..config(86_400_000)
        },
    )
    .unwrap();
    assert_eq!(medium.route, NotificationPolicyRoute::Digest);
    assert!(medium.reminder_at_unix_ms.is_some());
    assert!(medium.escalation.is_none());

    let low_item = item(HumanInboxKind::Feedback, 86_500_000, 86_600_000);
    let low_priority = NotificationPriority {
        schema: NOTIFICATION_PRIORITY_SCHEMA.to_owned(),
        item_id: low_item.item_id.clone(),
        urgency: NotificationUrgency::Low,
        due_at_unix_ms: 86_500_000,
        source_cursor: 17,
        digest_group: "digest:nm17".to_owned(),
    };
    let low =
        plan_notification_policy(&low_item, &low_priority, false, &config(86_400_000)).unwrap();
    assert_eq!(low.route, NotificationPolicyRoute::Suppressed);
    assert!(low.reminder_at_unix_ms.is_none());
}

#[test]
fn missing_server_owner_or_too_short_interval_fails_closed() {
    let mut source = item(HumanInboxKind::Approval, 86_460_000, 86_520_000);
    source.detail["notification"] = Value::Null;
    let priority = classify_notification(&source).unwrap();
    assert!(plan_notification_policy(&source, &priority, false, &config(86_400_000)).is_err());
    let valid = item(HumanInboxKind::Approval, 86_460_000, 86_520_000);
    let priority = classify_notification(&valid).unwrap();
    let mut invalid = config(86_400_000);
    invalid.reminder_interval_ms = 1;
    assert!(plan_notification_policy(&valid, &priority, false, &invalid).is_err());
}

#[test]
fn fixture_captures_deny_first_policy_contract() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/nm17-notification-policy.json"))
            .expect("valid NM-17 fixture");
    assert_eq!(fixture["schema"], "kiana.notification-policy-planner.v1");
    assert_eq!(fixture["routes"][0], "immediate");
    assert_eq!(
        fixture["channels"]["external_connector"],
        "disabled_by_default"
    );
    for denial in [
        "critical_or_unknown_must_not_enter_digest",
        "model_or_ui_self_report_does_not_create_unknown",
        "automatic_approve_retry_close",
        "direct_channel_effect",
    ] {
        assert!(fixture["deny_first"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == denial));
    }
}

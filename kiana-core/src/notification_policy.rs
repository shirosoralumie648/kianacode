//! Pure notification reminder, digest and escalation planner.
//!
//! The planner consumes server-owned materialization/priority data and returns a typed decision.
//! It has no clock read, channel call, EventLog write, Broker permit, approval decision or retry
//! driver. Unknown and critical inputs bypass digest suppression and retain an escalation fact.

use kiana_domain::{
    HumanInboxItem, NotificationChannel, NotificationEscalationFact, NotificationPolicyDecision,
    NotificationPolicyRoute, NotificationSeverity, QuietHoursUtc,
    NOTIFICATION_POLICY_MAX_REMINDER_INTERVAL_MS, NOTIFICATION_POLICY_MIN_REMINDER_INTERVAL_MS,
};
use thiserror::Error;

use crate::{NotificationPriority, NOTIFICATION_PRIORITY_SCHEMA};

pub const NOTIFICATION_POLICY_PLANNER_SCHEMA: &str = "kiana.notification-policy-planner.v1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationPolicyConfig {
    pub now_unix_ms: u64,
    pub reminder_interval_ms: u64,
    pub primary_channel: NotificationChannel,
    pub fallback_channel: Option<NotificationChannel>,
    pub quiet_hours: Option<QuietHoursUtc>,
}

impl NotificationPolicyConfig {
    pub fn validate(&self) -> Result<(), NotificationPolicyError> {
        if self.now_unix_ms == 0
            || self.reminder_interval_ms < NOTIFICATION_POLICY_MIN_REMINDER_INTERVAL_MS
            || self.reminder_interval_ms > NOTIFICATION_POLICY_MAX_REMINDER_INTERVAL_MS
            || self.fallback_channel == Some(self.primary_channel)
        {
            return Err(NotificationPolicyError::Invalid("config"));
        }
        if let Some(quiet_hours) = self.quiet_hours {
            quiet_hours
                .validate()
                .map_err(|_| NotificationPolicyError::Invalid("quiet_hours"))?;
        }
        Ok(())
    }
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum NotificationPolicyError {
    #[error("notification_policy_invalid:{0}")]
    Invalid(&'static str),
    #[error("notification_policy_source_missing")]
    SourceMissing,
    #[error("notification_policy_owner_missing")]
    OwnerMissing,
    #[error("notification_policy_priority_invalid")]
    PriorityInvalid,
    #[error("notification_policy_decision:{0}")]
    Decision(String),
}

fn server_string<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(serde_json::Value::as_str)
}

fn source_metadata(
    item: &HumanInboxItem,
) -> Result<(String, Vec<String>, String), NotificationPolicyError> {
    let detail = &item.detail;
    let notification = detail
        .get("notification")
        .and_then(serde_json::Value::as_object)
        .ok_or(NotificationPolicyError::SourceMissing)?;
    let owner_id = notification
        .get("recipient_id")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            detail
                .get("human_task")
                .and_then(serde_json::Value::as_object)
                .and_then(|task| task.get("decider_principal_id"))
                .and_then(serde_json::Value::as_str)
        })
        .filter(|value| !value.trim().is_empty())
        .ok_or(NotificationPolicyError::OwnerMissing)?
        .to_owned();
    let source_event_id = server_string(detail, "source_event_id")
        .or_else(|| item.source_ref.strip_prefix("event:"))
        .filter(|value| !value.trim().is_empty())
        .ok_or(NotificationPolicyError::SourceMissing)?
        .to_owned();
    let mut source_event_ids = vec![source_event_id.clone()];
    if let Some(source_ref) = item.source_ref.strip_prefix("event:") {
        if source_ref != source_event_id {
            source_event_ids.push(source_ref.to_owned());
        }
    }
    source_event_ids.sort();
    source_event_ids.dedup();
    Ok((owner_id, source_event_ids, source_event_id))
}

fn deadline(item: &HumanInboxItem, priority: &NotificationPriority) -> Option<u64> {
    item.detail
        .get("expires_at_unix_ms")
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            item.detail
                .get("notification")
                .and_then(serde_json::Value::as_object)
                .and_then(|notification| notification.get("expires_at_unix_ms"))
                .and_then(serde_json::Value::as_u64)
        })
        .or(Some(priority.due_at_unix_ms))
}

fn severity(priority: &NotificationPriority, unknown: bool) -> NotificationSeverity {
    if unknown || priority.urgency == crate::NotificationUrgency::Critical {
        NotificationSeverity::Critical
    } else {
        match priority.urgency {
            crate::NotificationUrgency::High => NotificationSeverity::High,
            crate::NotificationUrgency::Medium => NotificationSeverity::Medium,
            crate::NotificationUrgency::Low => NotificationSeverity::Low,
            crate::NotificationUrgency::Critical => NotificationSeverity::Critical,
        }
    }
}

fn next_action(item: &HumanInboxItem, unknown: bool) -> String {
    if unknown {
        return "query_original".to_owned();
    }
    item.actions
        .first()
        .map(|action| action.command.clone())
        .filter(|command| !command.trim().is_empty())
        .unwrap_or_else(|| "open_inbox".to_owned())
}

fn reminder_at(now: u64, interval: u64, deadline: Option<u64>) -> u64 {
    let candidate = now.saturating_add(interval);
    deadline.map_or(candidate, |value| candidate.min(value))
}

/// Produce a deterministic source-bound policy decision. `unknown` is a server projection input;
/// callers must not derive it from model/UI prose.
pub fn plan_notification_policy(
    item: &HumanInboxItem,
    priority: &NotificationPriority,
    unknown: bool,
    config: &NotificationPolicyConfig,
) -> Result<NotificationPolicyDecision, NotificationPolicyError> {
    config.validate()?;
    if priority.schema != NOTIFICATION_PRIORITY_SCHEMA
        || priority.source_cursor == 0
        || priority.due_at_unix_ms == 0
        || priority.digest_group.trim().is_empty()
    {
        return Err(NotificationPolicyError::PriorityInvalid);
    }
    let (owner_id, source_event_ids, _source_event_id) = source_metadata(item)?;
    let severity = severity(priority, unknown);
    let deadline_at_unix_ms = deadline(item, priority);
    let in_quiet_hours = config
        .quiet_hours
        .is_some_and(|quiet_hours| quiet_hours.contains_unix_ms(config.now_unix_ms));
    let quiet_end_at_unix_ms = config
        .quiet_hours
        .filter(|quiet_hours| in_quiet_hours)
        .map(|quiet_hours| quiet_hours.end_unix_ms(config.now_unix_ms));
    let next_action = next_action(item, unknown);
    let (route, quiet_suppressed, reminder_at_unix_ms) = match severity {
        NotificationSeverity::Critical => (
            NotificationPolicyRoute::Immediate,
            false,
            Some(config.now_unix_ms),
        ),
        NotificationSeverity::High if in_quiet_hours => (
            NotificationPolicyRoute::SameDayReminder,
            true,
            Some(quiet_end_at_unix_ms.ok_or(NotificationPolicyError::Invalid("quiet_hours"))?),
        ),
        NotificationSeverity::High => (
            NotificationPolicyRoute::Immediate,
            false,
            Some(config.now_unix_ms),
        ),
        NotificationSeverity::Medium => (
            NotificationPolicyRoute::Digest,
            false,
            Some(reminder_at(
                config.now_unix_ms,
                config.reminder_interval_ms,
                deadline_at_unix_ms,
            )),
        ),
        NotificationSeverity::Low => (NotificationPolicyRoute::Suppressed, false, None),
    };
    let escalation = if matches!(
        severity,
        NotificationSeverity::High | NotificationSeverity::Critical
    ) {
        let escalate_at = deadline_at_unix_ms
            .map(|deadline| {
                deadline
                    .saturating_sub(config.reminder_interval_ms)
                    .max(config.now_unix_ms)
            })
            .unwrap_or_else(|| {
                config
                    .now_unix_ms
                    .saturating_add(config.reminder_interval_ms)
            });
        Some(
            NotificationEscalationFact::new(
                item.item_id.clone(),
                severity,
                owner_id.clone(),
                source_event_ids.clone(),
                next_action.clone(),
                escalate_at,
                if unknown {
                    "unknown_requires_reconciliation"
                } else {
                    "deadline_or_attention_required"
                },
            )
            .map_err(NotificationPolicyError::Decision)?,
        )
    } else {
        None
    };
    NotificationPolicyDecision::new(
        item.item_id.clone(),
        severity,
        route,
        priority.digest_group.clone(),
        owner_id,
        source_event_ids,
        next_action,
        config.primary_channel,
        config.fallback_channel,
        config.quiet_hours,
        quiet_suppressed,
        reminder_at_unix_ms,
        deadline_at_unix_ms,
        escalation,
    )
    .map_err(NotificationPolicyError::Decision)
}

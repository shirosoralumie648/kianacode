//! Server-owned notification severity, digest and reminder/escalation policy facts.
//!
//! These contracts describe a bounded delivery decision. They do not send a message, mutate a
//! HumanTask, approve a command, retry a run or close an incident. Unknown/critical work remains
//! visible and carries an explicit escalation fact instead of being silently folded into a digest.

use crate::{json_digest, redact_text, NotificationChannel};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const NOTIFICATION_POLICY_SCHEMA: &str = "kiana.notification-policy.v1";
pub const NOTIFICATION_ESCALATION_SCHEMA: &str = "kiana.notification-escalation.v1";
pub const NOTIFICATION_POLICY_MAX_SOURCE_IDS: usize = 32;
pub const NOTIFICATION_POLICY_MIN_REMINDER_INTERVAL_MS: u64 = 60_000;
pub const NOTIFICATION_POLICY_MAX_REMINDER_INTERVAL_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
const DAY_MS: u64 = 24 * 60 * 60 * 1_000;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > max
        || value.contains(['\0', '\n', '\r'])
        || redact_text(value) != value
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest_valid(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn source_ids_valid(values: &[String]) -> Result<(), String> {
    if values.is_empty() || values.len() > NOTIFICATION_POLICY_MAX_SOURCE_IDS {
        return Err("notification_policy_source_ids_invalid".to_owned());
    }
    for value in values {
        required(value, "notification_policy_source_id", 512)?;
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("notification_policy_source_ids_noncanonical".to_owned());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationSeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationPolicyRoute {
    Immediate,
    SameDayReminder,
    Digest,
    Suppressed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuietHoursUtc {
    pub start_minute: u16,
    pub end_minute: u16,
}

impl QuietHoursUtc {
    pub fn new(start_minute: u16, end_minute: u16) -> Result<Self, String> {
        let value = Self {
            start_minute,
            end_minute,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.start_minute >= 1_440
            || self.end_minute >= 1_440
            || self.start_minute == self.end_minute
        {
            return Err("notification_quiet_hours_invalid".to_owned());
        }
        Ok(())
    }

    pub fn contains_minute(&self, minute: u16) -> bool {
        if self.start_minute < self.end_minute {
            minute >= self.start_minute && minute < self.end_minute
        } else {
            minute >= self.start_minute || minute < self.end_minute
        }
    }

    pub fn contains_unix_ms(&self, now_unix_ms: u64) -> bool {
        let minute = ((now_unix_ms % DAY_MS) / 60_000) as u16;
        self.contains_minute(minute)
    }

    pub fn end_unix_ms(&self, now_unix_ms: u64) -> u64 {
        let day = now_unix_ms / DAY_MS;
        let minute = ((now_unix_ms % DAY_MS) / 60_000) as u16;
        let wraps = self.start_minute > self.end_minute && minute >= self.start_minute;
        let target_day = if wraps { day.saturating_add(1) } else { day };
        target_day
            .saturating_mul(DAY_MS)
            .saturating_add(u64::from(self.end_minute) * 60_000)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationEscalationFact {
    pub schema: String,
    pub notification_id: String,
    pub severity: NotificationSeverity,
    pub owner_id: String,
    pub source_event_ids: Vec<String>,
    pub next_action: String,
    pub escalate_at_unix_ms: u64,
    pub reason: String,
    pub fact_digest: String,
}

impl NotificationEscalationFact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        notification_id: impl Into<String>,
        severity: NotificationSeverity,
        owner_id: impl Into<String>,
        source_event_ids: Vec<String>,
        next_action: impl Into<String>,
        escalate_at_unix_ms: u64,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let mut fact = Self {
            schema: NOTIFICATION_ESCALATION_SCHEMA.to_owned(),
            notification_id: notification_id.into(),
            severity,
            owner_id: owner_id.into(),
            source_event_ids,
            next_action: next_action.into(),
            escalate_at_unix_ms,
            reason: reason.into(),
            fact_digest: String::new(),
        };
        fact.fact_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_ESCALATION_SCHEMA
            || self.escalate_at_unix_ms == 0
            || !matches!(
                self.severity,
                NotificationSeverity::High | NotificationSeverity::Critical
            )
        {
            return Err("notification_escalation_header_invalid".to_owned());
        }
        required(
            &self.notification_id,
            "notification_escalation_notification",
            512,
        )?;
        required(&self.owner_id, "notification_escalation_owner", 512)?;
        required(
            &self.next_action,
            "notification_escalation_next_action",
            256,
        )?;
        required(&self.reason, "notification_escalation_reason", 256)?;
        source_ids_valid(&self.source_event_ids)?;
        digest_valid(&self.fact_digest, "notification_escalation_digest")?;
        if self.fact_digest != self.digest() {
            return Err("notification_escalation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "notification_id": self.notification_id,
            "severity": self.severity,
            "owner_id": self.owner_id,
            "source_event_ids": self.source_event_ids,
            "next_action": self.next_action,
            "escalate_at_unix_ms": self.escalate_at_unix_ms,
            "reason": self.reason,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationPolicyDecision {
    pub schema: String,
    pub notification_id: String,
    pub severity: NotificationSeverity,
    pub route: NotificationPolicyRoute,
    pub digest_group: String,
    pub owner_id: String,
    pub source_event_ids: Vec<String>,
    pub next_action: String,
    pub primary_channel: NotificationChannel,
    #[serde(default)]
    pub fallback_channel: Option<NotificationChannel>,
    #[serde(default)]
    pub quiet_hours: Option<QuietHoursUtc>,
    pub quiet_suppressed: bool,
    #[serde(default)]
    pub reminder_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub deadline_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub escalation: Option<NotificationEscalationFact>,
    pub decision_digest: String,
}

impl NotificationPolicyDecision {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        notification_id: impl Into<String>,
        severity: NotificationSeverity,
        route: NotificationPolicyRoute,
        digest_group: impl Into<String>,
        owner_id: impl Into<String>,
        source_event_ids: Vec<String>,
        next_action: impl Into<String>,
        primary_channel: NotificationChannel,
        fallback_channel: Option<NotificationChannel>,
        quiet_hours: Option<QuietHoursUtc>,
        quiet_suppressed: bool,
        reminder_at_unix_ms: Option<u64>,
        deadline_at_unix_ms: Option<u64>,
        escalation: Option<NotificationEscalationFact>,
    ) -> Result<Self, String> {
        let mut decision = Self {
            schema: NOTIFICATION_POLICY_SCHEMA.to_owned(),
            notification_id: notification_id.into(),
            severity,
            route,
            digest_group: digest_group.into(),
            owner_id: owner_id.into(),
            source_event_ids,
            next_action: next_action.into(),
            primary_channel,
            fallback_channel,
            quiet_hours,
            quiet_suppressed,
            reminder_at_unix_ms,
            deadline_at_unix_ms,
            escalation,
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision.validate()?;
        Ok(decision)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_POLICY_SCHEMA {
            return Err("notification_policy_schema_invalid".to_owned());
        }
        required(
            &self.notification_id,
            "notification_policy_notification",
            512,
        )?;
        required(&self.digest_group, "notification_policy_digest_group", 128)?;
        required(&self.owner_id, "notification_policy_owner", 512)?;
        required(&self.next_action, "notification_policy_next_action", 256)?;
        source_ids_valid(&self.source_event_ids)?;
        if let Some(quiet_hours) = self.quiet_hours {
            quiet_hours.validate()?;
        }
        if self.fallback_channel == Some(self.primary_channel) {
            return Err("notification_policy_fallback_duplicate".to_owned());
        }
        if matches!(self.severity, NotificationSeverity::Critical)
            && matches!(
                self.route,
                NotificationPolicyRoute::Digest | NotificationPolicyRoute::Suppressed
            )
        {
            return Err("notification_policy_critical_digest_forbidden".to_owned());
        }
        if matches!(self.severity, NotificationSeverity::High)
            && matches!(
                self.route,
                NotificationPolicyRoute::Digest | NotificationPolicyRoute::Suppressed
            )
        {
            return Err("notification_policy_high_digest_forbidden".to_owned());
        }
        if matches!(self.severity, NotificationSeverity::Low)
            && !matches!(self.route, NotificationPolicyRoute::Suppressed)
        {
            return Err("notification_policy_low_route_invalid".to_owned());
        }
        if matches!(self.severity, NotificationSeverity::Critical) && self.quiet_suppressed {
            return Err("notification_policy_critical_quiet_suppressed".to_owned());
        }
        if self.quiet_suppressed && self.quiet_hours.is_none() {
            return Err("notification_policy_quiet_hours_missing".to_owned());
        }
        if matches!(self.route, NotificationPolicyRoute::Suppressed)
            && self.reminder_at_unix_ms.is_some()
        {
            return Err("notification_policy_suppressed_reminder_invalid".to_owned());
        }
        if self.reminder_at_unix_ms == Some(0) || self.deadline_at_unix_ms == Some(0) {
            return Err("notification_policy_time_invalid".to_owned());
        }
        if let Some(escalation) = &self.escalation {
            escalation.validate()?;
            if escalation.notification_id != self.notification_id
                || escalation.owner_id != self.owner_id
                || escalation.source_event_ids != self.source_event_ids
                || escalation.next_action != self.next_action
            {
                return Err("notification_policy_escalation_binding_invalid".to_owned());
            }
        }
        digest_valid(&self.decision_digest, "notification_policy_digest")?;
        if self.decision_digest != self.digest() {
            return Err("notification_policy_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "notification_id": self.notification_id,
            "severity": self.severity,
            "route": self.route,
            "digest_group": self.digest_group,
            "owner_id": self.owner_id,
            "source_event_ids": self.source_event_ids,
            "next_action": self.next_action,
            "primary_channel": self.primary_channel,
            "fallback_channel": self.fallback_channel,
            "quiet_hours": self.quiet_hours,
            "quiet_suppressed": self.quiet_suppressed,
            "reminder_at_unix_ms": self.reminder_at_unix_ms,
            "deadline_at_unix_ms": self.deadline_at_unix_ms,
            "escalation": self.escalation,
        }))
    }
}

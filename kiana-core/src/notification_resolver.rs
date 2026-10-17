//! Server-context notification recipient/scope/subscription resolution.
//!
//! Delivery recipients, project binding, role, project trust and authority epoch are derived
//! exclusively from a validated [`SecurityAuthoritySnapshot`] (Principal + resolved Assignment +
//! ProjectTrust + department + authority epoch). Client-supplied owner/project/recipient/role and
//! filter values are treated as untrusted claims: they may only ever equal — never widen — the
//! server context, and a mismatch is rejected with no delivery. Subscriptions can only narrow the
//! server maximum scope and channel set; resolution is a pure function of the server snapshot, the
//! notification and the stored subscriptions, so the same subject receives the same visible set no
//! matter which of the CLI/Workbench/Web entry points asked. This module never dispatches, never
//! calls a broker and never constructs an authority identity from wire input.

use crate::SecurityAuthoritySnapshot;
use kiana_domain::{
    Notification, NotificationChannel, ProjectId, RequestContext, Subscription, SubscriptionId,
    SubscriptionStatus, MAX_NOTIFICATION_CHANNELS, MAX_NOTIFICATION_SCOPE,
    MAX_NOTIFICATION_TEXT_BYTES,
};

/// Schema name for a resolved notification delivery set.
pub const NOTIFICATION_RESOLUTION_SCHEMA: &str = "kiana.notification-resolution.v1";

/// Which client surface asked for the visible set.
///
/// The label is recorded for parity audits and must never change which targets are returned: the
/// same principal resolving the same notification and subscriptions gets the same set from every
/// entry point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationEntryPoint {
    Cli,
    Workbench,
    Web,
}

/// Untrusted values a client may assert on the wire.
///
/// None of these are authority. The server only accepts a claim that already equals the resolved
/// value; any wider owner/project/recipient/role/filter/channel is rejected so a client cannot
/// expand its own visibility.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NotificationClientClaims {
    /// Client-asserted owner label; must equal the authenticated principal when present.
    pub owner: Option<String>,
    /// Client-asserted project; must equal the resolved assignment project when present.
    pub project_id: Option<ProjectId>,
    /// Client-asserted recipient; must equal the authenticated principal when present.
    pub recipient: Option<String>,
    /// Client-asserted role; must equal the resolved assignment role when present.
    pub role_id: Option<String>,
    /// Client-requested scope filter; each entry must stay inside the server maximum scope.
    pub filter_scope: Vec<String>,
    /// Client-requested channels; each entry must stay inside the server maximum channel set.
    pub channels: Vec<NotificationChannel>,
}

/// A single resolved delivery target for one subscription.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedNotificationTarget {
    /// The subscription that authorized this delivery.
    pub subscription_id: SubscriptionId,
    /// Revision of the authorizing subscription.
    pub subscription_revision: u64,
    /// Channel to deliver on (the notification channel, already inside the narrowing set).
    pub channel: NotificationChannel,
    /// Narrowed scope actually delivered; always a subset of the server maximum scope.
    pub scope: Vec<String>,
}

/// The deterministic visible delivery set for one notification and recipient.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedNotification {
    /// Fixed schema name.
    pub schema: String,
    /// Authenticated recipient derived from the server authority, never from client input.
    pub recipient_id: String,
    /// Project binding derived from the resolved assignment.
    pub project_id: ProjectId,
    /// Authority epoch under which the resolution was valid.
    pub authority_epoch: u64,
    /// Sorted, de-duplicated delivery targets.
    pub targets: Vec<ResolvedNotificationTarget>,
}

/// Server-owned resolver binding an authority snapshot to a bounded maximum scope/channel set.
#[derive(Clone, Debug, PartialEq)]
pub struct NotificationResolver {
    authority: SecurityAuthoritySnapshot,
    max_scope: Vec<String>,
    max_channels: Vec<NotificationChannel>,
}

impl NotificationResolver {
    /// Build a resolver from a validated authority snapshot and the server maximum scope/channels.
    ///
    /// The maximum scope/channels are the widest values the server is ever willing to deliver for
    /// this subject; every subscription and client filter must narrow them.
    pub fn new(
        authority: SecurityAuthoritySnapshot,
        max_scope: Vec<String>,
        max_channels: Vec<NotificationChannel>,
    ) -> Result<Self, String> {
        authority.validate()?;
        let max_scope = canonical_scope(max_scope, "resolver_max_scope")?;
        let max_channels = canonical_channels(max_channels, "resolver_max_channels")?;
        if !channels_subset(&[NotificationChannel::InApp], &max_channels) {
            return Err("resolver_in_app_channel_required".to_owned());
        }
        Ok(Self {
            authority,
            max_scope,
            max_channels,
        })
    }

    /// Authenticated recipient derived from the server authority.
    pub fn recipient_id(&self) -> &str {
        &self.authority.principal.principal_id
    }

    /// Project binding derived from the resolved assignment.
    pub fn project_id(&self) -> ProjectId {
        self.authority.assignment.project_id
    }

    /// Authority epoch carried by the server snapshot.
    pub fn authority_epoch(&self) -> u64 {
        self.authority.authority_epoch
    }

    /// Server maximum scope.
    pub fn max_scope(&self) -> &[String] {
        &self.max_scope
    }

    /// Server maximum channel set.
    pub fn max_channels(&self) -> &[NotificationChannel] {
        &self.max_channels
    }

    /// Resolve the visible delivery targets for one notification.
    ///
    /// Rejection order mirrors the security constitution: trust, authority epoch, assignment
    /// window, client claims, notification binding, then subscription narrowing. Any error returns
    /// `Ok` with no targets, so a rejected request never produces a delivery.
    #[allow(clippy::too_many_arguments)]
    pub fn resolve(
        &self,
        entry: NotificationEntryPoint,
        claims: &NotificationClientClaims,
        notification: &Notification,
        subscriptions: &[Subscription],
        now_unix_ms: u64,
        server_epoch: u64,
    ) -> Result<ResolvedNotification, String> {
        self.authority.validate()?;
        self.authority.require_trusted_for_effect()?;
        if server_epoch == 0 {
            return Err("notification_epoch_invalid".to_owned());
        }
        if server_epoch < self.authority.authority_epoch {
            return Err("notification_epoch_rollback".to_owned());
        }
        if server_epoch != self.authority.authority_epoch {
            return Err("notification_epoch_stale".to_owned());
        }
        let assignment = &self.authority.assignment;
        if now_unix_ms < assignment.valid_from_unix_ms
            || now_unix_ms >= assignment.expires_at_unix_ms
        {
            return Err("notification_authority_expired".to_owned());
        }
        let effective_scope = self.validate_claims(claims)?;
        notification.validate()?;
        if notification.recipient_id != self.recipient_id() {
            return Err("notification_recipient_not_authorized".to_owned());
        }
        if notification.project_id != Some(self.project_id()) {
            return Err("notification_project_not_authorized".to_owned());
        }
        let effective_channels = if claims.channels.is_empty() {
            self.max_channels.clone()
        } else {
            claims.channels.clone()
        };
        let mut targets = Vec::new();
        for subscription in subscriptions {
            subscription.validate()?;
            if subscription.recipient_id != self.recipient_id()
                || subscription.project_id != Some(self.project_id())
            {
                continue;
            }
            if subscription.status != SubscriptionStatus::Active
                || subscription.expires_at_unix_ms <= now_unix_ms
            {
                continue;
            }
            if !scope_is_subset(&subscription.scope, &effective_scope) {
                return Err("notification_subscription_scope_widening".to_owned());
            }
            if !channels_subset(&subscription.channels, &effective_channels) {
                return Err("notification_subscription_channel_widening".to_owned());
            }
            if subscription.revision != notification.subscription_revision {
                continue;
            }
            notification.validate_for_subscription(subscription)?;
            targets.push(ResolvedNotificationTarget {
                subscription_id: subscription.subscription_id,
                subscription_revision: subscription.revision,
                channel: notification.channel,
                scope: notification.scope.clone(),
            });
        }
        if targets.is_empty() {
            return Err("notification_recipient_subscription_not_found".to_owned());
        }
        targets.sort_by(|a, b| a.subscription_id.cmp(&b.subscription_id));
        targets.dedup();
        // The entry point is an audit label only; it must never widen or narrow the visible set.
        let _ = entry;
        Ok(ResolvedNotification {
            schema: NOTIFICATION_RESOLUTION_SCHEMA.to_owned(),
            recipient_id: self.recipient_id().to_owned(),
            project_id: self.project_id(),
            authority_epoch: self.authority.authority_epoch,
            targets,
        })
    }

    /// Reject any client claim that widens the server context and return the narrowed scope.
    fn validate_claims(&self, claims: &NotificationClientClaims) -> Result<Vec<String>, String> {
        if let Some(owner) = &claims.owner {
            if owner != self.recipient_id() {
                return Err("notification_owner_widening".to_owned());
            }
        }
        if let Some(project_id) = claims.project_id {
            if project_id != self.project_id() {
                return Err("notification_project_widening".to_owned());
            }
        }
        if let Some(recipient) = &claims.recipient {
            if recipient != self.recipient_id() {
                return Err("notification_recipient_widening".to_owned());
            }
        }
        if let Some(role_id) = &claims.role_id {
            if role_id != &self.authority.assignment.role_id {
                return Err("notification_role_self_report".to_owned());
            }
        }
        for item in &claims.filter_scope {
            if !scope_is_subset(std::slice::from_ref(item), &self.max_scope) {
                return Err("notification_filter_widening".to_owned());
            }
        }
        if !claims.channels.is_empty() && !channels_subset(&claims.channels, &self.max_channels) {
            return Err("notification_channel_widening".to_owned());
        }
        if claims.filter_scope.is_empty() {
            Ok(self.max_scope.clone())
        } else {
            canonical_scope(claims.filter_scope.clone(), "notification_filter_scope")
        }
    }
}

/// Resolve delivery targets from authenticated server context. Caller-supplied recipient/project
/// values are not accepted; subscriptions may only narrow the already-authorized notification.
pub fn resolve_notification_subscriptions(
    context: &RequestContext,
    server_project_id: Option<kiana_domain::ProjectId>,
    notification: &Notification,
    subscriptions: &[Subscription],
    now_unix_ms: u64,
) -> Result<Vec<Subscription>, String> {
    notification.validate()?;
    if !context.project_trusted {
        return Err("notification_project_untrusted".to_owned());
    }
    let actor = context
        .actor_id
        .as_deref()
        .filter(|actor| !actor.trim().is_empty())
        .ok_or_else(|| "notification_actor_required".to_owned())?;
    if now_unix_ms == 0 {
        return Err("notification_clock_invalid".to_owned());
    }
    let resolved = subscriptions
        .iter()
        .filter(|subscription| {
            subscription.recipient_id == actor
                && subscription.project_id == server_project_id
                && subscription.expires_at_unix_ms > now_unix_ms
        })
        .filter_map(|subscription| {
            notification
                .validate_for_subscription(subscription)
                .ok()
                .map(|_| subscription.clone())
        })
        .collect::<Vec<_>>();
    if resolved.is_empty() {
        return Err("notification_recipient_subscription_not_found".to_owned());
    }
    Ok(resolved)
}

fn canonical_scope(mut values: Vec<String>, field: &str) -> Result<Vec<String>, String> {
    if values.is_empty() {
        return Err(format!("{field}_required"));
    }
    if values.len() > MAX_NOTIFICATION_SCOPE {
        return Err(format!("{field}_too_large"));
    }
    for value in &mut values {
        *value = value.trim().to_owned();
        if value.is_empty() || value.len() > MAX_NOTIFICATION_TEXT_BYTES || value.contains('\0') {
            return Err(format!("{field}_invalid"));
        }
    }
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(format!("{field}_duplicate"));
    }
    Ok(values)
}

fn canonical_channels(
    mut channels: Vec<NotificationChannel>,
    field: &str,
) -> Result<Vec<NotificationChannel>, String> {
    if channels.is_empty() || channels.len() > MAX_NOTIFICATION_CHANNELS {
        return Err(format!("{field}_invalid"));
    }
    channels.sort();
    if channels.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(format!("{field}_duplicate"));
    }
    Ok(channels)
}

fn scope_is_subset(requested: &[String], granted: &[String]) -> bool {
    requested.iter().all(|item| {
        granted.iter().any(|candidate| {
            candidate == "*" || candidate == item || item.starts_with(&format!("{candidate}/"))
        })
    })
}

fn channels_subset(requested: &[NotificationChannel], granted: &[NotificationChannel]) -> bool {
    requested.iter().all(|channel| granted.contains(channel))
}

//! Server-context notification recipient/scope/subscription resolution.

use kiana_domain::{Notification, RequestContext, Subscription};

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

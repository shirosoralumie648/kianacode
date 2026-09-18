use kiana_core::resolve_notification_subscriptions;
use kiana_domain::{
    MessageId, Notification, NotificationChannel, ProjectId, RequestContext, Subscription,
};

fn notification(project_id: Option<ProjectId>) -> Notification {
    Notification::new(
        MessageId::new(),
        "actor-1",
        project_id,
        vec!["global".to_owned()],
        NotificationChannel::InApp,
        1,
        10_000,
        1,
        None,
    )
    .unwrap()
}

#[test]
fn resolver_uses_server_actor_project_and_subscription_scope() {
    let mut context = RequestContext::local("nm05", "/project");
    context.project_trusted = true;
    context.actor_id = Some("actor-1".to_owned());
    let project_id = ProjectId::new();
    let subscription = Subscription::new(
        "actor-1",
        Some(project_id),
        vec!["global".to_owned()],
        vec![NotificationChannel::InApp],
        1,
        1,
        10_000,
    )
    .unwrap();
    let resolved = resolve_notification_subscriptions(
        &context,
        Some(project_id),
        &notification(Some(project_id)),
        &[subscription.clone()],
        2_000,
    )
    .unwrap();
    assert_eq!(resolved, vec![subscription]);
}

#[test]
fn resolver_rejects_untrusted_or_client_scope_mismatch() {
    let context = RequestContext::local("nm05", "/project");
    let project_id = ProjectId::new();
    let subscription = Subscription::new(
        "actor-1",
        Some(project_id),
        vec!["global".to_owned()],
        vec![NotificationChannel::InApp],
        1,
        1,
        10_000,
    )
    .unwrap();
    assert_eq!(
        resolve_notification_subscriptions(
            &context,
            Some(project_id),
            &notification(Some(project_id)),
            &[subscription.clone()],
            2_000,
        )
        .unwrap_err(),
        "notification_project_untrusted"
    );
    let mut trusted = context;
    trusted.project_trusted = true;
    trusted.actor_id = Some("attacker".to_owned());
    assert_eq!(
        resolve_notification_subscriptions(
            &trusted,
            Some(project_id),
            &notification(Some(project_id)),
            &[subscription],
            2_000,
        )
        .unwrap_err(),
        "notification_recipient_subscription_not_found"
    );
}

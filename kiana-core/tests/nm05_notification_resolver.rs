use kiana_core::{
    resolve_notification_subscriptions, NotificationClientClaims, NotificationEntryPoint,
    NotificationResolver, ResolvedNotificationTarget, SecurityAuthoritySnapshot,
};
use kiana_domain::{
    json_digest, AssignmentDirectory, AssignmentId, AuthenticatedPrincipalRef, DepartmentSnapshot,
    DepartmentSpec, MessageId, Notification, NotificationChannel, OrganizationId, ProjectAssignment,
    ProjectAssignmentId, ProjectId, ProjectIdentity, ProjectTrustSnapshot, RequestContext,
    RoleAssignment, RoleAssignmentStatus, SecurityContextId, Subscription, SubscriptionStatus,
};
use serde_json::json;

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

// The assignment window is wide enough that each test can pick its own `now` without tripping the
// authority-expiry check before the check under test.
const AUTHORITY_EPOCH: u64 = 2;

fn authority_snapshot(trusted: bool) -> SecurityAuthoritySnapshot {
    let principal = AuthenticatedPrincipalRef::local();
    let project = ProjectIdentity::new(
        "/repo",
        "/repo",
        None,
        None,
        json_digest(&json!({"trusted": trusted})),
    )
    .unwrap();
    let organization_id = OrganizationId::new();
    let assignment_id = AssignmentId::new();
    let role = RoleAssignment::new(
        assignment_id,
        principal.clone(),
        organization_id,
        "builder",
        "executing",
        vec![project.project_id],
        1,
        100_000_000,
        RoleAssignmentStatus::Active,
        1,
        AUTHORITY_EPOCH,
    )
    .unwrap();
    let project_assignment = ProjectAssignment::new(
        ProjectAssignmentId::new(),
        assignment_id,
        principal.clone(),
        organization_id,
        project.project_id,
        1,
        100_000_000,
        false,
        1,
        AUTHORITY_EPOCH,
    )
    .unwrap();
    let mut directory = AssignmentDirectory::new();
    directory.register_role(role).unwrap();
    directory.register_project(project_assignment).unwrap();
    let resolved = directory
        .resolve(
            &principal,
            organization_id,
            project.project_id,
            "builder",
            100,
        )
        .unwrap();
    let trust = ProjectTrustSnapshot::from_project(&project, trusted, "fixture", 1).unwrap();
    let department = DepartmentSnapshot::from_spec(&DepartmentSpec::executing(), 1, AUTHORITY_EPOCH)
        .unwrap();
    SecurityAuthoritySnapshot::from_parts(
        SecurityContextId::new(),
        principal,
        trust,
        resolved,
        department,
        AUTHORITY_EPOCH,
    )
    .unwrap()
}

fn resolver_fixture(
    trusted: bool,
    max_scope: Vec<String>,
    max_channels: Vec<NotificationChannel>,
) -> NotificationResolver {
    NotificationResolver::new(authority_snapshot(trusted), max_scope, max_channels).unwrap()
}

fn full_channels() -> Vec<NotificationChannel> {
    vec![
        NotificationChannel::InApp,
        NotificationChannel::RunStream,
        NotificationChannel::Web,
        NotificationChannel::Workbench,
        NotificationChannel::Cli,
    ]
}

fn bound_notification(resolver: &NotificationResolver) -> Notification {
    Notification::new(
        MessageId::new(),
        resolver.recipient_id(),
        Some(resolver.project_id()),
        vec!["global".to_owned()],
        NotificationChannel::InApp,
        10,
        10_000,
        1,
        None,
    )
    .unwrap()
}

fn bound_subscription(resolver: &NotificationResolver, scope: Vec<String>) -> Subscription {
    Subscription::new(
        resolver.recipient_id(),
        Some(resolver.project_id()),
        scope,
        vec![NotificationChannel::InApp],
        1,
        1,
        10_000,
    )
    .unwrap()
}

fn revoke(mut subscription: Subscription) -> Subscription {
    subscription.status = SubscriptionStatus::Revoked;
    subscription.subscription_digest = subscription.digest();
    subscription
}

#[test]
fn resolver_derives_recipient_and_rejects_client_widening() {
    let resolver = resolver_fixture(
        true,
        vec!["global".to_owned(), "project".to_owned()],
        vec![
            NotificationChannel::InApp,
            NotificationChannel::Web,
            NotificationChannel::Workbench,
            NotificationChannel::Cli,
        ],
    );
    let note = bound_notification(&resolver);
    let subscription = bound_subscription(&resolver, vec!["global".to_owned()]);
    let claims = NotificationClientClaims::default();
    let resolved = resolver
        .resolve(
            NotificationEntryPoint::Cli,
            &claims,
            &note,
            &[subscription.clone()],
            100,
            AUTHORITY_EPOCH,
        )
        .unwrap();
    assert_eq!(resolved.recipient_id, resolver.recipient_id());
    assert_eq!(resolved.project_id, resolver.project_id());
    assert_eq!(resolved.targets.len(), 1);

    let wider = |claims: NotificationClientClaims| {
        resolver
            .resolve(
                NotificationEntryPoint::Cli,
                &claims,
                &note,
                &[subscription.clone()],
                100,
                AUTHORITY_EPOCH,
            )
            .unwrap_err()
    };
    assert_eq!(
        wider(NotificationClientClaims {
            owner: Some("attacker".to_owned()),
            ..Default::default()
        }),
        "notification_owner_widening"
    );
    assert_eq!(
        wider(NotificationClientClaims {
            project_id: Some(ProjectId::new()),
            ..Default::default()
        }),
        "notification_project_widening"
    );
    assert_eq!(
        wider(NotificationClientClaims {
            recipient: Some("attacker".to_owned()),
            ..Default::default()
        }),
        "notification_recipient_widening"
    );
    assert_eq!(
        wider(NotificationClientClaims {
            role_id: Some("reviewer".to_owned()),
            ..Default::default()
        }),
        "notification_role_self_report"
    );
    assert_eq!(
        wider(NotificationClientClaims {
            filter_scope: vec!["secret".to_owned()],
            ..Default::default()
        }),
        "notification_filter_widening"
    );
    assert_eq!(
        wider(NotificationClientClaims {
            channels: vec![NotificationChannel::RunStream],
            ..Default::default()
        }),
        "notification_channel_widening"
    );
    // A claim that equals the server context is accepted and does not widen anything.
    let accepted = resolver
        .resolve(
            NotificationEntryPoint::Cli,
            &NotificationClientClaims {
                owner: Some(resolver.recipient_id().to_owned()),
                project_id: Some(resolver.project_id()),
                recipient: Some(resolver.recipient_id().to_owned()),
                role_id: Some("builder".to_owned()),
                filter_scope: vec!["global".to_owned()],
                channels: vec![NotificationChannel::InApp],
            },
            &note,
            &[subscription.clone()],
            100,
            AUTHORITY_EPOCH,
        )
        .unwrap();
    assert_eq!(accepted.targets.len(), 1);
}

#[test]
fn resolver_entrypoints_share_visible_set() {
    let resolver = resolver_fixture(
        true,
        vec!["global".to_owned(), "project".to_owned()],
        full_channels(),
    );
    let note = bound_notification(&resolver);
    let subscriptions = vec![
        bound_subscription(&resolver, vec!["global".to_owned()]),
        bound_subscription(&resolver, vec!["global".to_owned(), "project".to_owned()]),
    ];
    let claims = NotificationClientClaims::default();
    let cli = resolver
        .resolve(
            NotificationEntryPoint::Cli,
            &claims,
            &note,
            &subscriptions,
            100,
            AUTHORITY_EPOCH,
        )
        .unwrap();
    let workbench = resolver
        .resolve(
            NotificationEntryPoint::Workbench,
            &claims,
            &note,
            &subscriptions,
            100,
            AUTHORITY_EPOCH,
        )
        .unwrap();
    let web = resolver
        .resolve(
            NotificationEntryPoint::Web,
            &claims,
            &note,
            &subscriptions,
            100,
            AUTHORITY_EPOCH,
        )
        .unwrap();
    assert_eq!(cli, workbench);
    assert_eq!(cli, web);
    assert_eq!(cli.targets.len(), 2);
}

#[test]
fn resolver_rejects_untrusted_expired_epoch_and_cross_project() {
    let untrusted = resolver_fixture(
        false,
        vec!["global".to_owned()],
        vec![NotificationChannel::InApp],
    );
    let note = bound_notification(&untrusted);
    let claims = NotificationClientClaims::default();
    assert_eq!(
        untrusted
            .resolve(
                NotificationEntryPoint::Cli,
                &claims,
                &note,
                &[],
                100,
                AUTHORITY_EPOCH,
            )
            .unwrap_err(),
        "AUTH_PROJECT_UNTRUSTED"
    );

    let resolver = resolver_fixture(
        true,
        vec!["global".to_owned()],
        vec![NotificationChannel::InApp],
    );
    assert_eq!(
        resolver
            .resolve(
                NotificationEntryPoint::Cli,
                &claims,
                &note,
                &[],
                100,
                0,
            )
            .unwrap_err(),
        "notification_epoch_invalid"
    );
    assert_eq!(
        resolver
            .resolve(
                NotificationEntryPoint::Cli,
                &claims,
                &note,
                &[],
                100,
                AUTHORITY_EPOCH - 1,
            )
            .unwrap_err(),
        "notification_epoch_rollback"
    );
    assert_eq!(
        resolver
            .resolve(
                NotificationEntryPoint::Cli,
                &claims,
                &note,
                &[],
                100,
                AUTHORITY_EPOCH + 1,
            )
            .unwrap_err(),
        "notification_epoch_stale"
    );
    assert_eq!(
        resolver
            .resolve(
                NotificationEntryPoint::Cli,
                &claims,
                &note,
                &[],
                100_000_000,
                AUTHORITY_EPOCH,
            )
            .unwrap_err(),
        "notification_authority_expired"
    );

    let foreign = Notification::new(
        MessageId::new(),
        resolver.recipient_id(),
        Some(ProjectId::new()),
        vec!["global".to_owned()],
        NotificationChannel::InApp,
        10,
        10_000,
        1,
        None,
    )
    .unwrap();
    assert_eq!(
        resolver
            .resolve(
                NotificationEntryPoint::Cli,
                &claims,
                &foreign,
                &[],
                100,
                AUTHORITY_EPOCH,
            )
            .unwrap_err(),
        "notification_project_not_authorized"
    );

    let wrong_recipient = Notification::new(
        MessageId::new(),
        "attacker",
        Some(resolver.project_id()),
        vec!["global".to_owned()],
        NotificationChannel::InApp,
        10,
        10_000,
        1,
        None,
    )
    .unwrap();
    assert_eq!(
        resolver
            .resolve(
                NotificationEntryPoint::Cli,
                &claims,
                &wrong_recipient,
                &[],
                100,
                AUTHORITY_EPOCH,
            )
            .unwrap_err(),
        "notification_recipient_not_authorized"
    );
}

#[test]
fn resolver_subscription_narrows_and_rebuilds_on_revision() {
    let resolver = resolver_fixture(
        true,
        vec!["global".to_owned(), "project".to_owned()],
        vec![NotificationChannel::InApp, NotificationChannel::Cli],
    );
    let note = bound_notification(&resolver);
    let claims = NotificationClientClaims::default();
    let subscription = bound_subscription(&resolver, vec!["global".to_owned()]);

    // A subscription narrower than the server maximum resolves and delivers the notification scope.
    let first = resolver
        .resolve(
            NotificationEntryPoint::Cli,
            &claims,
            &note,
            &[subscription.clone()],
            100,
            AUTHORITY_EPOCH,
        )
        .unwrap();
    assert_eq!(
        first.targets,
        vec![ResolvedNotificationTarget {
            subscription_id: subscription.subscription_id,
            subscription_revision: 1,
            channel: NotificationChannel::InApp,
            scope: vec!["global".to_owned()],
        }]
    );

    // A subscription wider than the server maximum is rejected, not silently narrowed.
    assert_eq!(
        resolver
            .resolve(
                NotificationEntryPoint::Cli,
                &claims,
                &note,
                &[bound_subscription(&resolver, vec!["secret".to_owned()])],
                100,
                AUTHORITY_EPOCH,
            )
            .unwrap_err(),
        "notification_subscription_scope_widening"
    );

    // A revoked subscription is excluded with no delivery.
    assert_eq!(
        resolver
            .resolve(
                NotificationEntryPoint::Cli,
                &claims,
                &note,
                &[revoke(subscription.clone())],
                100,
                AUTHORITY_EPOCH,
            )
            .unwrap_err(),
        "notification_recipient_subscription_not_found"
    );

    // An expired subscription is excluded with no delivery.
    let mut expired = subscription.clone();
    expired.expires_at_unix_ms = 5_000;
    expired.subscription_digest = expired.digest();
    assert_eq!(
        resolver
            .resolve(
                NotificationEntryPoint::Cli,
                &claims,
                &note,
                &[expired],
                6_000,
                AUTHORITY_EPOCH,
            )
            .unwrap_err(),
        "notification_recipient_subscription_not_found"
    );

    // A stale revision does not match the notification and is excluded.
    let mut stale = subscription.clone();
    stale.revision = 2;
    stale.subscription_digest = stale.digest();
    assert_eq!(
        resolver
            .resolve(
                NotificationEntryPoint::Cli,
                &claims,
                &note,
                &[stale],
                100,
                AUTHORITY_EPOCH,
            )
            .unwrap_err(),
        "notification_recipient_subscription_not_found"
    );

    // Same inputs rebuild the exact same visible set; once the old revision is revoked only the new
    // active revision remains visible.
    let rebuilt = resolver
        .resolve(
            NotificationEntryPoint::Cli,
            &claims,
            &note,
            &[subscription.clone()],
            100,
            AUTHORITY_EPOCH,
        )
        .unwrap();
    assert_eq!(first, rebuilt);
    let replacement = bound_subscription(&resolver, vec!["global".to_owned()]);
    let after = resolver
        .resolve(
            NotificationEntryPoint::Cli,
            &claims,
            &note,
            &[revoke(subscription), replacement.clone()],
            100,
            AUTHORITY_EPOCH,
        )
        .unwrap();
    assert_eq!(after.targets.len(), 1);
    assert_eq!(after.targets[0].subscription_id, replacement.subscription_id);
}

#[test]
fn resolver_requires_in_app_channel_in_server_maximum() {
    // A server maximum channel set without in-app delivery is rejected outright.
    assert_eq!(
        NotificationResolver::new(
            authority_snapshot(true),
            vec!["global".to_owned()],
            vec![NotificationChannel::Cli],
        )
        .unwrap_err(),
        "resolver_in_app_channel_required"
    );
    // An in-app-only maximum is accepted.
    let resolver = resolver_fixture(
        true,
        vec!["global".to_owned()],
        vec![NotificationChannel::InApp],
    );
    assert_eq!(resolver.max_channels(), &[NotificationChannel::InApp]);
}

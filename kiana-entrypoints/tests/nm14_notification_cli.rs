use kiana_entrypoints::notification_cli::{
    parse_cli_inbox_line, plan_cli_detach, plan_cli_inbox_intent, present_notification_page,
    present_run_status, present_session_badges, render_badge_lines, render_inbox_lines,
    CliApprovalBinding, CliInboxIntent, CliInboxPlan, CliInboxRoute, CliInboxVerb, CliRunStatus,
    CLI_INBOX_ACTION_NOT_IN_PROJECTION, CLI_INBOX_ACTION_UNKNOWN, CLI_INBOX_DECISION_NOT_OFFERED,
    CLI_INBOX_DISCONNECT_IS_NOT_CANCEL, CLI_INBOX_LINE_INVALID, CLI_INBOX_PAGE_DIGEST_REQUIRED,
    CLI_INBOX_PLAN_SCHEMA, CLI_INBOX_READ_IS_NOT_CONSUME, CLI_INBOX_SCOPE_FORBIDDEN,
    CLI_INBOX_SERVER_TIME_REQUIRED, CLI_INBOX_SESSIONS_LIMIT, CLI_INBOX_SESSION_DUPLICATE,
    CLI_INBOX_SESSION_MISMATCH, CLI_INBOX_TARGET_INVALID, CLI_NOTIFICATION_CLIENT_HINT,
    CLI_NOTIFICATION_PAGE_DIGEST_INVALID, CLI_SESSION_BADGE_SCHEMA,
};
use kiana_protocol::{
    UiCursor, UiFeedCursorV1, UiFeedFrameKind, UiFeedFrameV1, UI_FEED_FRAME_SCHEMA,
};
use serde_json::json;

const PAGE_DIGEST: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const REQUEST_HASH: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const SERVER_TIME_UNIX_MS: u64 = 1_700_000_000_000;

fn page() -> serde_json::Value {
    json!({
        "schema":"kiana.notification-page.v1",
        "source_cursor":12,
        "items":[{
            "item":{
                "item_id":"notification:14",
                "kind":"approval",
                "title":"Approval required",
                "source_ref":"event:14",
                "detail":{"secret":"must not cross presenter","due_at_unix_ms":30,"source_cursor":12},
                "actions":[{"id":"approve","command":"notification.approve","arguments":{"token":"raw"}}]
            },
            "urgency":"high",
            "due_at_unix_ms":30,
            "read":false,
            "acknowledged":false,
            "snoozed_until_unix_ms":null
        }],
        "next_after_item_id":null,
        "page_digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"
    })
}

fn page_with_items(
    items: serde_json::Value,
    server_time_unix_ms: Option<u64>,
) -> serde_json::Value {
    json!({
        "schema":"kiana.notification-page.v1",
        "source_cursor":12,
        "server_time_unix_ms":server_time_unix_ms,
        "items":items,
        "next_after_item_id":null,
        "page_digest":PAGE_DIGEST
    })
}

fn inbox_page() -> serde_json::Value {
    page_with_items(
        json!([
            {
                "item":{
                    "item_id":"notification:14",
                    "kind":"approval",
                    "title":"Approval required",
                    "source_ref":"event:14",
                    "detail":{"secret":"must not cross presenter"},
                    "actions":[
                        {"id":"ack","command":"notification.acknowledge","arguments":{"token":"raw"}},
                        {"id":"review","command":"notification.review","arguments":{}}
                    ]
                },
                "urgency":"critical",
                "due_at_unix_ms":30,
                "read":false,
                "acknowledged":false,
                "snoozed_until_unix_ms":null,
                "unknown":false
            },
            {
                "item":{
                    "item_id":"notification:15",
                    "kind":"reconciliation",
                    "title":"Reconcile delivery",
                    "source_ref":"event:15",
                    "detail":{},
                    "actions":[
                        {"id":"reconcile","command":"notification.reconcile","arguments":{}}
                    ]
                },
                "urgency":"high",
                "due_at_unix_ms":60,
                "read":true,
                "acknowledged":true,
                "snoozed_until_unix_ms":99,
                "unknown":true
            }
        ]),
        Some(SERVER_TIME_UNIX_MS),
    )
}

/// The server-issued pending-approval listing this CLI session received.
fn listing() -> serde_json::Value {
    json!({
        "approvals":[{
            "challenge":{
                "schema":"kiana.approval-challenge.v1",
                "approval_id":"approval:1",
                "request_id":"request:1",
                "request_hash":REQUEST_HASH,
                "risk":"high",
                "expires_at_unix_ms":SERVER_TIME_UNIX_MS + 60_000,
                "reason":"capability escalation",
                "nonce":"nonce:1",
                "policy_version":"policy-v1"
            },
            "available_decisions":["approve","deny"]
        }]
    })
}

fn deny_only_listing() -> serde_json::Value {
    json!({
        "approvals":[{
            "challenge":{
                "approval_id":"approval:1",
                "request_hash":REQUEST_HASH,
                "nonce":""
            },
            "available_decisions":["deny"]
        }]
    })
}

fn frame(kind: UiFeedFrameKind, terminal: bool) -> UiFeedFrameV1 {
    UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        kind,
        cursor: UiFeedCursorV1::new(
            "instance-14",
            "epoch-14",
            1,
            UiCursor {
                epoch: "epoch-14".to_owned(),
                sequence: 12,
            },
        )
        .unwrap(),
        event_id: "event-14".to_owned(),
        replay: false,
        terminal,
        event: None,
        gap: None,
    }
}

#[test]
fn notification_cli_refuses_forged_scope_before_binding_any_action() {
    for line in [
        "/approve scope:session-a approval:1",
        "/ack notification:14 key=value",
        "/read notification:14 extra",
        "/inbox session:other",
        "/detach scope:session-a",
        "/deny --force",
        "/snooze sandbox:workspace-write",
    ] {
        let error = parse_cli_inbox_line(line).unwrap_err();
        assert!(
            error.contains(CLI_INBOX_SCOPE_FORBIDDEN),
            "forged scope must be refused: {line} -> {error}"
        );
    }
    assert_eq!(
        parse_cli_inbox_line("/").unwrap_err(),
        CLI_INBOX_LINE_INVALID
    );
    let unknown = parse_cli_inbox_line("/promote notification:14").unwrap_err();
    assert!(
        unknown.starts_with(CLI_INBOX_ACTION_UNKNOWN),
        "unknown verb must be refused: {unknown}"
    );
    assert_eq!(
        parse_cli_inbox_line("/inbox").unwrap(),
        CliInboxIntent::List
    );
    assert_eq!(
        parse_cli_inbox_line("/detach").unwrap(),
        CliInboxIntent::Detach
    );
    assert_eq!(
        parse_cli_inbox_line("/eof").unwrap(),
        CliInboxIntent::Detach
    );
}

#[test]
fn notification_cli_refuses_client_hints_and_pages_without_server_binding() {
    let mut hinted_row = page();
    hinted_row["items"][0]["unread_hint"] = json!(4);
    let error = present_notification_page(&hinted_row).unwrap_err();
    assert!(
        error.starts_with(CLI_NOTIFICATION_CLIENT_HINT),
        "client hints must be refused: {error}"
    );

    let mut hinted_item = page();
    hinted_item["items"][0]["item"]["preselected_actions"] = json!(["approve"]);
    let error = present_notification_page(&hinted_item).unwrap_err();
    assert!(
        error.starts_with(CLI_NOTIFICATION_CLIENT_HINT),
        "preselected answers must be refused: {error}"
    );

    let mut short_digest = page();
    short_digest["page_digest"] = json!("sha256:00");
    assert_eq!(
        present_notification_page(&short_digest).unwrap_err(),
        CLI_NOTIFICATION_PAGE_DIGEST_INVALID
    );
    let mut missing_digest = page();
    missing_digest["page_digest"] = json!(null);
    assert_eq!(
        present_notification_page(&missing_digest).unwrap_err(),
        CLI_NOTIFICATION_PAGE_DIGEST_INVALID
    );
    let mut missing_cursor = page();
    missing_cursor["source_cursor"] = json!(0);
    assert!(present_notification_page(&missing_cursor).is_err());

    let presented = present_notification_page(&page()).unwrap();
    let mut tampered = presented.clone();
    tampered.page_digest = "sha256:11".to_owned();
    assert!(
        CliInboxRoute::from_view("session-a", &tampered).is_err(),
        "a route must refuse a page whose digest is not server-shaped"
    );
    assert!(CliInboxRoute::from_view("", &presented).is_err());
    assert!(!CliInboxRoute::for_session("session-a").unwrap().has_page());
}

#[test]
fn notification_cli_refuses_targets_and_decisions_outside_the_server_projection() {
    let presented = present_notification_page(&inbox_page()).unwrap();
    let route = CliInboxRoute::from_view("session-a", &presented).unwrap();

    let unknown_target = plan_cli_inbox_intent(
        &route,
        "session-a",
        &CliInboxIntent::Act {
            verb: CliInboxVerb::Acknowledge,
            target: "notification:999".to_owned(),
        },
    )
    .unwrap_err();
    assert!(
        unknown_target.starts_with(CLI_INBOX_ACTION_NOT_IN_PROJECTION),
        "targets outside the projection must be refused: {unknown_target}"
    );

    // The row has decision actions, so an unoffered presentation verb cannot borrow them.
    let unoffered = plan_cli_inbox_intent(
        &route,
        "session-a",
        &CliInboxIntent::Act {
            verb: CliInboxVerb::Snooze,
            target: "notification:14".to_owned(),
        },
    )
    .unwrap_err();
    assert!(
        unoffered.starts_with(CLI_INBOX_ACTION_NOT_IN_PROJECTION),
        "unoffered actions must be refused: {unoffered}"
    );

    // Approval decisions are bound to the server-issued listing, never to inbox rows.
    let with_approvals = route.clone().with_approvals(&listing()).unwrap();
    assert!(with_approvals.approval("approval:1").is_some());
    let unlisted = plan_cli_inbox_intent(
        &with_approvals,
        "session-a",
        &CliInboxIntent::Act {
            verb: CliInboxVerb::Approve,
            target: "approval:999".to_owned(),
        },
    )
    .unwrap_err();
    assert!(
        unlisted.starts_with(CLI_INBOX_ACTION_NOT_IN_PROJECTION),
        "unlisted approvals must be refused: {unlisted}"
    );

    let with_deny_only = route.clone().with_approvals(&deny_only_listing()).unwrap();
    let not_offered = plan_cli_inbox_intent(
        &with_deny_only,
        "session-a",
        &CliInboxIntent::Act {
            verb: CliInboxVerb::Approve,
            target: "approval:1".to_owned(),
        },
    )
    .unwrap_err();
    assert!(
        not_offered.starts_with(CLI_INBOX_DECISION_NOT_OFFERED),
        "a decision the server did not offer must be refused: {not_offered}"
    );

    let bad_target = plan_cli_inbox_intent(
        &route,
        "session-a",
        &CliInboxIntent::Act {
            verb: CliInboxVerb::Read,
            target: "bad target".to_owned(),
        },
    )
    .unwrap_err();
    assert_eq!(bad_target, CLI_INBOX_TARGET_INVALID);

    // Malformed listings fail closed instead of being filled in locally.
    assert!(CliApprovalBinding::from_listing(&json!({
        "approvals": [{"available_decisions": ["approve"]}]
    }))
    .is_err());
    assert!(CliApprovalBinding::from_listing(&json!({
        "approvals": [{
            "challenge": {"approval_id": "approval:1", "request_hash": REQUEST_HASH},
            "available_decisions": []
        }]
    }))
    .is_err());
    assert!(CliApprovalBinding::from_listing(&json!({"approvals": []}))
        .unwrap()
        .is_empty());
}

#[test]
fn notification_cli_refuses_another_sessions_line_and_never_crosses_routes() {
    let presented = present_notification_page(&inbox_page()).unwrap();
    let route = CliInboxRoute::from_view("session-a", &presented).unwrap();

    let mismatch = plan_cli_inbox_intent(
        &route,
        "session-b",
        &CliInboxIntent::Act {
            verb: CliInboxVerb::Read,
            target: "notification:14".to_owned(),
        },
    )
    .unwrap_err();
    assert!(
        mismatch.starts_with(CLI_INBOX_SESSION_MISMATCH),
        "another session must be refused: {mismatch}"
    );
    assert!(
        plan_cli_inbox_intent(&route, "session-b", &CliInboxIntent::List)
            .unwrap_err()
            .starts_with(CLI_INBOX_SESSION_MISMATCH)
    );
    assert!(plan_cli_detach(&route, "session-b")
        .unwrap_err()
        .starts_with(CLI_INBOX_SESSION_MISMATCH));

    // The bound route still answers its own session for every intent kind.
    assert!(matches!(
        plan_cli_inbox_intent(&route, "session-a", &CliInboxIntent::List).unwrap(),
        CliInboxPlan::Display { rows: 2, .. }
    ));
    assert!(matches!(
        plan_cli_detach(&route, "session-a").unwrap(),
        CliInboxPlan::Detach { .. }
    ));
}

#[test]
fn notification_cli_read_stays_presentation_and_never_consumes_the_action() {
    assert_eq!(CliInboxVerb::parse("read"), Some(CliInboxVerb::Read));
    assert_eq!(CliInboxVerb::parse("mark-read"), Some(CliInboxVerb::Read));
    assert_eq!(CliInboxVerb::parse("reject"), Some(CliInboxVerb::Deny));
    assert!(!CliInboxVerb::Read.consumes_action());
    assert!(!CliInboxVerb::Acknowledge.consumes_action());
    assert!(!CliInboxVerb::Snooze.consumes_action());
    assert!(CliInboxVerb::Review.consumes_action());
    assert!(CliInboxVerb::Reconcile.consumes_action());
    assert!(CliInboxVerb::Approve.is_approval_decision());
    assert!(CliInboxVerb::Deny.is_approval_decision());
    assert!(!CliInboxVerb::Acknowledge.is_approval_decision());
    assert_eq!(CliInboxVerb::Read.authority_effect(), "presentation_read");

    let presented = present_notification_page(&inbox_page()).unwrap();
    let route = CliInboxRoute::from_view("session-a", &presented).unwrap();
    let plan = plan_cli_inbox_intent(
        &route,
        "session-a",
        &CliInboxIntent::Act {
            verb: CliInboxVerb::Read,
            target: "notification:14".to_owned(),
        },
    )
    .unwrap();
    match &plan {
        CliInboxPlan::Read {
            target,
            already_read,
            acknowledged,
            decides,
            submission_route,
            ..
        } => {
            assert_eq!(target, "notification:14");
            assert!(!already_read);
            assert!(!acknowledged);
            assert!(!decides, "a read can never decide or consume the action");
            assert_eq!(submission_route, "notification_read_state");
        }
        other => panic!("expected a read plan: {other:?}"),
    }
    assert!(!plan.cancels_or_resumes());
    assert_eq!(plan.schema(), CLI_INBOX_PLAN_SCHEMA);
    assert!(plan.describe().contains("not consumed"));

    // Reading an already-read row stays presentation state.
    let already = plan_cli_inbox_intent(
        &route,
        "session-a",
        &CliInboxIntent::Act {
            verb: CliInboxVerb::Read,
            target: "notification:15".to_owned(),
        },
    )
    .unwrap();
    assert!(matches!(
        already,
        CliInboxPlan::Read {
            already_read: true,
            acknowledged: true,
            decides: false,
            ..
        }
    ));

    // A row that only offers presentation actions can never satisfy a decision verb.
    let presentation_only = page_with_items(
        json!([{
            "item":{
                "item_id":"notification:16",
                "kind":"delivery",
                "title":"Delivery state changed",
                "source_ref":"event:16",
                "actions":[{"id":"ack","command":"notification.acknowledge","arguments":{}}]
            },
            "urgency":"low",
            "due_at_unix_ms":90,
            "read":false,
            "acknowledged":false,
            "snoozed_until_unix_ms":null
        }]),
        None,
    );
    let presented = present_notification_page(&presentation_only).unwrap();
    let route = CliInboxRoute::from_view("session-a", &presented).unwrap();
    let refused = plan_cli_inbox_intent(
        &route,
        "session-a",
        &CliInboxIntent::Act {
            verb: CliInboxVerb::Review,
            target: "notification:16".to_owned(),
        },
    )
    .unwrap_err();
    assert!(
        refused.starts_with(CLI_INBOX_READ_IS_NOT_CONSUME),
        "read/ACK state must never be borrowed as a decision: {refused}"
    );
}

#[test]
fn notification_cli_disconnect_is_a_detach_and_never_cancels_or_resumes() {
    assert_eq!(
        parse_cli_inbox_line("/disconnect").unwrap(),
        CliInboxIntent::Detach
    );
    let presented = present_notification_page(&page()).unwrap();
    let route = CliInboxRoute::from_view("session-a", &presented).unwrap();
    let plan = plan_cli_detach(&route, "session-a").unwrap();
    match &plan {
        CliInboxPlan::Detach {
            session_id,
            follow_up,
            refusal,
        } => {
            assert_eq!(session_id, "session-a");
            assert_eq!(follow_up, "requery_original");
            assert_eq!(*refusal, CLI_INBOX_DISCONNECT_IS_NOT_CANCEL);
        }
        other => panic!("expected a detach plan: {other:?}"),
    }
    assert!(!plan.cancels_or_resumes());
    assert!(plan.describe().contains("no implicit cancel or resume"));
}

#[test]
fn notification_cli_badges_refuse_client_counters_and_unbound_pages() {
    let presented = present_notification_page(&inbox_page()).unwrap();
    let route = CliInboxRoute::from_view("session-a", &presented).unwrap();

    assert_eq!(
        present_session_badges("session-a", 0, std::slice::from_ref(&route)).unwrap_err(),
        CLI_INBOX_SERVER_TIME_REQUIRED
    );
    let unbound = CliInboxRoute::for_session("session-b").unwrap();
    assert_eq!(
        present_session_badges("session-a", SERVER_TIME_UNIX_MS, &[unbound]).unwrap_err(),
        CLI_INBOX_PAGE_DIGEST_REQUIRED
    );
    assert_eq!(
        present_session_badges(
            "session-a",
            SERVER_TIME_UNIX_MS,
            &[route.clone(), route.clone()]
        )
        .unwrap_err(),
        CLI_INBOX_SESSION_DUPLICATE
    );
    let many: Vec<CliInboxRoute> = (0..17)
        .map(|index| CliInboxRoute::from_view(&format!("session-{index}"), &presented).unwrap())
        .collect();
    assert_eq!(
        present_session_badges("session-0", SERVER_TIME_UNIX_MS, &many).unwrap_err(),
        CLI_INBOX_SESSIONS_LIMIT
    );
}

#[test]
fn malformed_and_oversized_pages_fail_closed() {
    let mut invalid = page();
    invalid["items"][0]["item"]["title"] = serde_json::Value::String("".to_owned());
    assert!(present_notification_page(&invalid).is_err());
    let mut oversized = page();
    oversized["items"] = serde_json::Value::Array(vec![page()["items"][0].clone(); 31]);
    assert!(present_notification_page(&oversized).is_err());
}

#[test]
fn presenter_redacts_payload_and_keeps_actions_display_only() {
    let view = present_notification_page(&page()).unwrap();
    assert_eq!(view.rows.len(), 1);
    assert_eq!(view.rows[0].actions[0].command, "notification.approve");
    assert!(!serde_json::to_string(&view).unwrap().contains("raw"));
    assert!(view.unknown_visible);
}

#[test]
fn run_status_preserves_unknown_and_terminal_retry_semantics() {
    let running = present_run_status(&frame(UiFeedFrameKind::Delta, false)).unwrap();
    assert_eq!(running.status, CliRunStatus::Running);
    assert_eq!(running.retry, "do_not_retry");
    let unknown = present_run_status(&frame(UiFeedFrameKind::Unknown, false)).unwrap();
    assert_eq!(unknown.status, CliRunStatus::Unknown);
    assert!(unknown.snapshot_required);
    assert_eq!(unknown.retry, "query_original");
    let terminal = present_run_status(&frame(UiFeedFrameKind::Terminal, true)).unwrap();
    assert_eq!(terminal.status, CliRunStatus::Terminal);
}

#[test]
fn notification_cli_inbox_approvals_and_badges_share_one_server_bound_route() {
    let presented = present_notification_page(&inbox_page()).unwrap();
    let route = CliInboxRoute::from_view("session-a", &presented)
        .unwrap()
        .with_approvals(&listing())
        .unwrap();

    let lines = render_inbox_lines(&presented);
    assert_eq!(lines[0], "inbox: 2 row(s) at server cursor 12");
    assert!(lines
        .iter()
        .any(|line| line.contains("[critical] Approval required")));
    assert!(lines
        .iter()
        .any(|line| line.contains("read/ack/snoozed/unknown")));

    let read = plan_cli_inbox_intent(
        &route,
        "session-a",
        &CliInboxIntent::Act {
            verb: CliInboxVerb::Read,
            target: "notification:14".to_owned(),
        },
    )
    .unwrap();
    match &read {
        CliInboxPlan::Read {
            decides,
            submission_route,
            ..
        } => {
            assert!(!decides);
            assert_eq!(submission_route, "notification_read_state");
        }
        other => panic!("expected a read plan: {other:?}"),
    }

    // An ACK is a registered candidate the ControlPlane must re-admit.
    let ack = plan_cli_inbox_intent(
        &route,
        "session-a",
        &CliInboxIntent::Act {
            verb: CliInboxVerb::Acknowledge,
            target: "notification:14".to_owned(),
        },
    )
    .unwrap();
    match &ack {
        CliInboxPlan::Action {
            command,
            re_admitted_by,
            authority_effect,
            source_cursor,
            ..
        } => {
            assert_eq!(command, "notification.acknowledge");
            assert_eq!(re_admitted_by, "control_plane");
            assert_eq!(authority_effect, "presentation");
            assert_eq!(*source_cursor, 12);
        }
        other => panic!("expected an action candidate: {other:?}"),
    }
    assert!(ack.describe().contains("re-admitted by control_plane"));

    // The approval decision is bound to the server-issued challenge hash.
    let approve = plan_cli_inbox_intent(
        &route,
        "session-a",
        &CliInboxIntent::Act {
            verb: CliInboxVerb::Approve,
            target: "approval:1".to_owned(),
        },
    )
    .unwrap();
    match &approve {
        CliInboxPlan::Approval {
            approval_id,
            request_hash,
            approve,
            ..
        } => {
            assert_eq!(approval_id, "approval:1");
            assert_eq!(request_hash, REQUEST_HASH);
            assert!(approve);
        }
        other => panic!("expected an approval plan: {other:?}"),
    }
    let deny = plan_cli_inbox_intent(
        &route,
        "session-a",
        &CliInboxIntent::Act {
            verb: CliInboxVerb::Deny,
            target: "approval:1".to_owned(),
        },
    )
    .unwrap();
    assert!(matches!(
        deny,
        CliInboxPlan::Approval { approve: false, .. }
    ));

    // Badges are a projection of the bound server page; the client only renders.
    let badges = present_session_badges(
        "session-a",
        presented.server_time_unix_ms.unwrap(),
        &[route.clone()],
    )
    .unwrap();
    assert_eq!(badges.schema, CLI_SESSION_BADGE_SCHEMA);
    assert_eq!(badges.badges.len(), 1);
    let badge = &badges.badges[0];
    assert_eq!(badge.session_id, "session-a");
    assert_eq!(badge.source_cursor, 12);
    assert_eq!(badge.page_digest, PAGE_DIGEST);
    assert_eq!(badge.unread, 1);
    assert_eq!(badge.read, 1);
    assert_eq!(badge.acknowledged, 1);
    assert_eq!(badge.unknown, 1);
    assert_eq!(badge.critical, 1);
    assert_eq!(badge.pending_approvals, 1);
    assert_eq!(badge.derived_from, "server_page");
    let badge_lines = render_badge_lines(&badges);
    assert!(badge_lines
        .iter()
        .any(|line| line.starts_with("*session-a")));
    assert!(badge_lines
        .iter()
        .any(|line| line.contains("unread:1 pending:1 unknown:1 critical:1")));
}

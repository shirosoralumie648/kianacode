//! ER-27 source guard for the four-entry read-only receipt/recovery path.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "ER-27 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn all_surfaces_use_one_daemon_protocol_and_control_plane() {
    let client = include_str!("../../kiana-client/src/lib.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let harness = include_str!("../../kiana-entrypoints/src/harness_run.rs");
    let workbench = include_str!("../../kiana-entrypoints/src/workbench_chat.rs");
    let web = include_str!("../../kiana-entrypoints/src/web.rs");
    let page = include_str!("../../kiana-entrypoints/src/web_page.html");
    let product = include_str!("../../kiana-entrypoints/src/product_command.rs");
    let cli = include_str!("../../kiana-entrypoints/src/cli.rs");
    let desktop = include_str!("../../contrib/desktop/main.js");

    require(
        client,
        &[
            "ClientTransport",
            "RequestEnvelope::",
            "resume_run",
            "cancel_run",
            "receipt",
            "pending_approvals",
            "send(RequestEnvelope::command",
        ],
        "client facade",
    );
    require(
        protocol,
        &[
            "RequestEnvelope",
            "RequestBody",
            "ReceiptRequest",
            "ResumeRequest",
            "CancelRequest",
            "ResponseEnvelope",
        ],
        "protocol DTO",
    );
    require(
        daemon,
        &[
            "RequestBody::Resume",
            "RequestBody::Cancel",
            "RequestBody::Receipt",
            "ResponseEnvelope::from_core",
            "ui_snapshot",
            "run_stream",
        ],
        "DaemonHost route",
    );
    require(
        harness,
        &[
            "pending_approvals_envelope_on_host",
            "resume_envelope_on_host",
            "cancel_envelope_on_host",
            "receipt_envelope_on_host",
            "command_envelope_on_host",
            "DaemonHost",
        ],
        "shared entrypoint adapter",
    );
    require(
        workbench,
        &[
            "DaemonHost",
            "receipt_envelope_on_host",
            "resume_envelope_on_host",
            "cancel_envelope_on_host",
            "subscribe_run_after",
            "ResultUnknown",
        ],
        "Workbench",
    );
    require(
        web,
        &[
            "/api/state",
            "/api/events",
            "/api/receipt",
            "/api/resume",
            "/api/cancel",
            "receipt_envelope_on_host",
            "resume_envelope_on_host",
            "cancel_envelope_on_host",
            "command_envelope_on_host",
        ],
        "Web",
    );
    require(
        page,
        &[
            "api('/api/resume'",
            "api('/api/cancel'",
            "receipt",
            "stream_gap",
        ],
        "Web client",
    );
    require(
        product,
        &[
            "DaemonHost",
            "RequestEnvelope::resume_run",
            "RequestEnvelope::command",
        ],
        "product command",
    );
    require(
        cli,
        &["DaemonHost", "RequestEnvelope", "ExecutionStatus"],
        "CLI",
    );
    require(
        desktop,
        &["startHarness", "waitForReady", "stopWorker", "/api"],
        "Desktop",
    );
}

#[test]
fn read_and_recovery_surfaces_cannot_parse_or_execute_eventlog() {
    let workbench = include_str!("../../kiana-entrypoints/src/workbench_chat.rs");
    let web = include_str!("../../kiana-entrypoints/src/web.rs");
    let page = include_str!("../../kiana-entrypoints/src/web_page.html");
    let product = include_str!("../../kiana-entrypoints/src/product_command.rs");
    let cli = include_str!("../../kiana-entrypoints/src/cli.rs");
    let desktop = include_str!("../../contrib/desktop/main.js");
    let cp22 = include_str!("../../kiana-entrypoints/tests/cp22_protocol_surfaces.rs");
    let accessibility = include_str!("../../kiana-entrypoints/tests/p2_m7_01_accessibility.rs");
    let inbox = include_str!("../../kiana-core/tests/p2_k3_01_human_inbox.rs");
    let action_cards = include_str!("../../kiana-core/tests/p2_m3_01_action_cards.rs");

    require(
        cp22,
        &[
            "receipt authoritative",
            "api('/api/resume'",
            "api('/api/cancel'",
        ],
        "CP-22 parity fixture",
    );
    require(
        accessibility,
        &["cancelled", "incomplete"],
        "accessibility fixture",
    );
    require(
        inbox,
        &["HumanInbox", "failure.reconcile", "failure_incidents"],
        "inbox fixture",
    );
    require(
        action_cards,
        &["human.inbox", "failure.reconcile"],
        "action-card fixture",
    );

    for source in [workbench, web, page, product, cli, desktop] {
        for forbidden in [
            "EventLog::",
            "JsonlEventLog::",
            "MemoryEventLog::",
            "ModelClient",
            "CapabilityBroker",
            "execute_authorized_request(",
            "auto_approve",
            "autoapprove",
        ] {
            assert!(
                !source.contains(forbidden),
                "ER-27 entrypoint bypass marker present: {forbidden}"
            );
        }
    }
}

/// The source guard above is not enough on its own: ER-27 must ship runnable behaviour fixtures for
/// the same four-entry read-only path, and those fixtures must be executed by the ER-27 CI lane.
#[test]
fn behavioural_fixtures_are_wired_into_the_er27_lane() {
    let fixture = include_str!("../../kiana-daemon/tests/er27_entrypoint_receipt_recovery.rs");
    require(
        fixture,
        &[
            "DaemonHost",
            "RequestEnvelope::receipt",
            "RequestEnvelope::resume_run",
            "RequestEnvelope::cancel_run",
            "RequestEnvelope::pending_approvals",
            "EntryPointKind::Cli",
            "EntryPointKind::Web",
            "EntryPointKind::Workbench",
            "EntryPointKind::Desktop",
            "ZeroCallRunner",
            "receipt_digest",
            "AUTH_CALLER_UNTRUSTED",
            "role_department_mismatch",
            "role_unknown",
            "receipt_not_found",
            "protocol_schema_unsupported",
            "already_terminal",
            "run_resume_terminal_completed",
        ],
        "ER-27 behavioural fixture",
    );

    let workflow = include_str!("../../.github/workflows/er27-entrypoint-parity.yml");
    require(
        workflow,
        &[
            "cargo test -p kiana-daemon --test er27_entrypoint_receipt_recovery",
            "er27_entrypoint_parity_guard",
        ],
        "ER-27 workflow lane",
    );
}

//! ER-26 source guard for cursor queries, snapshots and slow consumers.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "ER-26 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn cursor_epoch_gap_and_snapshot_boundaries_are_explicit() {
    let eventlog = include_str!("../../kiana-eventlog/src/journal_core.rs");
    let eventlog_stream = include_str!("../../kiana-eventlog/src/stream.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let ui_contract = include_str!("../../kiana-protocol/src/ui_contracts.rs");
    let daemon = include_str!("../../kiana-daemon/src/run_stream.rs");
    let daemon_host = include_str!("../../kiana-daemon/src/lib.rs");

    require(
        eventlog,
        &[
            "pub fn page",
            "eventlog_cursor_not_commit_boundary",
            "JournalPage",
            "boundaries",
        ],
        "EventLog page",
    );
    require(
        eventlog_stream,
        &[
            "committed_cursor",
            "read_from",
            "durable cursor",
            "source_cursor",
        ],
        "stream cursor",
    );
    require(
        ports,
        &[
            "Read committed RuntimeEvent facts",
            "read_from",
            "cursor_reads_unsupported",
        ],
        "cursor port",
    );
    require(
        protocol,
        &[
            "AuditQueryRequest",
            "source_cursor",
            "after_cursor",
            "AuditQueryResponse",
            "projection_version",
            "RunStreamEnvelope",
            "advance_cursor",
            "stream_sequence_gap",
            "stream_epoch_changed",
        ],
        "wire cursor",
    );
    require(
        ui_contract,
        &["UiSnapshotV1", "snapshot_cursor", "UiCursorV1"],
        "UI snapshot contract",
    );
    require(
        daemon,
        &[
            "RUN_STREAM_CAPACITY",
            "subscribe_after",
            "same_epoch",
            "has_gap",
            "terminal = Some(envelope.clone())",
            "RunStreamSubscription",
        ],
        "run stream boundary",
    );
    require(
        daemon_host,
        &[
            "ui_snapshot",
            "stream_cursor",
            "RunOutcome::ResultUnknown",
            "run_stream_cursor",
        ],
        "daemon snapshot",
    );
}

#[test]
fn entrypoints_reconnect_read_only_and_receipt_authoritative() {
    let web = include_str!("../../kiana-entrypoints/src/web.rs");
    let page = include_str!("../../kiana-entrypoints/src/web_page.html");
    let workbench = include_str!("../../kiana-entrypoints/src/workbench_chat.rs");
    let cli = include_str!("../../kiana-entrypoints/src/cli.rs");
    let web_fixture = include_str!("../../kiana-entrypoints/tests/p2_m5_01_web_sync.rs");
    let cli_fixture = include_str!("../../kiana-entrypoints/tests/cli_stream_json.rs");
    let protocol_fixture = include_str!("../../kiana-protocol/tests/p4_j7_02_sequence.rs");
    let cursor_fixture = include_str!("../../kiana-core/tests/oa17_query_cursor.rs");

    require(
        web,
        &[
            "/api/state",
            "/api/events",
            "ui_snapshot",
            "subscribe_run_after",
            "stream_cursor_from_request",
            "snapshot_required_after_stream_gap",
            "stream_subscription_lagged",
            "stream_closed_before_terminal",
            "Subscribe before returning the SSE response headers",
        ],
        "Web reconnect",
    );
    require(
        page,
        &[
            "EventSource",
            "streamCursors",
            "observedUiCursor",
            "clearStreamProjection",
            "markStreamIncomplete",
            "source.addEventListener('stream_gap', handleStreamGap)",
            "stream_sequence_gap",
        ],
        "Web hydration",
    );
    require(
        workbench,
        &[
            "advance_cursor",
            "subscribe_run_after",
            "stream_cursor",
            "receipt",
        ],
        "Workbench reconnect",
    );
    require(cli, &["events.snapshot.read"], "CLI read path");
    require(
        web_fixture,
        &[
            "snapshot_required_after_stream_gap",
            "stream_sequence_gap",
            "receipt authoritative",
        ],
        "Web fixture",
    );
    require(
        cli_fixture,
        &[
            "--input-format=stream-json",
            "kiana.stream-json-input-error.v1",
        ],
        "CLI fixture",
    );
    require(
        protocol_fixture,
        &[
            "stream_sequence_gap",
            "stream_epoch_changed",
            "advance_cursor",
        ],
        "protocol fixture",
    );
    require(
        cursor_fixture,
        &[
            "cursor_binds_epoch_projection_source_and_filter_digest",
            "source_cursor",
        ],
        "query cursor fixture",
    );

    for source in [web, page, workbench, cli] {
        for forbidden in [
            "ModelClient",
            "CapabilityBroker",
            "start_run(",
            "execute_authorized_request(",
            "append_event(",
        ] {
            assert!(
                !source.contains(forbidden),
                "ER-26 read path widened authority: {forbidden}"
            );
        }
    }
}

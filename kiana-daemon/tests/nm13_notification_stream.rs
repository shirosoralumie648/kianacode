use kiana_daemon::{
    NotificationStreamBridge, NotificationStreamDisposition, NotificationStreamError,
    NOTIFICATION_STREAM_BRIDGE_SCHEMA, NOTIFICATION_STREAM_MERGE_WINDOW,
    NOTIFICATION_STREAM_SEEN_EVENT_IDS,
};
use kiana_domain::RunId;
use kiana_protocol::{
    UiCursor, UiFeedCursorV1, UiFeedFrameKind, UiFeedFrameV1, UiFeedGapReason, UiFeedGapV1,
    UI_FEED_FRAME_SCHEMA, UI_FEED_GAP_SCHEMA,
};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn frame(
    sequence: u64,
    kind: UiFeedFrameKind,
    terminal: bool,
    instance: &str,
    epoch: &str,
    snapshot_sequence: u64,
) -> UiFeedFrameV1 {
    let cursor = UiFeedCursorV1::new(
        instance,
        epoch,
        sequence,
        UiCursor {
            epoch: epoch.to_owned(),
            sequence: snapshot_sequence,
        },
    )
    .unwrap();
    let gap = (kind == UiFeedFrameKind::Gap).then(|| UiFeedGapV1 {
        schema: UI_FEED_GAP_SCHEMA.to_owned(),
        reason: UiFeedGapReason::SequenceGap,
        from: None,
        to: cursor.clone(),
        snapshot_required: true,
    });
    UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        kind,
        cursor,
        event_id: format!("event-{sequence}"),
        replay: false,
        terminal,
        event: None,
        gap,
    }
}

fn gap_frame(
    sequence: u64,
    reason: UiFeedGapReason,
    instance: &str,
    epoch: &str,
    snapshot_sequence: u64,
) -> UiFeedFrameV1 {
    let mut frame = frame(
        sequence,
        UiFeedFrameKind::Gap,
        false,
        instance,
        epoch,
        snapshot_sequence,
    );
    frame.event_id = format!("gap-{sequence}");
    let to = frame.cursor.clone();
    frame.gap = Some(UiFeedGapV1 {
        schema: UI_FEED_GAP_SCHEMA.to_owned(),
        reason,
        from: None,
        to,
        snapshot_required: true,
    });
    frame
}

fn with_event(mut frame: UiFeedFrameV1, event: Value) -> UiFeedFrameV1 {
    frame.event = Some(event);
    frame
}

fn with_event_id(mut frame: UiFeedFrameV1, event_id: &str) -> UiFeedFrameV1 {
    frame.event_id = event_id.to_owned();
    frame
}

#[test]
fn fixture_declares_snapshot_first_gap_and_disposed_contract() {
    let value: Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/nm13-notification-stream.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(value["schema"], "kiana.notification-stream-fixture.v1");
    assert!(value["denied"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains("gap")));
    assert!(value["accepted"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains("merge window")));
    assert!(value["accepted"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains("event id")));
    assert!(value["denied"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains("foreign run")));
    assert!(value["denied"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains("terminal loss")));
}

#[test]
fn bridge_requires_boundary_then_handles_after_cursor_heartbeat_gap_and_dispose() {
    let mut bridge =
        NotificationStreamBridge::new(RunId::new(), "instance-13", "epoch-13").unwrap();
    assert_eq!(bridge.cursor().schema, NOTIFICATION_STREAM_BRIDGE_SCHEMA);
    assert!(bridge.snapshot_required());
    assert_eq!(
        bridge.ingest(&frame(
            1,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            1
        )),
        Ok(NotificationStreamDisposition::SnapshotRequired(
            UiFeedGapReason::ReplayExpired
        ))
    );
    assert_eq!(
        bridge.ingest(&frame(
            0,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            1
        )),
        Ok(NotificationStreamDisposition::SnapshotBoundary)
    );
    assert_eq!(
        bridge.ingest(&frame(
            1,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            1
        )),
        Ok(NotificationStreamDisposition::Accepted {
            feed_sequence: 1,
            source_cursor: 1
        })
    );
    assert_eq!(
        bridge.ingest(&frame(
            1,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            1
        )),
        Ok(NotificationStreamDisposition::Replayed)
    );
    assert_eq!(
        bridge.ingest(&frame(
            3,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            3
        )),
        Ok(NotificationStreamDisposition::SnapshotRequired(
            UiFeedGapReason::SequenceGap
        ))
    );
    assert_eq!(
        bridge.ingest(&frame(
            1,
            UiFeedFrameKind::Heartbeat,
            false,
            "instance-13",
            "epoch-13",
            2
        )),
        Ok(NotificationStreamDisposition::Heartbeat)
    );
    bridge.dispose();
    assert_eq!(
        bridge.ingest(&frame(
            2,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            2
        )),
        Ok(NotificationStreamDisposition::Disposed)
    );
}

#[test]
fn old_epoch_and_terminal_late_update_fail_closed() {
    let mut bridge =
        NotificationStreamBridge::new(RunId::new(), "instance-13", "epoch-13").unwrap();
    bridge
        .ingest(&frame(
            0,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            1,
        ))
        .unwrap();
    assert_eq!(
        bridge.ingest(&frame(
            1,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "old",
            1
        )),
        Ok(NotificationStreamDisposition::SnapshotRequired(
            UiFeedGapReason::OldEpoch
        ))
    );
    bridge
        .ingest(&frame(
            1,
            UiFeedFrameKind::Terminal,
            true,
            "instance-13",
            "epoch-13",
            1,
        ))
        .unwrap();
    assert_eq!(
        bridge.ingest(&frame(
            2,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            2
        )),
        Ok(NotificationStreamDisposition::SnapshotRequired(
            UiFeedGapReason::ReplayExpired,
        ))
    );
}

#[test]
fn terminal_retention_and_unknown_frames_never_collapse_into_a_delta() {
    let mut replayed =
        NotificationStreamBridge::new(RunId::new(), "instance-13", "epoch-13").unwrap();
    assert_eq!(
        replayed.ingest(&frame(
            4,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            7
        )),
        Ok(NotificationStreamDisposition::SnapshotBoundary)
    );
    // The retained terminal shares the boundary position; it must still be reported as the
    // terminal fact instead of being suppressed as a duplicate frame.
    assert_eq!(
        replayed.ingest(&frame(
            4,
            UiFeedFrameKind::Terminal,
            true,
            "instance-13",
            "epoch-13",
            7
        )),
        Ok(NotificationStreamDisposition::Terminal)
    );
    assert!(replayed.cursor().terminal);
    assert_eq!(
        replayed.ingest(&frame(
            4,
            UiFeedFrameKind::Terminal,
            true,
            "instance-13",
            "epoch-13",
            7
        )),
        Ok(NotificationStreamDisposition::Terminal)
    );
    assert_eq!(
        replayed.ingest(&frame(
            5,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            8
        )),
        Err(NotificationStreamError::TerminalUpdate)
    );

    let mut unknown =
        NotificationStreamBridge::new(RunId::new(), "instance-13", "epoch-13").unwrap();
    unknown
        .ingest(&frame(
            1,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            3,
        ))
        .unwrap();
    assert!(!unknown.snapshot_required());
    assert_eq!(
        unknown.ingest(&frame(
            2,
            UiFeedFrameKind::Unknown,
            false,
            "instance-13",
            "epoch-13",
            4,
        )),
        Ok(NotificationStreamDisposition::Unknown)
    );
    // The position is the server's, so the cursor advances; the payload stays uninterpreted and
    // the surface is told to hydrate the snapshot instead of rendering it as content.
    assert_eq!(unknown.cursor().feed_sequence, 2);
    assert_eq!(unknown.cursor().source_cursor, 4);
    assert!(!unknown.cursor().terminal);
    assert!(unknown.snapshot_required());
}

#[test]
fn the_first_attach_hydrates_before_it_renders_and_keeps_no_window() {
    let mut bridge =
        NotificationStreamBridge::new(RunId::new(), "instance-13", "epoch-13").unwrap();
    assert!(bridge.snapshot_required());
    assert!(bridge.terminal_query_required());
    assert_eq!(bridge.merge_window_len(), 0);
    assert_eq!(
        bridge.ingest(&frame(
            1,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            1
        )),
        Ok(NotificationStreamDisposition::SnapshotRequired(
            UiFeedGapReason::ReplayExpired
        ))
    );
    // A bridge with no baseline has nothing to merge against: the frame is refused, not kept.
    assert_eq!(bridge.merge_window_len(), 0);
    assert_eq!(
        bridge.ingest(&frame(
            0,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            1
        )),
        Ok(NotificationStreamDisposition::SnapshotBoundary)
    );
    assert_eq!(bridge.merged_frames_len(), 0);
    assert!(bridge.take_merged_frames().is_empty());
    assert_eq!(
        bridge.ingest(&frame(
            1,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            1
        )),
        Ok(NotificationStreamDisposition::Accepted {
            feed_sequence: 1,
            source_cursor: 1
        })
    );
    assert!(!bridge.terminal_query_required());
}

#[test]
fn lagged_and_retired_gaps_never_report_completion() {
    let mut bridge =
        NotificationStreamBridge::new(RunId::new(), "instance-13", "epoch-13").unwrap();
    assert_eq!(
        bridge.ingest(&frame(
            2,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            2
        )),
        Ok(NotificationStreamDisposition::SnapshotBoundary)
    );
    // A slow consumer loses frames: the feed says so with a backpressure gap and the bridge must
    // never turn it into a finished run.
    assert_eq!(
        bridge.ingest(&gap_frame(
            2,
            UiFeedGapReason::Backpressure,
            "instance-13",
            "epoch-13",
            2
        )),
        Ok(NotificationStreamDisposition::SnapshotRequired(
            UiFeedGapReason::Backpressure
        ))
    );
    assert!(!bridge.cursor().terminal);
    assert!(!bridge.terminal_observed());
    assert!(bridge.terminal_query_required());
    // Deltas arriving during the recovery window are buffered instead of being rendered.
    assert_eq!(
        bridge.ingest(&frame(
            3,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            3
        )),
        Ok(NotificationStreamDisposition::SnapshotRequired(
            UiFeedGapReason::ReplayExpired
        ))
    );
    assert_eq!(bridge.merge_window_len(), 1);
    assert!(!bridge.cursor().terminal);
    // The fresh boundary supersedes the buffered frame: the snapshot carries it.
    assert_eq!(
        bridge.ingest(&frame(
            3,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            3
        )),
        Ok(NotificationStreamDisposition::SnapshotBoundary)
    );
    assert_eq!(bridge.merge_window_len(), 0);
    assert!(bridge.take_merged_frames().is_empty());
    assert_eq!(bridge.cursor().feed_sequence, 3);
    assert_eq!(
        bridge.ingest(&frame(
            4,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            4
        )),
        Ok(NotificationStreamDisposition::Accepted {
            feed_sequence: 4,
            source_cursor: 4
        })
    );
}

#[test]
fn a_late_socket_keeps_the_retained_terminal_and_a_retired_window_stays_visible() {
    // A socket that attached after the run finished still reads the retained terminal.
    let mut late = NotificationStreamBridge::new(RunId::new(), "instance-13", "epoch-13").unwrap();
    assert_eq!(
        late.ingest(&frame(
            5,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            9
        )),
        Ok(NotificationStreamDisposition::SnapshotBoundary)
    );
    let mut retained = frame(
        5,
        UiFeedFrameKind::Terminal,
        true,
        "instance-13",
        "epoch-13",
        9,
    );
    retained.replay = true;
    assert_eq!(
        late.ingest(&retained),
        Ok(NotificationStreamDisposition::Terminal)
    );
    assert!(late.terminal_observed());
    assert!(late.cursor().terminal);
    assert!(!late.terminal_query_required());

    // A window that retention already dropped is reported as a gap, never as silence.
    let mut retired =
        NotificationStreamBridge::new(RunId::new(), "instance-13", "epoch-13").unwrap();
    assert_eq!(
        retired.ingest(&gap_frame(
            0,
            UiFeedGapReason::TerminalRetention,
            "instance-13",
            "epoch-13",
            1
        )),
        Ok(NotificationStreamDisposition::SnapshotRequired(
            UiFeedGapReason::TerminalRetention
        ))
    );
    assert!(retired.snapshot_required());
    assert!(!retired.cursor().terminal);
    assert!(retired.terminal_query_required());
}

#[test]
fn a_recovery_window_merges_only_the_frames_the_snapshot_does_not_contain() {
    let mut bridge =
        NotificationStreamBridge::new(RunId::new(), "instance-13", "epoch-13").unwrap();
    assert_eq!(
        bridge.ingest(&frame(
            1,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            1
        )),
        Ok(NotificationStreamDisposition::SnapshotBoundary)
    );
    assert_eq!(
        bridge.ingest(&frame(
            2,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            2
        )),
        Ok(NotificationStreamDisposition::Accepted {
            feed_sequence: 2,
            source_cursor: 2
        })
    );
    assert_eq!(
        bridge.ingest(&gap_frame(
            2,
            UiFeedGapReason::SequenceGap,
            "instance-13",
            "epoch-13",
            2
        )),
        Ok(NotificationStreamDisposition::SnapshotRequired(
            UiFeedGapReason::SequenceGap
        ))
    );
    // Subscribe-first: three frames race the hydration.  Only the one ahead of the adopted
    // boundary is content the snapshot cannot contain, and it stays visible to the surface.
    for sequence in [3, 4] {
        assert_eq!(
            bridge.ingest(&frame(
                sequence,
                UiFeedFrameKind::Delta,
                false,
                "instance-13",
                "epoch-13",
                sequence
            )),
            Ok(NotificationStreamDisposition::SnapshotRequired(
                UiFeedGapReason::ReplayExpired
            ))
        );
    }
    assert_eq!(
        bridge.ingest(&frame(
            5,
            UiFeedFrameKind::Terminal,
            true,
            "instance-13",
            "epoch-13",
            5
        )),
        Ok(NotificationStreamDisposition::SnapshotRequired(
            UiFeedGapReason::ReplayExpired
        ))
    );
    assert_eq!(bridge.merge_window_len(), 3);
    assert_eq!(
        bridge.ingest(&frame(
            4,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            10
        )),
        Ok(NotificationStreamDisposition::SnapshotBoundary)
    );
    let merged = bridge.take_merged_frames();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].cursor.feed_sequence, 5);
    assert_eq!(merged[0].kind, UiFeedFrameKind::Terminal);
    assert_eq!(bridge.merged_frames_len(), 0);
    assert_eq!(bridge.cursor().feed_sequence, 5);
    assert_eq!(bridge.cursor().source_cursor, 10);
    assert!(bridge.merge_window_len() == 0);
    // The merged terminal kept its meaning: a following delta is a late update, not a fresh live
    // segment, and the surface never needs to guess whether the run finished.
    assert!(bridge.terminal_observed());
    assert_eq!(
        bridge.ingest(&frame(
            6,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            11
        )),
        Err(NotificationStreamError::TerminalUpdate)
    );
}

#[test]
fn a_recovery_window_never_delivers_an_event_twice() {
    let mut bridge =
        NotificationStreamBridge::new(RunId::new(), "instance-13", "epoch-13").unwrap();
    assert_eq!(
        bridge.ingest(&frame(
            1,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            1
        )),
        Ok(NotificationStreamDisposition::SnapshotBoundary)
    );
    assert_eq!(
        bridge.ingest(&frame(
            2,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            2
        )),
        Ok(NotificationStreamDisposition::Accepted {
            feed_sequence: 2,
            source_cursor: 2
        })
    );
    assert_eq!(
        bridge.ingest(&gap_frame(
            2,
            UiFeedGapReason::SequenceGap,
            "instance-13",
            "epoch-13",
            2
        )),
        Ok(NotificationStreamDisposition::SnapshotRequired(
            UiFeedGapReason::SequenceGap
        ))
    );
    // The feed re-publishes the already delivered event at a new position while the surface is
    // still hydrating; merging it would render the same content twice.
    let republished = with_event_id(
        frame(
            5,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            5,
        ),
        "event-2",
    );
    assert_eq!(
        bridge.ingest(&republished),
        Ok(NotificationStreamDisposition::SnapshotRequired(
            UiFeedGapReason::ReplayExpired
        ))
    );
    assert_eq!(
        bridge.ingest(&frame(
            4,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            20
        )),
        Ok(NotificationStreamDisposition::SnapshotBoundary)
    );
    assert!(bridge.take_merged_frames().is_empty());
    assert_eq!(bridge.cursor().feed_sequence, 4);

    // Live dedupe: the same event id at a later position never becomes a second item, and it does
    // not move the cursor, so the real next frame still lines up.
    let mut live = NotificationStreamBridge::new(RunId::new(), "instance-13", "epoch-13").unwrap();
    live.ingest(&frame(
        1,
        UiFeedFrameKind::SnapshotBoundary,
        false,
        "instance-13",
        "epoch-13",
        1,
    ))
    .unwrap();
    assert_eq!(
        live.ingest(&frame(
            2,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            2
        )),
        Ok(NotificationStreamDisposition::Accepted {
            feed_sequence: 2,
            source_cursor: 2
        })
    );
    let before = live.cursor().clone();
    let replay = with_event_id(
        frame(
            3,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            3,
        ),
        "event-2",
    );
    assert_eq!(
        live.ingest(&replay),
        Ok(NotificationStreamDisposition::Replayed)
    );
    assert_eq!(live.cursor(), &before);
    assert_eq!(
        live.ingest(&frame(
            3,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            3
        )),
        Ok(NotificationStreamDisposition::Accepted {
            feed_sequence: 3,
            source_cursor: 3
        })
    );
}

#[test]
fn an_overflowing_merge_window_fails_closed_instead_of_reporting_a_partial_merge() {
    let mut bridge =
        NotificationStreamBridge::new(RunId::new(), "instance-13", "epoch-13").unwrap();
    bridge
        .ingest(&frame(
            1,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            1,
        ))
        .unwrap();
    bridge
        .ingest(&gap_frame(
            1,
            UiFeedGapReason::SequenceGap,
            "instance-13",
            "epoch-13",
            1,
        ))
        .unwrap();
    for sequence in 2..=(NOTIFICATION_STREAM_MERGE_WINDOW as u64 + 2) {
        assert_eq!(
            bridge.ingest(&frame(
                sequence,
                UiFeedFrameKind::Delta,
                false,
                "instance-13",
                "epoch-13",
                sequence
            )),
            Ok(NotificationStreamDisposition::SnapshotRequired(
                UiFeedGapReason::ReplayExpired
            ))
        );
    }
    // One frame past the bound clears the window: a partial merge is never presented as complete.
    assert_eq!(bridge.merge_window_len(), 0);
    assert!(bridge.snapshot_required());
    assert_eq!(
        bridge.ingest(&frame(
            NOTIFICATION_STREAM_MERGE_WINDOW as u64 + 2,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            NOTIFICATION_STREAM_MERGE_WINDOW as u64 + 2
        )),
        Ok(NotificationStreamDisposition::SnapshotBoundary)
    );
    assert!(bridge.take_merged_frames().is_empty());
}

#[test]
fn a_foreign_run_payload_is_refused_before_any_state_moves() {
    let run_id = RunId::new();
    let foreign_run = RunId::new();
    let mut bridge = NotificationStreamBridge::new(run_id, "instance-13", "epoch-13").unwrap();
    bridge
        .ingest(&frame(
            1,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            1,
        ))
        .unwrap();
    let before = bridge.cursor().clone();
    let foreign = with_event(
        frame(
            2,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            2,
        ),
        serde_json::json!({"type": "delta", "run_id": foreign_run.to_string(), "text": "foreign"}),
    );
    assert_eq!(
        bridge.ingest(&foreign),
        Err(NotificationStreamError::ForeignRun)
    );
    assert_eq!(bridge.cursor(), &before);
    assert_eq!(bridge.merge_window_len(), 0);
    // A payload that names an unreadable run is malformed rather than silently accepted.
    let malformed = with_event(
        frame(
            2,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            2,
        ),
        serde_json::json!({"type": "delta", "run_id": 7, "text": "bad"}),
    );
    assert_eq!(
        bridge.ingest(&malformed),
        Err(NotificationStreamError::Invalid("frame_run".to_owned()))
    );
    assert_eq!(bridge.cursor(), &before);
    // The bridge's own run still flows through the after-cursor path.
    let mine = with_event(
        frame(
            2,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            2,
        ),
        serde_json::json!({"type": "delta", "run_id": run_id.to_string(), "text": "mine"}),
    );
    assert_eq!(
        bridge.ingest(&mine),
        Ok(NotificationStreamDisposition::Accepted {
            feed_sequence: 2,
            source_cursor: 2
        })
    );
}

#[test]
fn a_feed_that_ends_without_a_terminal_raises_the_query_alarm() {
    let mut bridge =
        NotificationStreamBridge::new(RunId::new(), "instance-13", "epoch-13").unwrap();
    bridge
        .ingest(&frame(
            1,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            1,
        ))
        .unwrap();
    // A recorded error delta is not a terminal fact: the run may still be alive.
    let failed = with_event(
        frame(
            2,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            2,
        ),
        serde_json::json!({
            "type": "error",
            "run_id": bridge.cursor().run_id.to_string(),
            "data": {},
        }),
    );
    assert_eq!(
        bridge.ingest(&failed),
        Ok(NotificationStreamDisposition::Accepted {
            feed_sequence: 2,
            source_cursor: 2
        })
    );
    assert!(!bridge.terminal_observed());
    assert!(!bridge.terminal_query_required());
    bridge.dispose();
    assert_eq!(
        bridge.ingest(&frame(
            3,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            3
        )),
        Ok(NotificationStreamDisposition::Disposed)
    );
    // The feed ended without a terminal observation: the surface must re-read `run.finished`
    // instead of presenting the run as finished or as still running.
    assert!(bridge.terminal_query_required());
    assert!(!bridge.terminal_observed());

    // With the terminal observed the same shutdown needs no query.
    let mut complete =
        NotificationStreamBridge::new(RunId::new(), "instance-13", "epoch-13").unwrap();
    complete
        .ingest(&frame(
            0,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            1,
        ))
        .unwrap();
    assert_eq!(
        complete.ingest(&frame(
            1,
            UiFeedFrameKind::Terminal,
            true,
            "instance-13",
            "epoch-13",
            1
        )),
        Ok(NotificationStreamDisposition::Terminal)
    );
    complete.dispose();
    assert!(complete.terminal_observed());
    assert!(!complete.terminal_query_required());
}

#[test]
fn the_event_id_memory_is_bounded_and_the_oldest_id_is_forgotten_first() {
    let mut bridge =
        NotificationStreamBridge::new(RunId::new(), "instance-13", "epoch-13").unwrap();
    bridge
        .ingest(&frame(
            0,
            UiFeedFrameKind::SnapshotBoundary,
            false,
            "instance-13",
            "epoch-13",
            1,
        ))
        .unwrap();
    // One frame per position fills the dedupe ring past its bound.
    for sequence in 1..=(NOTIFICATION_STREAM_SEEN_EVENT_IDS as u64 + 1) {
        assert!(matches!(
            bridge.ingest(&frame(
                sequence,
                UiFeedFrameKind::Delta,
                false,
                "instance-13",
                "epoch-13",
                sequence
            )),
            Ok(NotificationStreamDisposition::Accepted { .. })
        ));
    }
    // The oldest id was evicted: its position can no longer be replayed, so the feed reports the
    // position instead.  The newest id is still remembered.
    let newest = bridge.cursor().feed_sequence;
    let republished_newest = with_event_id(
        frame(
            newest + 1,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            newest + 1,
        ),
        &format!("event-{newest}"),
    );
    assert_eq!(
        bridge.ingest(&republished_newest),
        Ok(NotificationStreamDisposition::Replayed)
    );
    assert_eq!(bridge.cursor().feed_sequence, newest);
    // An evicted id is no longer recognized, which is the documented trade-off of the bound: the
    // bridge keeps a bounded memory instead of letting a long run grow it forever.
    let forgotten = with_event_id(
        frame(
            newest + 1,
            UiFeedFrameKind::Delta,
            false,
            "instance-13",
            "epoch-13",
            newest + 1,
        ),
        "event-1",
    );
    assert_eq!(
        bridge.ingest(&forgotten),
        Ok(NotificationStreamDisposition::Accepted {
            feed_sequence: newest + 1,
            source_cursor: newest + 1
        })
    );
}

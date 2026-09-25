use kiana_daemon::{
    NotificationStreamBridge, NotificationStreamDisposition, NotificationStreamError,
    NOTIFICATION_STREAM_BRIDGE_SCHEMA,
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
        Err(NotificationStreamError::TerminalUpdate)
    );
}

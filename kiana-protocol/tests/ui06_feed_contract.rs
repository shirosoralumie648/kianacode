use kiana_protocol::{
    UiCursor, UiFeedCursorV1, UiFeedFrameKind, UiFeedFrameV1, UiFeedGapReason, UiFeedGapV1,
    UI_FEED_FRAME_SCHEMA, UI_FEED_GAP_SCHEMA,
};

fn cursor(instance: &str, epoch: &str, sequence: u64) -> UiFeedCursorV1 {
    UiFeedCursorV1::new(
        instance,
        epoch,
        sequence,
        UiCursor {
            epoch: if sequence == 0 {
                String::new()
            } else {
                epoch.to_owned()
            },
            sequence,
        },
    )
    .unwrap()
}

#[test]
fn feed_cursor_round_trip_and_gap_are_explicit() {
    let before = cursor("instance-1", "epoch-1", 2);
    let after = cursor("instance-1", "epoch-1", 5);
    let encoded = before.encode().unwrap();
    assert_eq!(UiFeedCursorV1::decode(&encoded).unwrap(), before);
    let gap = UiFeedGapV1 {
        schema: UI_FEED_GAP_SCHEMA.to_owned(),
        reason: UiFeedGapReason::SequenceGap,
        from: Some(before.clone()),
        to: after.clone(),
        snapshot_required: true,
    };
    gap.validate().unwrap();
    let frame = UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        kind: UiFeedFrameKind::Gap,
        cursor: after,
        event_id: "gap-1".to_owned(),
        replay: false,
        terminal: false,
        event: None,
        gap: Some(gap),
    };
    frame.validate().unwrap();
}

#[test]
fn feed_cursor_digest_and_terminal_boundary_fail_closed() {
    let mut forged = cursor("instance-1", "epoch-1", 4);
    forged.feed_sequence = 5;
    assert_eq!(
        forged.validate().unwrap_err(),
        "ui_feed_cursor_digest_mismatch"
    );

    let cursor = cursor("instance-1", "epoch-1", 1);
    let frame = UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        kind: UiFeedFrameKind::Delta,
        cursor,
        event_id: "terminal-forged".to_owned(),
        replay: false,
        terminal: true,
        event: None,
        gap: None,
    };
    assert_eq!(
        frame.validate().unwrap_err(),
        "ui_feed_frame_terminal_mismatch"
    );
}

//! NM-20 channel/UI fixtures: the two notification-chain fault cases whose seam lives in the
//! daemon-owned stream bridge.
//!
//! The card names the whole chain (EventLog/Projector/Worker/Channel/UI). `kiana-core` cannot
//! link this crate's `NotificationStreamBridge`, so the two channel/UI cases are driven here and
//! composed into the same [`Nm20FaultHarnessRun`] shape:
//!
//! - `slow_subscriber`: a surface that never hydrated (or that overflowed the bounded merge
//!   window) must not render a delta as content, and an overflow must fail closed rather than
//!   report a partial merge.
//! - `multi_subscriber_isolation`: one subscriber's gap must not move another subscriber's
//!   cursor, and the lagging subscriber must re-read the authoritative facts instead of treating
//!   the gap as a terminal.
//!
//! The EventLog-backed cases (`crash_after_claim`, `bounded_queue_shed`) are driven by the
//! `kiana-core` fixture target; here they are reported as missing, never faked. Nothing in this
//! target executes a notification action, writes a fact or starts a second bus.

use kiana_core::{
    Nm20FaultCase, Nm20FaultHarnessRun, Nm20FaultObservation, Nm20FaultSeam, Nm20LibrarySeam,
};
use kiana_daemon::{
    NotificationStreamBridge, NotificationStreamDisposition, NOTIFICATION_STREAM_MERGE_WINDOW,
};
use kiana_domain::RunId;
use kiana_protocol::{
    UiCursor, UiFeedCursorV1, UiFeedFrameKind, UiFeedFrameV1, UiFeedGapReason, UiFeedGapV1,
    UI_FEED_FRAME_SCHEMA, UI_FEED_GAP_SCHEMA,
};

const INSTANCE: &str = "instance-nm20";
const EPOCH: &str = "epoch-nm20";

fn frame(
    sequence: u64,
    kind: UiFeedFrameKind,
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
    .expect("feed cursor");
    // Read the terminal flag before `kind` is moved into the frame.
    let terminal = kind == UiFeedFrameKind::Terminal;
    UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        kind,
        cursor,
        event_id: format!("event-{sequence}"),
        replay: false,
        terminal,
        event: None,
        gap: None,
    }
}

fn gap_frame(sequence: u64, reason: UiFeedGapReason) -> UiFeedFrameV1 {
    let mut frame = frame(sequence, UiFeedFrameKind::Gap, INSTANCE, EPOCH, sequence);
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

/// A daemon-side seam for the two channel/UI cases; everything else stays with the library.
struct ChannelSeam {
    inner: Nm20LibrarySeam,
}

impl Nm20FaultSeam for ChannelSeam {
    fn inject(&mut self, case: Nm20FaultCase) -> Result<Nm20FaultObservation, String> {
        match case {
            Nm20FaultCase::SlowSubscriber => drive_slow_subscriber(),
            Nm20FaultCase::MultiSubscriberIsolation => drive_multi_subscriber_isolation(),
            other => self.inner.inject(other),
        }
    }
}

/// The ten cases this target can reach: eight library cases plus the two channel/UI cases. The
/// two EventLog-backed cases are named as missing so the report never over-claims.
fn daemon_run() -> Nm20FaultHarnessRun {
    let mut seam = ChannelSeam {
        inner: Nm20LibrarySeam,
    };
    let mut cases = Vec::new();
    for case in Nm20FaultCase::ALL {
        match seam.inject(case) {
            Ok(observation) => cases.push(observation),
            Err(error) if error.starts_with("nm20_adapter_seam_required:") => continue,
            Err(error) => panic!("nm20 channel seam failed on {}: {error}", case.as_str()),
        }
    }
    Nm20FaultHarnessRun::from_observations(
        20,
        3,
        vec!["sha256:".to_owned() + &"1".repeat(64)],
        cases,
    )
    .expect("nm20 daemon run")
}

/// Slow consumer: an unhydrated surface renders nothing, and an overflowing merge window fails
/// closed instead of reporting a partial merge.
fn drive_slow_subscriber() -> Result<Nm20FaultObservation, String> {
    let mut bridge = NotificationStreamBridge::new(RunId::new(), INSTANCE, EPOCH)
        .map_err(|error| error.to_string())?;

    // Before hydration a delta is not content.
    match bridge
        .ingest(&frame(1, UiFeedFrameKind::Delta, INSTANCE, EPOCH, 1))
        .map_err(|error| error.to_string())?
    {
        NotificationStreamDisposition::SnapshotRequired(UiFeedGapReason::ReplayExpired) => {}
        other => {
            return Err(format!(
                "nm20_slow_subscriber_first_delta_not_refused:{other:?}"
            ))
        }
    }
    if !bridge.snapshot_required() || bridge.merged_frames_len() != 0 {
        return Err("nm20_slow_subscriber_unhydrated_state_invalid".to_owned());
    }

    // Hydrate, then hit a gap: the window stays closed until a fresh boundary.
    match bridge
        .ingest(&frame(
            0,
            UiFeedFrameKind::SnapshotBoundary,
            INSTANCE,
            EPOCH,
            0,
        ))
        .map_err(|error| error.to_string())?
    {
        NotificationStreamDisposition::SnapshotBoundary => {}
        other => {
            return Err(format!(
                "nm20_slow_subscriber_boundary_not_adopted:{other:?}"
            ))
        }
    }
    match bridge
        .ingest(&gap_frame(1, UiFeedGapReason::SequenceGap))
        .map_err(|error| error.to_string())?
    {
        NotificationStreamDisposition::SnapshotRequired(UiFeedGapReason::SequenceGap) => {}
        other => return Err(format!("nm20_slow_subscriber_gap_not_refused:{other:?}")),
    }

    // Frames that race the fresh boundary are buffered up to the bounded window, then the
    // outlook fails closed: no partial merge, and the snapshot requirement survives.
    for sequence in 2..=(NOTIFICATION_STREAM_MERGE_WINDOW as u64 + 2) {
        match bridge
            .ingest(&frame(
                sequence,
                UiFeedFrameKind::Delta,
                INSTANCE,
                EPOCH,
                sequence,
            ))
            .map_err(|error| error.to_string())?
        {
            NotificationStreamDisposition::SnapshotRequired(UiFeedGapReason::ReplayExpired) => {}
            other => {
                return Err(format!(
                    "nm20_slow_subscriber_racy_frame_renderable:{other:?}"
                ))
            }
        }
    }
    if bridge.merge_window_len() != 0 || bridge.merged_frames_len() != 0 {
        return Err("nm20_slow_subscriber_partial_merge_reported".to_owned());
    }
    if !bridge.snapshot_required() {
        return Err("nm20_slow_subscriber_overflow_cleared_requirement".to_owned());
    }

    // Recovering with a fresh boundary yields no phantom frames.
    match bridge
        .ingest(&frame(
            NOTIFICATION_STREAM_MERGE_WINDOW as u64 + 3,
            UiFeedFrameKind::SnapshotBoundary,
            INSTANCE,
            EPOCH,
            NOTIFICATION_STREAM_MERGE_WINDOW as u64 + 3,
        ))
        .map_err(|error| error.to_string())?
    {
        NotificationStreamDisposition::SnapshotBoundary => {}
        other => return Err(format!("nm20_slow_subscriber_recovery_failed:{other:?}")),
    }
    if bridge.snapshot_required() || bridge.merged_frames_len() != 0 {
        return Err("nm20_slow_subscriber_recovery_state_invalid".to_owned());
    }

    Nm20FaultObservation::new(
        Nm20FaultCase::SlowSubscriber,
        "kiana_daemon::NotificationStreamBridge::ingest",
        true,
        "notification_stream_backpressure_fail_closed",
        Some("slow consumer blocking the feed or a dropped delta rendered as a fact"),
        false,
        true,
        false,
        true,
        false,
        true,
    )
}

/// Multi-subscriber isolation: one surface's gap must not move the other's cursor or terminal,
/// and the lagging surface must ask for the authoritative facts.
fn drive_multi_subscriber_isolation() -> Result<Nm20FaultObservation, String> {
    let run_id = RunId::new();
    let mut lagging = NotificationStreamBridge::new(run_id, INSTANCE, EPOCH)
        .map_err(|error| error.to_string())?;
    let mut healthy = NotificationStreamBridge::new(run_id, INSTANCE, EPOCH)
        .map_err(|error| error.to_string())?;

    for surface in [&mut lagging, &mut healthy] {
        match surface
            .ingest(&frame(
                0,
                UiFeedFrameKind::SnapshotBoundary,
                INSTANCE,
                EPOCH,
                0,
            ))
            .map_err(|error| error.to_string())?
        {
            NotificationStreamDisposition::SnapshotBoundary => {}
            other => return Err(format!("nm20_isolation_boundary_failed:{other:?}")),
        }
    }

    for sequence in 1..=3u64 {
        for surface in [&mut lagging, &mut healthy] {
            match surface
                .ingest(&frame(
                    sequence,
                    UiFeedFrameKind::Delta,
                    INSTANCE,
                    EPOCH,
                    sequence,
                ))
                .map_err(|error| error.to_string())?
            {
                NotificationStreamDisposition::Accepted { .. } => {}
                other => return Err(format!("nm20_isolation_delta_failed:{other:?}")),
            }
        }
    }

    // The lagging surface misses frames 4 and 5; observing 6 must demand a snapshot, not
    // complete the run and not touch its own cursor.
    match lagging
        .ingest(&frame(6, UiFeedFrameKind::Delta, INSTANCE, EPOCH, 6))
        .map_err(|error| error.to_string())?
    {
        NotificationStreamDisposition::SnapshotRequired(UiFeedGapReason::SequenceGap) => {}
        other => return Err(format!("nm20_isolation_gap_not_refused:{other:?}")),
    }
    if lagging.terminal_observed() || lagging.cursor().feed_sequence != 3 {
        return Err("nm20_isolation_lagging_cursor_mutated".to_owned());
    }
    if !lagging.terminal_query_required() {
        return Err("nm20_isolation_query_fallback_missing".to_owned());
    }

    // The healthy surface receives the same frames in order and its own terminal.
    for sequence in 4..=6u64 {
        match healthy
            .ingest(&frame(
                sequence,
                UiFeedFrameKind::Delta,
                INSTANCE,
                EPOCH,
                sequence,
            ))
            .map_err(|error| error.to_string())?
        {
            NotificationStreamDisposition::Accepted { .. } => {}
            other => return Err(format!("nm20_isolation_healthy_delta_failed:{other:?}")),
        }
    }
    match healthy
        .ingest(&frame(7, UiFeedFrameKind::Terminal, INSTANCE, EPOCH, 7))
        .map_err(|error| error.to_string())?
    {
        NotificationStreamDisposition::Terminal => {}
        other => return Err(format!("nm20_isolation_terminal_failed:{other:?}")),
    }
    if !healthy.terminal_observed() || healthy.snapshot_required() {
        return Err("nm20_isolation_healthy_terminal_invalid".to_owned());
    }
    if healthy.cursor().feed_sequence != 7 || healthy.cursor().source_cursor != 7 {
        return Err("nm20_isolation_healthy_cursor_invalid".to_owned());
    }
    if lagging.terminal_observed() {
        return Err("nm20_isolation_gap_became_a_terminal".to_owned());
    }

    // A disposed surface asks for the query instead of pretending the run finished.
    lagging.dispose();
    if !lagging.terminal_query_required() {
        return Err("nm20_isolation_disposed_query_missing".to_owned());
    }
    match lagging
        .ingest(&frame(8, UiFeedFrameKind::Terminal, INSTANCE, EPOCH, 8))
        .map_err(|error| error.to_string())?
    {
        NotificationStreamDisposition::Disposed => {}
        other => return Err(format!("nm20_isolation_disposed_ingest:{other:?}")),
    }

    Nm20FaultObservation::new(
        Nm20FaultCase::MultiSubscriberIsolation,
        "kiana_daemon::NotificationStreamBridge (two surfaces)",
        true,
        "notification_stream_subscriber_isolation",
        Some("one subscriber's gap mutating another subscriber's cursor or terminal"),
        false,
        true,
        false,
        true,
        false,
        true,
    )
}

#[test]
fn slow_subscriber_is_bounded_and_never_renders_a_delta_before_hydration() {
    let observation = drive_slow_subscriber().expect("slow subscriber case");
    observation.validate().expect("observation valid");
    assert_eq!(observation.case, Nm20FaultCase::SlowSubscriber);
    assert!(observation.snapshot_required);
    assert!(observation.refusal_fired);
    assert_eq!(
        observation.refusal_code,
        "notification_stream_backpressure_fail_closed"
    );
    assert_eq!(
        observation.forbidden_value.as_deref(),
        Some("slow consumer blocking the feed or a dropped delta rendered as a fact")
    );
    assert!(!observation.effect_started);
    assert!(observation.secret_free);
    assert!(observation.critical_preserved);
}

#[test]
fn multi_subscriber_isolation_keeps_one_gap_out_of_the_other_cursor() {
    let observation = drive_multi_subscriber_isolation().expect("multi subscriber case");
    observation.validate().expect("observation valid");
    assert_eq!(observation.case, Nm20FaultCase::MultiSubscriberIsolation);
    assert_eq!(
        observation.refusal_code,
        "notification_stream_subscriber_isolation"
    );
    assert!(observation.query_fallback_available);
    assert!(!observation.effect_started);
    assert!(observation.secret_free);
    assert!(observation.critical_preserved);
}

#[test]
fn daemon_chain_run_covers_channel_cases_and_reports_eventlog_cases_missing() {
    let run = daemon_run();
    run.validate().expect("daemon run valid");
    assert_eq!(run.cases.len(), 10);
    assert_eq!(
        run.missing_cases(),
        vec![
            Nm20FaultCase::CrashAfterClaim,
            Nm20FaultCase::BoundedQueueShed
        ],
        "the EventLog-backed cases are driven by the kiana-core fixture target"
    );
    for case in [
        Nm20FaultCase::SlowSubscriber,
        Nm20FaultCase::MultiSubscriberIsolation,
    ] {
        let observation = run.observation(case).expect("channel case covered");
        assert!(!observation.effect_started);
        assert!(observation.secret_free);
        assert!(observation.critical_preserved);
    }
}

#[test]
fn daemon_chain_report_is_replayable_and_effect_free() {
    let first = daemon_run();
    let second = daemon_run();
    assert_eq!(first, second);
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap()
    );
    for observation in &first.cases {
        assert!(!observation.effect_started, "an effect was started");
        assert!(observation.secret_free, "a secret reached the report");
        assert!(observation.critical_preserved, "a critical fact was lost");
    }
}

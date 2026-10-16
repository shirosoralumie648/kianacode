use kiana_domain::{
    canonical_journal_bytes, AggregateVersion, CommandReceipt, EventId, JournalFrame,
    JournalFramePayload, RequestId, RuntimeEvent, TransitionBatch,
};
use serde_json::json;

#[test]
fn complete_journal_frames_validate_body_and_expand_only_committed_events() {
    let request_id = RequestId::new();
    let event = RuntimeEvent::new(request_id, 1, "run.accepted", json!({"ok":true}))
        .unwrap()
        .with_stream_metadata("run", "run-1", 1);
    let command_digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let batch = TransitionBatch {
        command_id: request_id,
        command_digest: command_digest.to_owned(),
        expected_versions: vec![AggregateVersion::new("run", "run-1", 0)],
        events: vec![event.clone()],
    };
    let receipt = CommandReceipt {
        command_id: request_id,
        command_digest: command_digest.to_owned(),
        commit_id: EventId::new(),
        first_cursor: 1,
        cursor: 1,
        event_ids: vec![event.event_id],
        versions: vec![AggregateVersion::new("run", "run-1", 1)],
    };
    let frame = JournalFrame::new(JournalFramePayload::Transition { batch, receipt }).unwrap();
    frame.validate().unwrap();
    assert_eq!(frame.logical_events(), vec![event]);
    assert!(canonical_journal_bytes(&frame).unwrap().len() as u64 > frame.body_len);
}

#[test]
fn journal_frame_rejects_tampered_receipt_unknown_body_and_nil_event() {
    let request_id = RequestId::new();
    let event = RuntimeEvent::new(request_id, 1, "run.accepted", json!({"ok":true}))
        .unwrap()
        .with_stream_metadata("run", "run-1", 1);
    let frame = JournalFrame::new(JournalFramePayload::Event { event }).unwrap();
    let mut tampered = frame.clone();
    tampered.body_len += 1;
    assert_eq!(
        tampered.validate().unwrap_err(),
        "journal_frame_integrity_failed"
    );

    let mut nil = frame.clone();
    if let JournalFramePayload::Event { event } = &mut nil.body {
        event.event_id = EventId::from_uuid(uuid::Uuid::nil());
        nil.body_sha256 = kiana_domain::journal_sha256(
            &kiana_domain::canonical_journal_bytes(&nil.body).unwrap(),
        );
        nil.body_len = kiana_domain::canonical_journal_bytes(&nil.body)
            .unwrap()
            .len() as u64;
    }
    assert_eq!(nil.validate().unwrap_err(), "journal_frame_event_invalid");

    let mut unknown = serde_json::to_value(&frame).unwrap();
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<JournalFrame>(unknown).is_err());
}

#[test]
fn transition_frame_rejects_nil_event_identity() {
    for (nil_event_id, nil_request_id) in [(true, false), (false, true)] {
        let request_id = RequestId::new();
        let mut event = RuntimeEvent::new(request_id, 1, "run.accepted", json!({"ok":true}))
            .unwrap()
            .with_stream_metadata("run", "run-1", 1);
        if nil_event_id {
            event.event_id = EventId::from_uuid(uuid::Uuid::nil());
        }
        if nil_request_id {
            event.request_id = RequestId::from_uuid(uuid::Uuid::nil());
        }
        let command_digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let batch = TransitionBatch {
            command_id: request_id,
            command_digest: command_digest.to_owned(),
            expected_versions: vec![AggregateVersion::new("run", "run-1", 0)],
            events: vec![event.clone()],
        };
        let receipt = CommandReceipt {
            command_id: request_id,
            command_digest: command_digest.to_owned(),
            commit_id: EventId::new(),
            first_cursor: 1,
            cursor: 1,
            event_ids: vec![event.event_id],
            versions: vec![AggregateVersion::new("run", "run-1", 1)],
        };
        let frame = JournalFrame::new(JournalFramePayload::Transition { batch, receipt }).unwrap();
        assert_eq!(
            frame.validate().unwrap_err(),
            "journal_event_identity_invalid"
        );
    }
}

#[test]
fn transition_frame_rejects_nil_command_and_receipt_commit_ids() {
    let request_id = RequestId::new();
    let event = RuntimeEvent::new(request_id, 1, "run.accepted", json!({"ok":true}))
        .unwrap()
        .with_stream_metadata("run", "run-1", 1);
    let command_digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let batch = TransitionBatch {
        command_id: request_id,
        command_digest: command_digest.to_owned(),
        expected_versions: vec![AggregateVersion::new("run", "run-1", 0)],
        events: vec![event.clone()],
    };
    let receipt = CommandReceipt {
        command_id: request_id,
        command_digest: command_digest.to_owned(),
        commit_id: EventId::new(),
        first_cursor: 1,
        cursor: 1,
        event_ids: vec![event.event_id],
        versions: vec![AggregateVersion::new("run", "run-1", 1)],
    };

    let mut nil_command = batch.clone();
    nil_command.command_id = RequestId::from_uuid(uuid::Uuid::nil());
    let mut matching_receipt = receipt.clone();
    matching_receipt.command_id = nil_command.command_id;
    let frame = JournalFrame::new(JournalFramePayload::Transition {
        batch: nil_command,
        receipt: matching_receipt,
    })
    .unwrap();
    let command_id_result = frame.validate().err();

    let mut nil_commit = receipt;
    nil_commit.commit_id = EventId::from_uuid(uuid::Uuid::nil());
    let frame = JournalFrame::new(JournalFramePayload::Transition {
        batch,
        receipt: nil_commit,
    })
    .unwrap();
    let commit_id_result = frame.validate().err();
    assert_eq!(
        [command_id_result.as_deref(), commit_id_result.as_deref()],
        [
            Some("journal_command_id_invalid"),
            Some("command_receipt_commit_id_invalid")
        ]
    );
}

#[test]
fn legacy_event_frame_remains_single_event_but_never_accepts_zero_sequence() {
    let event = RuntimeEvent::new(RequestId::new(), 1, "legacy", json!(null)).unwrap();
    let frame = JournalFrame::new(JournalFramePayload::Event { event }).unwrap();
    assert_eq!(frame.logical_events().len(), 1);
}

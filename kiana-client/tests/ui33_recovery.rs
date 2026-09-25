use kiana_client::{
    plan_recovery, FeedDisposition, RecoveryDecision, RecoveryError, RecoveryFault, RecoveryFence,
    RecoveryInput, RecoveryPhase, UI_RECOVERY_SCHEMA,
};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn input(phase: RecoveryPhase, fault: RecoveryFault) -> RecoveryInput {
    RecoveryInput {
        schema: UI_RECOVERY_SCHEMA.to_owned(),
        phase,
        fault,
        instance_id: "instance-33".to_owned(),
        epoch: "epoch-33".to_owned(),
        sequence: 3,
        command_id: "command-33".to_owned(),
        terminal_status: (phase == RecoveryPhase::Terminal).then(|| "unknown".to_owned()),
    }
}

#[test]
fn fixture_declares_recovery_deny_first_contract() {
    let value: Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ui33-recovery.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(value["schema"], "kiana.ui-recovery-fixture.v1");
    assert!(value["denied"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains("implicitly")));
}

#[test]
fn recovery_plans_preserve_command_and_never_allow_new_effect() {
    let accepted = plan_recovery(&input(
        RecoveryPhase::ActionAccepted,
        RecoveryFault::Disconnect,
    ))
    .unwrap();
    assert_eq!(accepted.decision, RecoveryDecision::QueryOriginalCommand);
    assert_eq!(accepted.command_id, "command-33");
    assert!(!accepted.new_effect_allowed);

    let gap = plan_recovery(&input(RecoveryPhase::Feed, RecoveryFault::Gap)).unwrap();
    assert_eq!(gap.decision, RecoveryDecision::HydrateSnapshot);
    assert!(gap.snapshot_required);
    assert!(!gap.new_effect_allowed);

    let unknown =
        plan_recovery(&input(RecoveryPhase::Cancel, RecoveryFault::WorkerKilled)).unwrap();
    assert_eq!(unknown.decision, RecoveryDecision::QueryOriginalCommand);
    assert!(!unknown.new_effect_allowed);

    assert_eq!(
        plan_recovery(&RecoveryInput {
            terminal_status: None,
            ..input(RecoveryPhase::Terminal, RecoveryFault::Disconnect)
        }),
        Err(RecoveryError::TerminalStatusInvalid)
    );
}

#[test]
fn feed_fence_rejects_gap_old_epoch_and_late_terminal() {
    let mut fence = RecoveryFence::new("instance-33", "epoch-33").unwrap();
    assert_eq!(
        fence.accept_feed("epoch-33", 1, false),
        FeedDisposition::Accepted {
            sequence: 1,
            terminal: false
        }
    );
    assert_eq!(
        fence.accept_feed("epoch-33", 1, false),
        FeedDisposition::Duplicate { sequence: 1 }
    );
    assert_eq!(
        fence.accept_feed("epoch-33", 3, false),
        FeedDisposition::Gap {
            expected: 2,
            received: 3
        }
    );
    assert_eq!(
        fence.accept_feed("old-epoch", 2, false),
        FeedDisposition::OldEpoch
    );
    assert_eq!(
        fence.accept_feed("epoch-33", 2, true),
        FeedDisposition::Accepted {
            sequence: 2,
            terminal: true
        }
    );
    assert_eq!(
        fence.accept_feed("epoch-33", 3, true),
        FeedDisposition::LateAfterTerminal
    );
    assert_eq!(fence.last_sequence(), 2);
    assert!(fence.terminal_seen());
}

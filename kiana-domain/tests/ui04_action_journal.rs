use kiana_domain::{json_digest, RequestId, UiActionCommand, UiActionJournal, UiActionState};
use serde_json::json;

fn command(key: &str, payload: serde_json::Value) -> UiActionCommand {
    UiActionCommand::new(
        RequestId::new(),
        key,
        "run-target",
        "owner-1",
        json_digest(&json!({"scope":"run-1"})),
        "epoch-1",
        0,
        Some(4),
        10_000,
        payload,
    )
    .unwrap()
}

#[test]
fn action_journal_replays_exact_key_without_second_effect() {
    let mut journal = UiActionJournal::new();
    let action = command("ui-04-key", json!({"decision":"approve"}));
    let (accepted, replayed) = journal
        .accept(
            &action,
            "epoch-1",
            0,
            Some(4),
            "owner-1",
            &action.scope_digest,
            1_000,
        )
        .unwrap();
    assert!(!replayed);
    assert_eq!(accepted.state, UiActionState::Accepted);
    assert_eq!(accepted.effect_count, 0);

    let (same, replayed) = journal
        .accept(
            &action,
            "epoch-1",
            9,
            Some(99),
            "owner-1",
            &action.scope_digest,
            2_000,
        )
        .unwrap();
    assert!(replayed);
    assert_eq!(same, accepted);
    let applied = journal
        .apply(&action.idempotency_key, json_digest(&json!({"receipt":1})), 2_001)
        .unwrap();
    assert_eq!(applied.state, UiActionState::Applied);
    assert_eq!(applied.effect_count, 1);
    let applied_again = journal
        .apply(&action.idempotency_key, applied.receipt_digest.clone().unwrap(), 2_002)
        .unwrap();
    assert_eq!(applied_again.effect_count, 1);
}

#[test]
fn action_journal_rejects_changed_digest_owner_scope_and_cas() {
    let action = command("ui-04-conflict", json!({"decision":"approve"}));
    let mut journal = UiActionJournal::new();
    journal
        .accept(
            &action,
            "epoch-1",
            0,
            Some(4),
            "owner-1",
            &action.scope_digest,
            1_000,
        )
        .unwrap();

    let changed = command("ui-04-conflict", json!({"decision":"deny"}));
    assert_eq!(
        journal
            .accept(
                &changed,
                "epoch-1",
                0,
                Some(4),
                "owner-1",
                &changed.scope_digest,
                1_000,
            )
            .unwrap_err(),
        "ui_action_idempotency_digest_mismatch"
    );
    let stale = command("ui-04-stale", json!({"decision":"approve"}));
    assert_eq!(
        journal
            .accept(
                &stale,
                "epoch-old",
                0,
                Some(4),
                "owner-1",
                &stale.scope_digest,
                1_000,
            )
            .unwrap_err(),
        "ui_action_stale_epoch"
    );
    assert_eq!(
        journal
            .accept(
                &stale,
                "epoch-1",
                8,
                Some(4),
                "owner-1",
                &stale.scope_digest,
                1_000,
            )
            .unwrap_err(),
        "ui_action_stale_cursor"
    );
    assert_eq!(
        journal
            .accept(
                &stale,
                "epoch-1",
                0,
                Some(5),
                "owner-1",
                &stale.scope_digest,
                1_000,
            )
            .unwrap_err(),
        "ui_action_stale_revision"
    );
    assert_eq!(
        journal
            .accept(
                &stale,
                "epoch-1",
                0,
                Some(4),
                "other-owner",
                &stale.scope_digest,
                1_000,
            )
            .unwrap_err(),
        "ui_action_owner_mismatch"
    );
}

#[test]
fn unknown_requires_original_key_and_never_becomes_applied() {
    let action = command("ui-04-unknown", json!({"decision":"approve"}));
    let mut journal = UiActionJournal::new();
    journal
        .accept(
            &action,
            "epoch-1",
            0,
            Some(4),
            "owner-1",
            &action.scope_digest,
            1_000,
        )
        .unwrap();
    let unknown = journal
        .unknown(&action.idempotency_key, "ack_lost")
        .unwrap();
    assert_eq!(unknown.state, UiActionState::Unknown);
    assert_eq!(unknown.effect_count, 0);
    assert_eq!(
        journal
            .apply(&action.idempotency_key, json_digest(&json!({"receipt":1})), 1_001)
            .unwrap_err(),
        "ui_action_result_unknown"
    );
    assert!(journal.query_original("ui-04-unknown").is_some());
}

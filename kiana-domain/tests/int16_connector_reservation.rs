use kiana_domain::*;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn binding() -> ConnectorBindingSnapshot {
    ConnectorBindingSnapshot {
        definition: ConnectorDefinition {
            schema: "kiana.connector-definition.v1".to_owned(),
            connector_id: "connector-int16".to_owned(),
            version: "v1".to_owned(),
            provider_id: "provider-int16".to_owned(),
            adapter: "local_fixture".to_owned(),
            operations: BTreeMap::from([(
                "read".to_owned(),
                ConnectorOperation {
                    effect: ConnectorEffect::ReadOnly,
                    required_scope: "read".to_owned(),
                    data_classes: BTreeSet::new(),
                },
            )]),
            rate_limit_per_minute: 10,
            idempotency_required: true,
            reconciliation_required: true,
            data_processing: "local_only".to_owned(),
        },
        binding: AccountBinding {
            schema: "kiana.account-binding.v1".to_owned(),
            binding_id: "binding-int16".to_owned(),
            connector_id: "connector-int16".to_owned(),
            account_id: "account-int16".to_owned(),
            read_scopes: BTreeSet::from(["read".to_owned()]),
            write_scopes: BTreeSet::new(),
            fixture_path: "fixtures/int16.json".to_owned(),
            fixture_sha256: digest('f'),
            credential_ref: None,
        },
        project_root: "/tmp/int16".to_owned(),
        revision: 7,
        status: "active".to_owned(),
    }
}

fn command(
    binding: &ConnectorBindingSnapshot,
    payload: &serde_json::Value,
) -> ConnectorInvocationCommand {
    ConnectorInvocationCommand::from_payload(
        InvocationId::new(),
        1,
        binding,
        "read",
        digest('o'),
        4,
        "policy.v7",
        "config.v1",
        0,
        9,
        payload,
        "same-key",
    )
    .expect("command")
}

fn lease() -> ConnectorReservationLease {
    ConnectorReservationLease::new("lease-int16", "fence-int16", 100, 1_000).expect("lease")
}

#[test]
fn same_idempotency_key_with_different_command_digest_is_denied() {
    let snapshot = binding();
    let first = command(&snapshot, &json!({"value": 1}));
    let second = command(&snapshot, &json!({"value": 2}));
    assert_ne!(first.command_digest, second.command_digest);
    let mut ledger = ConnectorInvocationLedger::default();
    ledger
        .reserve(first, lease(), "reservation-1")
        .expect("first reserve");
    assert_eq!(
        ledger
            .reserve(second, lease(), "reservation-2")
            .unwrap_err(),
        "connector_idempotency_digest_conflict"
    );
}

#[test]
fn all_server_owned_revisions_are_rechecked_at_effect_boundary() {
    let snapshot = binding();
    let command = command(&snapshot, &json!({"value": 1}));
    command
        .validate_for_binding(&snapshot, &digest('o'), 4, "policy.v7", "config.v1", 0, 9)
        .expect("current revisions");

    let mut raced = snapshot.clone();
    raced.revision = 8;
    assert_eq!(
        command
            .validate_for_binding(&raced, &digest('o'), 4, "policy.v7", "config.v1", 0, 9)
            .unwrap_err(),
        "connector_reservation_binding_revision_mismatch"
    );
    assert_eq!(
        command
            .validate_for_binding(&snapshot, &digest('x'), 4, "policy.v7", "config.v1", 0, 9)
            .unwrap_err(),
        "connector_reservation_owner_revision_mismatch"
    );
    assert_eq!(
        command
            .validate_for_binding(&snapshot, &digest('o'), 5, "policy.v7", "config.v1", 0, 9)
            .unwrap_err(),
        "connector_reservation_authority_revision_mismatch"
    );
    assert_eq!(
        command
            .validate_for_binding(&snapshot, &digest('o'), 4, "policy.v8", "config.v1", 0, 9)
            .unwrap_err(),
        "connector_reservation_policy_revision_mismatch"
    );
    assert_eq!(
        command
            .validate_for_binding(&snapshot, &digest('o'), 4, "policy.v7", "config.v2", 0, 9)
            .unwrap_err(),
        "connector_reservation_configuration_revision_mismatch"
    );
    assert_eq!(
        command
            .validate_for_binding(&snapshot, &digest('o'), 4, "policy.v7", "config.v1", 0, 10)
            .unwrap_err(),
        "connector_reservation_data_revision_mismatch"
    );
}

#[test]
fn uncommitted_reservation_old_permit_and_replayed_receipt_have_zero_new_effect() {
    let snapshot = binding();
    let command = command(&snapshot, &json!({"value": 1}));
    let mut ledger = ConnectorInvocationLedger::default();
    let reservation = match ledger
        .reserve(command, lease(), "reservation-1")
        .expect("reserve")
    {
        ConnectorReservationOutcome::Reserved(value) => value,
        ConnectorReservationOutcome::Replayed(_) => panic!("unexpected replay"),
    };
    assert_eq!(
        ledger
            .issue_permit(&reservation.reservation_id)
            .unwrap_err(),
        "connector_reservation_not_committed"
    );
    ledger
        .commit(&reservation.reservation_id, 1)
        .expect("commit");
    let permit = ledger
        .issue_permit(&reservation.reservation_id)
        .expect("permit");
    assert_eq!(
        ledger.consume_permit(&permit, 200).expect("consume once"),
        ConnectorPermitConsumeOutcome::Consumed
    );
    assert!(ledger.consume_permit(&permit, 200).is_err());
    let receipt = ConnectorInvocationReceipt::new(
        &permit,
        Some("provider-receipt-1".to_owned()),
        ProviderOutcome::Succeeded,
        true,
        digest('r'),
    )
    .expect("receipt");
    assert_eq!(
        ledger.apply_receipt(receipt.clone()).expect("apply"),
        ConnectorReceiptApplyOutcome::Applied
    );
    assert_eq!(
        ledger.apply_receipt(receipt.clone()).expect("replay"),
        ConnectorReceiptApplyOutcome::Replayed
    );
    assert_eq!(
        ledger
            .replay_receipt(
                &reservation.command.idempotency_key_digest,
                &reservation.command.command_digest
            )
            .expect("lookup"),
        Some(receipt)
    );
}

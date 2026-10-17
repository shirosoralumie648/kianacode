use kiana_domain::*;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn digest(seed: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (seed as u32) & 0xffff).repeat(16)
    )
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
    assert_eq!(
        ledger.consume_permit(&permit, 200).unwrap_err(),
        "connector_permit_already_consumed"
    );
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

fn command_with_key(
    binding: &ConnectorBindingSnapshot,
    payload: &serde_json::Value,
    idempotency_key: &str,
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
        idempotency_key,
    )
    .expect("command")
}

fn expect_reserved(outcome: ConnectorReservationOutcome) -> ConnectorInvocationReservation {
    match outcome {
        ConnectorReservationOutcome::Reserved(reservation) => reservation,
        ConnectorReservationOutcome::Replayed(_) => panic!("expected a fresh reservation"),
    }
}

fn expect_replayed(outcome: ConnectorReservationOutcome) -> ConnectorInvocationReservation {
    match outcome {
        ConnectorReservationOutcome::Replayed(reservation) => reservation,
        ConnectorReservationOutcome::Reserved(_) => panic!("expected a replayed reservation"),
    }
}

#[test]
fn command_digest_covers_every_server_owned_field_and_refuses_tampering() {
    let snapshot = binding();
    let command = command_with_key(&snapshot, &json!({"value": 1}), "key-1");
    assert_eq!(connector_command_digest(&command), command.command_digest);
    assert_eq!(command.command_digest, command.digest());

    // The canonical identity separates the idempotency key and the payload: the same payload
    // under another key and the same key over another payload are different commands.
    let other_key = command_with_key(&snapshot, &json!({"value": 1}), "key-2");
    assert_ne!(
        other_key.idempotency_key_digest,
        command.idempotency_key_digest
    );
    assert_ne!(other_key.command_digest, command.command_digest);
    let other_payload = command_with_key(&snapshot, &json!({"value": 2}), "key-1");
    assert_eq!(
        other_payload.idempotency_key_digest,
        command.idempotency_key_digest
    );
    assert_ne!(other_payload.payload_digest, command.payload_digest);
    assert_ne!(other_payload.command_digest, command.command_digest);

    // A revision race is visible in the digest before any adapter call, and revalidating the
    // same command against the old binding is a typed denial.
    let mut raced = snapshot.clone();
    raced.revision = 8;
    let raced_command = command_with_key(&raced, &json!({"value": 1}), "key-1");
    assert_ne!(raced_command.command_digest, command.command_digest);
    assert_eq!(
        raced_command
            .validate_for_binding(&snapshot, &digest('o'), 4, "policy.v7", "config.v1", 0, 9)
            .unwrap_err(),
        "connector_reservation_binding_revision_mismatch"
    );

    // Overwriting the recorded digest without re-deriving it is refused outright, and the
    // digest survives a serialization round-trip unchanged.
    let mut tampered = command.clone();
    tampered.payload_digest = digest('z');
    assert_eq!(
        tampered.validate().unwrap_err(),
        "connector_invocation_command_invalid"
    );
    let restored: ConnectorInvocationCommand =
        serde_json::from_str(&serde_json::to_string(&command).expect("encode")).expect("decode");
    assert_eq!(restored, command);
    assert_eq!(restored.digest(), command.command_digest);
}

#[test]
fn same_command_replays_the_first_reservation_and_cas_races_are_denied() {
    let snapshot = binding();
    let command = command(&snapshot, &json!({"value": 1}));
    let mut ledger = ConnectorInvocationLedger::default();
    let reserved = expect_reserved(
        ledger
            .reserve(command.clone(), lease(), "reservation-1")
            .expect("reserve"),
    );
    assert_eq!(reserved.state, ConnectorReservationState::Reserved);
    assert_eq!(reserved.state_revision, 1);

    // An identical command under the same idempotency key replays the original reservation; a
    // second reservation ID or attempt is never minted.
    let replayed = expect_replayed(
        ledger
            .reserve(command.clone(), lease(), "reservation-2")
            .expect("replay"),
    );
    assert_eq!(replayed.reservation_id, "reservation-1");
    assert_eq!(replayed.reservation_digest, reserved.reservation_digest);

    // A different idempotency key may not reuse an existing reservation identity.
    let other = command_with_key(&snapshot, &json!({"value": 1}), "key-2");
    assert_eq!(
        ledger.reserve(other, lease(), "reservation-1").unwrap_err(),
        "connector_reservation_id_conflict"
    );

    // CAS: the expected state revision must be the one actually read.
    assert_eq!(
        ledger.commit("reservation-1", 0).unwrap_err(),
        "connector_reservation_cas_conflict"
    );
    assert_eq!(
        ledger.commit("reservation-1", 1).expect("commit"),
        ConnectorReservationCommitOutcome::Committed
    );
    assert_eq!(
        ledger.commit("reservation-1", 1).unwrap_err(),
        "connector_reservation_cas_conflict"
    );
    assert_eq!(
        ledger.commit("reservation-1", 2).expect("replayed commit"),
        ConnectorReservationCommitOutcome::Replayed
    );
    assert_eq!(
        ledger.commit("reservation-missing", 1).unwrap_err(),
        "connector_reservation_missing"
    );
}

#[test]
fn one_attempt_consumes_exactly_one_permit_and_uncommitted_work_cannot_consume() {
    let snapshot = binding();
    let command = command(&snapshot, &json!({"value": 1}));
    let mut ledger = ConnectorInvocationLedger::default();
    let reserved = expect_reserved(
        ledger
            .reserve(command, lease(), "reservation-1")
            .expect("reserve"),
    );

    // Uncommitted work can neither issue nor consume a permit: the state CAS is the authority.
    assert_eq!(
        ledger.issue_permit("reservation-1").unwrap_err(),
        "connector_reservation_not_committed"
    );
    let mut committed_clone = reserved.clone();
    committed_clone.state = ConnectorReservationState::Committed;
    committed_clone.state_revision = 2;
    let uncommitted_permit =
        ConnectorInvocationPermit::issue(&committed_clone).expect("permit from committed clone");
    assert_eq!(
        connector_effect_admission(&reserved, &uncommitted_permit, 200).unwrap_err(),
        "connector_reservation_not_committed"
    );
    // The reservation binding check refuses the same permit while the reservation is not
    // committed, so an uncommitted reservation can never be consumed through the ledger.
    assert_eq!(
        ledger.consume_permit(&uncommitted_permit, 200).unwrap_err(),
        "connector_permit_reservation_mismatch"
    );

    ledger.commit("reservation-1", 1).expect("commit");
    let permit = ledger.issue_permit("reservation-1").expect("permit");
    assert_eq!(
        ledger.issue_permit("reservation-1").unwrap_err(),
        "connector_attempt_permit_already_issued"
    );
    assert_eq!(
        ledger.consume_permit(&permit, 200).expect("consume"),
        ConnectorPermitConsumeOutcome::Consumed
    );
    assert_eq!(
        ledger.consume_permit(&permit, 200).unwrap_err(),
        "connector_permit_already_consumed"
    );
    // A cloned or deserialized replay of the same one-shot permit is refused too.
    let restored: ConnectorInvocationPermit =
        serde_json::from_str(&serde_json::to_string(&permit).expect("encode")).expect("decode");
    assert_eq!(
        ledger.consume_permit(&restored, 200).unwrap_err(),
        "connector_permit_already_consumed"
    );

    // A permit issued for another attempt of the same reservation can never be substituted.
    let attempt_two = ConnectorInvocationCommand::from_payload(
        InvocationId::new(),
        2,
        &snapshot,
        "read",
        digest('o'),
        4,
        "policy.v7",
        "config.v1",
        0,
        9,
        &json!({"value": 1}),
        "same-key",
    )
    .expect("attempt two command");
    let mut other_reservation =
        ConnectorInvocationReservation::new("reservation-1", attempt_two, lease())
            .expect("other reservation");
    other_reservation.state = ConnectorReservationState::Committed;
    other_reservation.state_revision = 2;
    assert_eq!(
        permit
            .validate_for_reservation(&other_reservation, Some(200))
            .unwrap_err(),
        "connector_permit_reservation_mismatch"
    );
    assert_eq!(
        connector_effect_admission(&other_reservation, &permit, 200).unwrap_err(),
        "connector_permit_reservation_mismatch"
    );
}

#[test]
fn stale_leases_permits_and_epochs_never_reach_the_adapter() {
    // An expired lease is denied by itself and by every reservation built on it.
    let expired =
        ConnectorReservationLease::new("lease-int16", "fence-int16", 100, 200).expect("lease");
    assert!(expired.validate(Some(199)).is_ok());
    assert_eq!(
        expired.validate(Some(200)).unwrap_err(),
        "connector_reservation_lease_expired"
    );

    let snapshot = binding();
    let command = command(&snapshot, &json!({"value": 1}));
    let mut ledger = ConnectorInvocationLedger::default();
    let reservation = expect_reserved(
        ledger
            .reserve(command, lease(), "reservation-1")
            .expect("reserve"),
    );
    ledger.commit("reservation-1", 1).expect("commit");
    let permit = ledger.issue_permit("reservation-1").expect("permit");

    // Before the lease window opens and after it closes the permit is expired, so no adapter
    // call can be admitted even though the reservation object itself was committed.
    assert!(connector_effect_admission(&reservation, &permit, 150).is_ok());
    assert_eq!(
        connector_effect_admission(&reservation, &permit, 99).unwrap_err(),
        "connector_permit_expired"
    );
    assert_eq!(
        connector_effect_admission(&reservation, &permit, 1_000).unwrap_err(),
        "connector_permit_expired"
    );
    assert_eq!(
        ledger.consume_permit(&permit, 1_000).unwrap_err(),
        "connector_permit_expired"
    );

    // A renewed lease does not revive the old permit: the whole binding must match.
    let mut refreshed = reservation.clone();
    refreshed.lease =
        ConnectorReservationLease::new("lease-int16", "fence-int16", 500, 2_000).expect("lease");
    refreshed.reservation_digest = refreshed.digest();
    assert_eq!(
        permit
            .validate_for_reservation(&refreshed, Some(600))
            .unwrap_err(),
        "connector_permit_reservation_mismatch"
    );
}

#[test]
fn receipt_conflicts_and_effects_before_consumption_are_refused() {
    let snapshot = binding();
    let command = command(&snapshot, &json!({"value": 1}));
    let mut ledger = ConnectorInvocationLedger::default();
    let reservation = expect_reserved(
        ledger
            .reserve(command, lease(), "reservation-1")
            .expect("reserve"),
    );
    ledger.commit("reservation-1", 1).expect("commit");
    let permit = ledger.issue_permit("reservation-1").expect("permit");

    // A receipt cannot be applied before the one-shot permit was consumed.
    let receipt = ConnectorInvocationReceipt::new(
        &permit,
        Some("provider-receipt-1".to_owned()),
        ProviderOutcome::Succeeded,
        true,
        digest('r'),
    )
    .expect("receipt");
    assert_eq!(
        ledger.apply_receipt(receipt.clone()).unwrap_err(),
        "connector_reservation_effect_not_admitted"
    );
    assert_eq!(
        ledger.consume_permit(&permit, 200).expect("consume"),
        ConnectorPermitConsumeOutcome::Consumed
    );
    assert_eq!(
        ledger.apply_receipt(receipt.clone()).expect("apply"),
        ConnectorReceiptApplyOutcome::Applied
    );
    assert_eq!(
        ledger.apply_receipt(receipt.clone()).expect("replay"),
        ConnectorReceiptApplyOutcome::Replayed
    );

    // A different terminal result under the same reservation is a conflict, never a rewrite.
    let conflicting = ConnectorInvocationReceipt::new(
        &permit,
        Some("provider-receipt-2".to_owned()),
        ProviderOutcome::Failed,
        true,
        digest('s'),
    )
    .expect("receipt");
    assert_eq!(
        ledger.apply_receipt(conflicting).unwrap_err(),
        "connector_idempotency_receipt_conflict"
    );

    // Receipt replay is only for the same command identity under the same key.
    assert_eq!(
        ledger
            .replay_receipt(
                &reservation.command.idempotency_key_digest,
                &reservation.command.command_digest
            )
            .expect("lookup"),
        Some(receipt)
    );
    assert_eq!(
        ledger
            .replay_receipt(&reservation.command.idempotency_key_digest, &digest('x'))
            .unwrap_err(),
        "connector_idempotency_digest_conflict"
    );
    assert_eq!(
        ledger
            .replay_receipt(&digest('y'), &digest('x'))
            .expect("unknown key"),
        None
    );
}

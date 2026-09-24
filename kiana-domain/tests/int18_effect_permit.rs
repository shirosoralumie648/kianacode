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
            connector_id: "connector-int18".to_owned(),
            version: "v1".to_owned(),
            provider_id: "provider-int18".to_owned(),
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
            binding_id: "binding-int18".to_owned(),
            connector_id: "connector-int18".to_owned(),
            account_id: "account-int18".to_owned(),
            read_scopes: BTreeSet::from(["read".to_owned()]),
            write_scopes: BTreeSet::new(),
            fixture_path: "fixtures/int18.json".to_owned(),
            fixture_sha256: digest('f'),
            credential_ref: None,
        },
        project_root: "/tmp/int18".to_owned(),
        revision: 7,
        status: "active".to_owned(),
    }
}

fn committed_reservation(snapshot: &ConnectorBindingSnapshot) -> ConnectorInvocationReservation {
    let command = ConnectorInvocationCommand::from_payload(
        InvocationId::new(),
        1,
        snapshot,
        "read",
        digest('o'),
        4,
        "policy.v7",
        "config.v1",
        0,
        9,
        &json!({"value": 1}),
        "int18-key",
    )
    .expect("command");
    let lease =
        ConnectorReservationLease::new("lease-int18", "fence-int18", 100, 1_000).expect("lease");
    let mut reservation = ConnectorInvocationReservation::new("reservation-int18", command, lease)
        .expect("reservation");
    reservation.state = ConnectorReservationState::Committed;
    reservation.state_revision = 2;
    reservation
}

fn fence(snapshot: &ConnectorBindingSnapshot) -> ConnectorEffectFence {
    ConnectorEffectFence::new(
        connector_effect_scope_digest(snapshot, "read").expect("scope"),
        4,
        3,
        5,
        0,
        9,
    )
    .expect("fence")
}

#[test]
fn effect_permit_binds_scope_digests_epochs_and_lease_expiry() {
    let snapshot = binding();
    let reservation = committed_reservation(&snapshot);
    let current = fence(&snapshot);
    let permit = ConnectorEffectPermit::issue(&reservation, &snapshot, &current, 200, 900)
        .expect("effect permit");
    permit
        .validate_for_effect(&reservation, &snapshot, &current, 300)
        .expect("current permit");

    for (mut stale, reason) in [
        (
            ConnectorEffectFence::new(permit.scope_digest.clone(), 5, 3, 5, 0, 9).unwrap(),
            "connector_effect_authority_epoch_stale",
        ),
        (
            ConnectorEffectFence::new(permit.scope_digest.clone(), 4, 4, 5, 0, 9).unwrap(),
            "connector_effect_configuration_epoch_stale",
        ),
        (
            ConnectorEffectFence::new(permit.scope_digest.clone(), 4, 3, 6, 0, 9).unwrap(),
            "connector_effect_policy_epoch_stale",
        ),
        (
            ConnectorEffectFence::new(permit.scope_digest.clone(), 4, 3, 5, 0, 10).unwrap(),
            "connector_effect_data_epoch_stale",
        ),
    ] {
        assert_eq!(
            permit
                .validate_for_effect(&reservation, &snapshot, &stale, 300)
                .unwrap_err(),
            reason
        );
        stale.scope_digest = digest('s');
        stale.fence_digest = stale.digest();
        assert_eq!(
            permit
                .validate_for_effect(&reservation, &snapshot, &stale, 300)
                .unwrap_err(),
            "connector_effect_scope_epoch_mismatch"
        );
    }
    assert_eq!(
        permit
            .validate_for_effect(&reservation, &snapshot, &current, 1_000)
            .unwrap_err(),
        "connector_effect_permit_expired"
    );

    let mut revoked = snapshot.clone();
    revoked.status = "revoked".to_owned();
    assert_eq!(
        permit
            .validate_for_effect(&reservation, &revoked, &current, 300)
            .unwrap_err(),
        "connector_effect_binding_revoked"
    );
}

#[test]
fn permit_cannot_be_minted_from_uncommitted_or_mismatched_snapshot() {
    let snapshot = binding();
    let mut reservation = committed_reservation(&snapshot);
    reservation.state = ConnectorReservationState::Reserved;
    let current = fence(&snapshot);
    assert_eq!(
        ConnectorEffectPermit::issue(&reservation, &snapshot, &current, 200, 900).unwrap_err(),
        "connector_effect_permit_reservation_not_committed"
    );

    let reservation = committed_reservation(&snapshot);
    let mut changed = snapshot.clone();
    changed.revision += 1;
    assert_eq!(
        ConnectorEffectPermit::issue(&reservation, &changed, &current, 200, 900).unwrap_err(),
        "connector_reservation_binding_revision_mismatch"
    );
}

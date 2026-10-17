//! INT-18 domain fixture: effect-time connector permit and epoch fencing.
//!
//! Deny-first: a permit minted from an uncommitted reservation, an old binding snapshot, a
//! revoked/rotated/reconfigured binding, a drifted scope digest, a stale authority/config/policy/
//! credential/data epoch, a substituted lease or an expired window never validates for the adapter
//! effect boundary.  Only the success case at the end of each test proves that a permit which
//! matches the freshly read fence is admitted.  No provider, adapter or network is involved.

use kiana_domain::*;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

const CONNECTOR_ID: &str = "connector-int18";
const CONNECTOR_VERSION: &str = "v1";
const BINDING_ID: &str = "binding-int18";
const ACCOUNT_ID: &str = "account-int18";
const OPERATION: &str = "read";
const BINDING_REVISION: u64 = 7;
const AUTHORITY_EPOCH: u64 = 4;
const CONFIGURATION_EPOCH: u64 = 3;
const POLICY_EPOCH: u64 = 5;
const DATA_EPOCH: u64 = 9;

fn digest(seed: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (seed as u32) & 0xffff).repeat(16)
    )
}

fn definition() -> ConnectorDefinition {
    ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: CONNECTOR_ID.to_owned(),
        version: CONNECTOR_VERSION.to_owned(),
        provider_id: "provider-int18".to_owned(),
        adapter: "local_fixture".to_owned(),
        operations: BTreeMap::from([(
            OPERATION.to_owned(),
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
    }
}

fn account_binding(credential: Option<SecretRef>) -> AccountBinding {
    AccountBinding {
        schema: "kiana.account-binding.v1".to_owned(),
        binding_id: BINDING_ID.to_owned(),
        connector_id: CONNECTOR_ID.to_owned(),
        account_id: ACCOUNT_ID.to_owned(),
        read_scopes: BTreeSet::from(["read".to_owned()]),
        write_scopes: BTreeSet::new(),
        fixture_path: "fixtures/int18.json".to_owned(),
        fixture_sha256: digest('f'),
        credential_ref: credential,
    }
}

fn binding_without_credential() -> ConnectorBindingSnapshot {
    ConnectorBindingSnapshot {
        definition: definition(),
        binding: account_binding(None),
        project_root: "/tmp/int18".to_owned(),
        revision: BINDING_REVISION,
        status: "active".to_owned(),
    }
}

fn credential_reference(generation: u64) -> SecretRef {
    SecretRef::new(
        "env",
        "INT18_FIXTURE_TOKEN",
        CONNECTOR_CREDENTIAL_PURPOSE,
        connector_credential_audience(CONNECTOR_ID, CONNECTOR_VERSION),
        generation,
    )
    .expect("credential reference")
}

fn binding_with_credential(generation: u64) -> ConnectorBindingSnapshot {
    ConnectorBindingSnapshot {
        definition: definition(),
        binding: account_binding(Some(credential_reference(generation))),
        project_root: "/tmp/int18".to_owned(),
        revision: BINDING_REVISION,
        status: "active".to_owned(),
    }
}

fn command(
    snapshot: &ConnectorBindingSnapshot,
    attempt: u32,
    credential_generation: u64,
) -> ConnectorInvocationCommand {
    ConnectorInvocationCommand::from_payload(
        InvocationId::new(),
        attempt,
        snapshot,
        OPERATION,
        digest('o'),
        AUTHORITY_EPOCH,
        "policy.v7",
        "config.v1",
        credential_generation,
        DATA_EPOCH,
        &json!({"value": 1}),
        "int18-key",
    )
    .expect("command")
}

fn committed_reservation(snapshot: &ConnectorBindingSnapshot) -> ConnectorInvocationReservation {
    reservation_from_command(
        snapshot,
        command(snapshot, 1, 0),
        "lease-int18",
        "fence-int18",
    )
}

fn reservation_from_command(
    snapshot: &ConnectorBindingSnapshot,
    command: ConnectorInvocationCommand,
    lease_id: &str,
    fence_token: &str,
) -> ConnectorInvocationReservation {
    let lease = ConnectorReservationLease::new(lease_id, fence_token, 100, 1_000).expect("lease");
    let mut reservation = ConnectorInvocationReservation::new(
        format!("reservation-{}", snapshot.binding.binding_id),
        command,
        lease,
    )
    .expect("reservation");
    reservation.state = ConnectorReservationState::Committed;
    reservation.state_revision = 2;
    reservation
}

fn fence(snapshot: &ConnectorBindingSnapshot, credential_epoch: u64) -> ConnectorEffectFence {
    ConnectorEffectFence::new(
        connector_effect_scope_digest(snapshot, OPERATION).expect("scope"),
        AUTHORITY_EPOCH,
        CONFIGURATION_EPOCH,
        POLICY_EPOCH,
        credential_epoch,
        DATA_EPOCH,
    )
    .expect("fence")
}

fn issue_permit(
    reservation: &ConnectorInvocationReservation,
    snapshot: &ConnectorBindingSnapshot,
    current: &ConnectorEffectFence,
) -> ConnectorEffectPermit {
    ConnectorEffectPermit::issue(reservation, snapshot, current, 200, 900).expect("effect permit")
}

/// Every permit variant below must change the canonical permit digest: the digest covers the whole
/// server-owned envelope, not only a subset of it.
fn permit_variants(permit: &ConnectorEffectPermit) -> Vec<ConnectorEffectPermit> {
    let mut variants = Vec::new();
    let mut variant = permit.clone();
    variant.reservation_id = "reservation-int18-other".to_owned();
    variants.push(variant);
    let mut variant = permit.clone();
    variant.attempt += 1;
    variants.push(variant);
    let mut variant = permit.clone();
    variant.connector_version = "v2".to_owned();
    variants.push(variant);
    let mut variant = permit.clone();
    variant.account_id = "account-int18-other".to_owned();
    variants.push(variant);
    let mut variant = permit.clone();
    variant.scope_digest = digest('s');
    variants.push(variant);
    let mut variant = permit.clone();
    variant.command_digest = digest('c');
    variants.push(variant);
    let mut variant = permit.clone();
    variant.payload_digest = digest('p');
    variants.push(variant);
    let mut variant = permit.clone();
    variant.idempotency_key_digest = digest('k');
    variants.push(variant);
    let mut variant = permit.clone();
    variant.owner_revision = digest('r');
    variants.push(variant);
    let mut variant = permit.clone();
    variant.policy_revision = "policy.v8".to_owned();
    variants.push(variant);
    let mut variant = permit.clone();
    variant.configuration_revision = "config.v2".to_owned();
    variants.push(variant);
    let mut variant = permit.clone();
    variant.authority_epoch += 1;
    variants.push(variant);
    let mut variant = permit.clone();
    variant.configuration_epoch += 1;
    variants.push(variant);
    let mut variant = permit.clone();
    variant.policy_epoch += 1;
    variants.push(variant);
    let mut variant = permit.clone();
    variant.credential_epoch += 1;
    variants.push(variant);
    let mut variant = permit.clone();
    variant.data_epoch += 1;
    variants.push(variant);
    let mut variant = permit.clone();
    variant.binding_revision += 1;
    variants.push(variant);
    let mut variant = permit.clone();
    variant.lease_id = "lease-int18-other".to_owned();
    variants.push(variant);
    let mut variant = permit.clone();
    variant.fence_token = "fence-int18-other".to_owned();
    variants.push(variant);
    let mut variant = permit.clone();
    variant.issued_at_unix_ms += 1;
    variants.push(variant);
    let mut variant = permit.clone();
    variant.expires_at_unix_ms -= 1;
    variants.push(variant);
    variants
}

#[test]
fn effect_permit_binds_scope_digests_epochs_and_lease_expiry() {
    let snapshot = binding_without_credential();
    let reservation = committed_reservation(&snapshot);
    let current = fence(&snapshot, 0);
    let permit = issue_permit(&reservation, &snapshot, &current);

    // The permit copies the immutable command/lease identity instead of re-deriving it.
    assert_eq!(permit.schema, "kiana.connector-effect-permit.v1");
    assert_eq!(permit.reservation_id, reservation.reservation_id);
    assert_eq!(permit.attempt, reservation.command.attempt);
    assert_eq!(permit.connector_id, reservation.command.connector_id);
    assert_eq!(
        permit.connector_version,
        reservation.command.connector_version
    );
    assert_eq!(permit.binding_id, reservation.command.binding_id);
    assert_eq!(permit.account_id, reservation.command.account_id);
    assert_eq!(permit.operation, reservation.command.operation);
    assert_eq!(permit.command_digest, reservation.command.command_digest);
    assert_eq!(permit.payload_digest, reservation.command.payload_digest);
    assert_eq!(
        permit.idempotency_key_digest,
        reservation.command.idempotency_key_digest
    );
    assert_eq!(permit.owner_revision, reservation.command.owner_revision);
    assert_eq!(permit.policy_revision, reservation.command.policy_revision);
    assert_eq!(
        permit.configuration_revision,
        reservation.command.configuration_revision
    );
    assert_eq!(
        permit.authority_epoch,
        reservation.command.authority_revision
    );
    assert_eq!(
        permit.credential_epoch,
        reservation.command.credential_generation
    );
    assert_eq!(permit.data_epoch, reservation.command.data_revision);
    assert_eq!(
        permit.binding_revision,
        reservation.command.binding_revision
    );
    assert_eq!(permit.lease_id, reservation.lease.lease_id);
    assert_eq!(permit.fence_token, reservation.lease.fence_token);
    assert_eq!(
        permit.scope_digest,
        connector_effect_scope_digest(&snapshot, OPERATION).expect("scope digest")
    );
    assert_eq!(permit.permit_digest, permit.digest());
    assert_eq!(current.fence_digest, current.digest());

    // Deterministic issuance and a serde round-trip keep the same digest.
    let reissued = issue_permit(&reservation, &snapshot, &current);
    assert_eq!(reissued, permit);
    let encoded = serde_json::to_value(&permit).expect("permit encodes");
    let decoded: ConnectorEffectPermit = serde_json::from_value(encoded).expect("permit decodes");
    assert_eq!(decoded, permit);
    assert_eq!(decoded.digest(), permit.digest());

    // Success: the freshly read fence admits the permit inside its own window only.
    permit
        .validate_for_effect(&reservation, &snapshot, &current, 200)
        .expect("window start");
    permit
        .validate_for_effect(&reservation, &snapshot, &current, 899)
        .expect("window edge");
    for now in [199, 900, 1_100] {
        assert_eq!(
            permit
                .validate_for_effect(&reservation, &snapshot, &current, now)
                .unwrap_err(),
            "connector_effect_permit_expired"
        );
    }

    // An effect permit may not outlive its reservation lease.
    let overlong = ConnectorEffectPermit::issue(&reservation, &snapshot, &current, 200, 1_001)
        .expect("issue does not check the lease window");
    assert_eq!(
        overlong
            .validate_for_effect(&reservation, &snapshot, &current, 300)
            .unwrap_err(),
        "connector_effect_permit_reservation_mismatch"
    );

    for (issued, expires) in [(0, 900), (900, 900), (900, 800)] {
        assert_eq!(
            ConnectorEffectPermit::issue(&reservation, &snapshot, &current, issued, expires)
                .unwrap_err(),
            "connector_effect_permit_expiry_invalid"
        );
    }
}

#[test]
fn stale_authority_configuration_policy_credential_and_data_epochs_never_reach_the_adapter() {
    let snapshot = binding_without_credential();
    let reservation = committed_reservation(&snapshot);
    let current = fence(&snapshot, 0);
    let permit = issue_permit(&reservation, &snapshot, &current);

    for (stale, reason) in [
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
            ConnectorEffectFence::new(permit.scope_digest.clone(), 4, 3, 5, 1, 9).unwrap(),
            "connector_effect_credential_epoch_stale",
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
    }

    // A rotated/revoked scope is refused even when the caller re-derives its own fence digest.
    let mut drifted = ConnectorEffectFence::new(digest('s'), 4, 3, 5, 0, 9).unwrap();
    assert_eq!(
        permit
            .validate_for_effect(&reservation, &snapshot, &drifted, 300)
            .unwrap_err(),
        "connector_effect_scope_epoch_mismatch"
    );

    // A fence whose recorded digest no longer matches its own fields is evidence, not authority.
    drifted = current.clone();
    drifted.data_epoch = 10;
    assert_eq!(
        permit
            .validate_for_effect(&reservation, &snapshot, &drifted, 300)
            .unwrap_err(),
        "connector_effect_fence_invalid"
    );

    // Zero revocable epochs cannot describe a current fence at all.
    for (authority, configuration, policy, credential, data) in [
        (0, 3, 5, 0, 9),
        (4, 0, 5, 0, 9),
        (4, 3, 0, 0, 9),
        (4, 3, 5, 0, 0),
    ] {
        assert_eq!(
            ConnectorEffectFence::new(
                digest('s'),
                authority,
                configuration,
                policy,
                credential,
                data
            )
            .unwrap_err(),
            "connector_effect_fence_invalid"
        );
    }
}

#[test]
fn revoked_rotated_or_reconfigured_bindings_and_old_snapshots_are_refused() {
    let snapshot = binding_without_credential();
    let reservation = committed_reservation(&snapshot);
    let current = fence(&snapshot, 0);
    let permit = issue_permit(&reservation, &snapshot, &current);

    // Revocation after admission.
    let mut revoked = snapshot.clone();
    revoked.status = "revoked".to_owned();
    assert_eq!(
        permit
            .validate_for_effect(&reservation, &revoked, &current, 300)
            .unwrap_err(),
        "connector_effect_binding_revoked"
    );

    // A binding reconfigured after admission is a new revision, in either direction.
    for revision in [BINDING_REVISION - 1, BINDING_REVISION + 1] {
        let mut reconfigured = snapshot.clone();
        reconfigured.revision = revision;
        assert_eq!(
            permit
                .validate_for_effect(&reservation, &reconfigured, &current, 300)
                .unwrap_err(),
            "connector_effect_binding_revision_stale"
        );
    }

    // A well-formed snapshot of another connector is not a substitute.
    let mut foreign = snapshot.clone();
    foreign.definition.connector_id = "connector-int18-other".to_owned();
    foreign.binding.connector_id = "connector-int18-other".to_owned();
    foreign.validate().expect("foreign snapshot is well-formed");
    assert_eq!(
        permit
            .validate_for_effect(&reservation, &foreign, &current, 300)
            .unwrap_err(),
        "connector_effect_binding_snapshot_stale"
    );

    // In-place scope widening at the same revision changes the scope digest.
    let mut widened = snapshot.clone();
    widened.binding.read_scopes.insert("read-extra".to_owned());
    widened.validate().expect("widened snapshot is well-formed");
    assert_eq!(
        permit
            .validate_for_effect(&reservation, &widened, &fence(&widened, 0), 300)
            .unwrap_err(),
        "connector_effect_scope_digest_mismatch"
    );

    // Credential fencing: rotation is stale, and a credential-backed binding can never be fenced
    // by a zero credential epoch.
    let credentialed = binding_with_credential(3);
    let credentialed_reservation = reservation_from_command(
        &credentialed,
        command(&credentialed, 1, 3),
        "lease-int18",
        "fence-int18",
    );
    let credentialed_fence = fence(&credentialed, 3);
    let credentialed_permit = issue_permit(
        &credentialed_reservation,
        &credentialed,
        &credentialed_fence,
    );
    credentialed_permit
        .validate_for_effect(
            &credentialed_reservation,
            &credentialed,
            &credentialed_fence,
            300,
        )
        .expect("current credentialed permit");
    assert_eq!(
        credentialed_permit
            .validate_for_effect(
                &credentialed_reservation,
                &credentialed,
                &fence(&credentialed, 4),
                300
            )
            .unwrap_err(),
        "connector_effect_credential_epoch_stale"
    );
    assert_eq!(
        ConnectorEffectPermit::issue(
            &credentialed_reservation,
            &credentialed,
            &fence(&credentialed, 0),
            200,
            900
        )
        .unwrap_err(),
        "connector_effect_credential_epoch_required"
    );
}

#[test]
fn permit_cannot_be_minted_from_uncommitted_or_mismatched_snapshot() {
    let snapshot = binding_without_credential();
    let mut reservation = committed_reservation(&snapshot);
    reservation.state = ConnectorReservationState::Reserved;
    let current = fence(&snapshot, 0);
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

    // A fence whose epochs or scope do not describe this command cannot mint a permit.
    let drifted = ConnectorEffectFence::new(permit_scope(&reservation, &snapshot), 5, 3, 5, 0, 9)
        .expect("drifted fence");
    assert_eq!(
        ConnectorEffectPermit::issue(&reservation, &snapshot, &drifted, 200, 900).unwrap_err(),
        "connector_effect_fence_command_mismatch"
    );
    let foreign_scope = ConnectorEffectFence::new(
        digest('s'),
        AUTHORITY_EPOCH,
        CONFIGURATION_EPOCH,
        POLICY_EPOCH,
        0,
        DATA_EPOCH,
    )
    .expect("foreign scope fence");
    assert_eq!(
        ConnectorEffectPermit::issue(&reservation, &snapshot, &foreign_scope, 200, 900)
            .unwrap_err(),
        "connector_effect_fence_command_mismatch"
    );
    assert_eq!(
        ConnectorEffectFence::new(
            permit_scope(&reservation, &snapshot),
            AUTHORITY_EPOCH,
            0,
            POLICY_EPOCH,
            0,
            DATA_EPOCH
        )
        .unwrap_err(),
        "connector_effect_fence_invalid"
    );
}

fn permit_scope(
    reservation: &ConnectorInvocationReservation,
    snapshot: &ConnectorBindingSnapshot,
) -> String {
    connector_effect_scope_digest(snapshot, &reservation.command.operation).expect("scope digest")
}

#[test]
fn tampered_or_replayed_effect_material_is_refused_before_any_effect() {
    let snapshot = binding_without_credential();
    let reservation = committed_reservation(&snapshot);
    let current = fence(&snapshot, 0);
    let permit = issue_permit(&reservation, &snapshot, &current);

    // A permit whose recorded digest no longer matches its content is refused.
    let mut tampered = permit.clone();
    tampered.payload_digest = digest('x');
    assert_eq!(
        tampered
            .validate_for_effect(&reservation, &snapshot, &current, 300)
            .unwrap_err(),
        "connector_effect_permit_invalid"
    );

    // Recomputing the digest does not forge authority: the envelope must still match the
    // reservation, the binding and the current fence.
    let mut refitted = permit.clone();
    refitted.payload_digest = digest('x');
    refitted.permit_digest = refitted.digest();
    assert_eq!(
        refitted
            .validate_for_effect(&reservation, &snapshot, &current, 300)
            .unwrap_err(),
        "connector_effect_permit_reservation_mismatch"
    );
    let mut rescoped = permit.clone();
    rescoped.scope_digest = digest('s');
    rescoped.permit_digest = rescoped.digest();
    assert_eq!(
        rescoped
            .validate_for_effect(&reservation, &snapshot, &current, 300)
            .unwrap_err(),
        "connector_effect_scope_digest_mismatch"
    );

    // A reservation whose command no longer matches its own digest is refused.
    let mut tampered_reservation = reservation.clone();
    tampered_reservation.command.payload_digest = digest('z');
    tampered_reservation.reservation_digest = tampered_reservation.digest();
    assert_eq!(
        permit
            .validate_for_effect(&tampered_reservation, &snapshot, &current, 300)
            .unwrap_err(),
        "connector_invocation_command_invalid"
    );

    // A consumed/released reservation or a substituted lease cannot reuse the permit.
    for state in [
        ConnectorReservationState::PermitConsumed,
        ConnectorReservationState::Completed,
        ConnectorReservationState::Released,
        ConnectorReservationState::Unknown,
    ] {
        let mut advanced = reservation.clone();
        advanced.state = state;
        assert_eq!(
            permit
                .validate_for_effect(&advanced, &snapshot, &current, 300)
                .unwrap_err(),
            "connector_effect_permit_reservation_mismatch"
        );
    }
    let substituted = reservation_from_command(
        &snapshot,
        reservation.command.clone(),
        "lease-int18-other",
        "fence-int18-other",
    );
    assert_eq!(
        permit
            .validate_for_effect(&substituted, &snapshot, &current, 300)
            .unwrap_err(),
        "connector_effect_permit_reservation_mismatch"
    );

    // A permit from another attempt of the same command cannot be substituted either.
    let attempt_two = reservation_from_command(
        &snapshot,
        command(&snapshot, 2, 0),
        "lease-int18",
        "fence-int18",
    );
    let attempt_two_permit = issue_permit(&attempt_two, &snapshot, &fence(&snapshot, 0));
    assert_eq!(
        attempt_two_permit
            .validate_for_effect(&reservation, &snapshot, &current, 300)
            .unwrap_err(),
        "connector_effect_permit_reservation_mismatch"
    );

    // Unknown fields are refused before any default applies.
    let mut extended = serde_json::to_value(&permit).expect("permit encodes");
    extended["adapter"] = json!("local_fixture");
    assert!(serde_json::from_value::<ConnectorEffectPermit>(extended).is_err());
    let mut unknown_fence = serde_json::to_value(&current).expect("fence encodes");
    unknown_fence["binding_id"] = json!(BINDING_ID);
    assert!(serde_json::from_value::<ConnectorEffectFence>(unknown_fence).is_err());
}

#[test]
fn fence_and_permit_digests_cover_scope_digests_and_every_revocable_epoch() {
    let snapshot = binding_without_credential();
    let scope = connector_effect_scope_digest(&snapshot, OPERATION).expect("scope digest");
    let current = ConnectorEffectFence::new(
        scope.clone(),
        AUTHORITY_EPOCH,
        CONFIGURATION_EPOCH,
        POLICY_EPOCH,
        0,
        DATA_EPOCH,
    )
    .expect("fence");
    assert_eq!(
        current.fence_digest,
        ConnectorEffectFence::new(
            scope.clone(),
            AUTHORITY_EPOCH,
            CONFIGURATION_EPOCH,
            POLICY_EPOCH,
            0,
            DATA_EPOCH
        )
        .expect("fence")
        .fence_digest
    );
    for drifted in [
        ConnectorEffectFence::new(
            digest('s'),
            AUTHORITY_EPOCH,
            CONFIGURATION_EPOCH,
            POLICY_EPOCH,
            0,
            DATA_EPOCH,
        )
        .expect("fence"),
        ConnectorEffectFence::new(
            scope.clone(),
            AUTHORITY_EPOCH + 1,
            CONFIGURATION_EPOCH,
            POLICY_EPOCH,
            0,
            DATA_EPOCH,
        )
        .expect("fence"),
        ConnectorEffectFence::new(
            scope.clone(),
            AUTHORITY_EPOCH,
            CONFIGURATION_EPOCH + 1,
            POLICY_EPOCH,
            0,
            DATA_EPOCH,
        )
        .expect("fence"),
        ConnectorEffectFence::new(
            scope.clone(),
            AUTHORITY_EPOCH,
            CONFIGURATION_EPOCH,
            POLICY_EPOCH + 1,
            0,
            DATA_EPOCH,
        )
        .expect("fence"),
        ConnectorEffectFence::new(
            scope.clone(),
            AUTHORITY_EPOCH,
            CONFIGURATION_EPOCH,
            POLICY_EPOCH,
            1,
            DATA_EPOCH,
        )
        .expect("fence"),
        ConnectorEffectFence::new(
            scope.clone(),
            AUTHORITY_EPOCH,
            CONFIGURATION_EPOCH,
            POLICY_EPOCH,
            0,
            DATA_EPOCH + 1,
        )
        .expect("fence"),
    ] {
        assert_ne!(drifted.fence_digest, current.fence_digest);
    }

    let reservation = committed_reservation(&snapshot);
    let permit = issue_permit(&reservation, &snapshot, &current);
    for variant in permit_variants(&permit) {
        assert_ne!(
            variant.digest(),
            permit.digest(),
            "permit digest must cover every field: {variant:?}"
        );
    }
}

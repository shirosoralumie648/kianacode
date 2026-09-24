//! ControlPlane connector invocation reservation boundary.
//!
//! The domain ledger is a pure CAS model.  This module is the only Core-owned place that turns a
//! server-owned connector command into an EventStore transition; Broker and daemon code consume
//! the committed reservation through the existing permit path and never mint a reservation.

use super::*;
use kiana_domain::{
    connector_effect_admission, ConnectorInvocationCommand, ConnectorInvocationPermit,
    ConnectorInvocationReservation, ConnectorReservationLease, ConnectorReservationOutcome,
    ConnectorReservationState, CONNECTOR_RESERVATION_EVENT_RESERVED, CONNECTOR_RESERVATION_STREAM,
};

pub struct ControlPlaneConnectorReservation;

impl ControlPlaneConnectorReservation {
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        context: &RequestContext,
        binding: &kiana_domain::ConnectorBindingSnapshot,
        operation: &str,
        payload: &Value,
        idempotency_key: &str,
        authority_revision: u64,
        policy_revision: &str,
        configuration_revision: &str,
        credential_generation: u64,
        data_revision: u64,
        lease: ConnectorReservationLease,
    ) -> Result<ConnectorInvocationReservation, String> {
        let owner_revision = kiana_domain::json_digest(&json!({
            "project_root": context.project_root,
            "actor_id": context.actor_id,
            "role_id": context.role_id,
            "department_id": context.department_id,
        }));
        let command = ConnectorInvocationCommand::from_payload(
            kiana_domain::InvocationId::from_uuid(context.request_id.as_uuid()),
            1,
            binding,
            operation,
            owner_revision,
            authority_revision,
            policy_revision,
            configuration_revision,
            credential_generation,
            data_revision,
            payload,
            idempotency_key,
        )?;
        ConnectorInvocationReservation::new(
            format!("connector-reservation:{}", context.request_id),
            command,
            lease,
        )
    }

    /// The adapter boundary is deny-first: a reservation must be committed and its exact permit
    /// must still match the lease/fence/expiry before any Broker handler can be entered.
    pub fn admit_effect(
        reservation: &ConnectorInvocationReservation,
        permit: &ConnectorInvocationPermit,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        connector_effect_admission(reservation, permit, now_unix_ms)
    }
}

impl ControlPlane {
    /// Persist the reservation before Broker dispatch using the existing EventStore CAS spine.
    /// A replay returns the original reservation and never creates a second adapter attempt.
    pub(crate) async fn commit_connector_reservation(
        &self,
        command_id: RequestId,
        reservation: &ConnectorInvocationReservation,
        expected_version: u64,
    ) -> Result<bool, PortError> {
        reservation.validate(None).map_err(PortError::Conflict)?;
        let event = RuntimeEvent::new(
            command_id,
            1,
            CONNECTOR_RESERVATION_EVENT_RESERVED,
            json!({
                "schema": "kiana.connector-reservation-event.v1",
                "reservation": reservation,
                "command_digest": reservation.command.command_digest,
                "idempotency_key_digest": reservation.command.idempotency_key_digest,
                "state": ConnectorReservationState::Reserved,
            }),
        )
        .map_err(|error| PortError::Failed(error.to_string()))?
        .with_stream_metadata(
            CONNECTOR_RESERVATION_STREAM,
            reservation.reservation_id.clone(),
            expected_version.saturating_add(1),
        )
        .with_idempotency_key(format!(
            "connector-reservation:{}",
            reservation.command.idempotency_key_digest
        ));
        let batch = kiana_domain::TransitionBatch {
            command_id,
            command_digest: reservation.command.command_digest.clone(),
            expected_versions: vec![kiana_domain::AggregateVersion::new(
                CONNECTOR_RESERVATION_STREAM,
                reservation.reservation_id.clone(),
                expected_version,
            )],
            events: vec![event],
        };
        super::dispatch::commit_confirmed(self.events.as_ref(), batch).await
    }
}

pub fn connector_reservation_replayed(outcome: &ConnectorReservationOutcome) -> bool {
    matches!(outcome, ConnectorReservationOutcome::Replayed(_))
}

//! ControlPlane boundary for INT-17 connector quota facts.
//!
//! The module only compiles server-owned reservation/claim/settlement facts into the existing
//! EventStore CAS path. It does not run a worker, call a provider, or mint a capability permit.

use super::*;
use kiana_domain::{
    connector_quota_effect_admission, ConnectorInvocationCommand, ConnectorQuotaClaim,
    ConnectorQuotaDimensions, ConnectorQuotaPolicy, ConnectorQuotaReservation,
    ConnectorQuotaReservationState, ConnectorQuotaSettlement, CONNECTOR_QUOTA_EVENT_CLAIMED,
    CONNECTOR_QUOTA_EVENT_RESERVED, CONNECTOR_QUOTA_EVENT_SETTLED, CONNECTOR_QUOTA_STREAM,
};

pub struct ControlPlaneConnectorQuota;

fn journal_digest(value: &str) -> String {
    value.strip_prefix("sha256:").unwrap_or(value).to_owned()
}

impl ControlPlaneConnectorQuota {
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        context: &RequestContext,
        command: &ConnectorInvocationCommand,
        policy: &ConnectorQuotaPolicy,
        owner_run: RunId,
        owner_attempt: AttemptId,
        requested: ConnectorQuotaDimensions,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<ConnectorQuotaReservation, String> {
        context
            .project_trusted
            .then_some(())
            .ok_or_else(|| "connector_quota_project_untrusted".to_owned())?;
        command.validate()?;
        policy.validate()?;
        if command.command_digest != kiana_domain::connector_quota_command_digest(command) {
            return Err("connector_quota_command_digest_mismatch".to_owned());
        }
        let owner_digest = kiana_domain::json_digest(&json!({
            "project_root": context.project_root,
            "actor_id": context.actor_id,
            "role_id": context.role_id,
            "department_id": context.department_id,
            "project_trusted": context.project_trusted,
        }));
        let reservation = ConnectorQuotaReservation::new(
            kiana_domain::QuotaReservationId::from_uuid(context.request_id.as_uuid()),
            command.invocation_id,
            command.attempt,
            owner_run,
            owner_attempt,
            owner_digest,
            command.command_digest.clone(),
            policy,
            requested,
            issued_at_unix_ms,
            expires_at_unix_ms,
        )?;
        reservation.validate_against(policy, Some(issued_at_unix_ms))?;
        Ok(reservation)
    }

    pub fn admit_effect(
        reservation: &ConnectorQuotaReservation,
        claim: &ConnectorQuotaClaim,
        policy: &ConnectorQuotaPolicy,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        connector_quota_effect_admission(reservation, claim, policy, now_unix_ms)
    }
}

impl ControlPlane {
    /// Persist the server-owned reservation before Broker dispatch. Replaying the same
    /// idempotency key is handled by EventStore CAS and never creates another quota claim.
    pub(crate) async fn commit_connector_quota_reservation(
        &self,
        command_id: RequestId,
        reservation: &ConnectorQuotaReservation,
        expected_version: u64,
    ) -> Result<bool, PortError> {
        reservation.validate().map_err(PortError::Conflict)?;
        let event = RuntimeEvent::new(
            command_id,
            1,
            CONNECTOR_QUOTA_EVENT_RESERVED,
            json!({
                "schema": "kiana.connector-quota-event.v1",
                "reservation": reservation,
                "reservation_id": reservation.reservation_id,
                "command_digest": reservation.command_digest,
                "policy_digest": reservation.policy_digest,
                "state": ConnectorQuotaReservationState::Reserved,
            }),
        )
        .map_err(|error| PortError::Failed(error.to_string()))?
        .with_stream_metadata(
            CONNECTOR_QUOTA_STREAM,
            reservation.reservation_id.to_string(),
            expected_version.saturating_add(1),
        )
        .with_idempotency_key(format!(
            "connector-quota:reserve:{}",
            reservation.command_digest
        ));
        let batch = kiana_domain::TransitionBatch {
            command_id,
            command_digest: journal_digest(&reservation.reservation_digest),
            expected_versions: vec![kiana_domain::AggregateVersion::new(
                CONNECTOR_QUOTA_STREAM,
                reservation.reservation_id.to_string(),
                expected_version,
            )],
            events: vec![event],
        };
        super::dispatch::commit_confirmed(self.events.as_ref(), batch).await
    }

    pub(crate) async fn commit_connector_quota_claim(
        &self,
        command_id: RequestId,
        claim: &ConnectorQuotaClaim,
        reservation: &ConnectorQuotaReservation,
        expected_version: u64,
    ) -> Result<bool, PortError> {
        claim.validate().map_err(PortError::Conflict)?;
        if claim.reservation_id != reservation.reservation_id
            || claim.reservation_digest != reservation.reservation_digest
            || reservation.state != ConnectorQuotaReservationState::Claimed
        {
            return Err(PortError::Conflict(
                "connector_quota_claim_fence_mismatch".to_owned(),
            ));
        }
        self.commit_connector_quota_fact(
            command_id,
            CONNECTOR_QUOTA_EVENT_CLAIMED,
            reservation.reservation_id.to_string(),
            claim.claim_digest.clone(),
            expected_version,
            json!({
                "schema": "kiana.connector-quota-event.v1",
                "claim": claim,
                "reservation": reservation,
                "reservation_id": reservation.reservation_id,
                "reservation_digest": reservation.reservation_digest,
                "state": ConnectorQuotaReservationState::Claimed,
            }),
        )
        .await
    }

    pub(crate) async fn commit_connector_quota_settlement(
        &self,
        command_id: RequestId,
        settlement: &ConnectorQuotaSettlement,
        reservation: &ConnectorQuotaReservation,
        expected_version: u64,
    ) -> Result<bool, PortError> {
        settlement
            .validate_against(reservation)
            .map_err(PortError::Conflict)?;
        if reservation.state != ConnectorQuotaReservationState::Claimed {
            return Err(PortError::Conflict(
                "connector_quota_settlement_not_claimed".to_owned(),
            ));
        }
        self.commit_connector_quota_fact(
            command_id,
            CONNECTOR_QUOTA_EVENT_SETTLED,
            reservation.reservation_id.to_string(),
            settlement.settlement_digest.clone(),
            expected_version,
            json!({
                "schema": "kiana.connector-quota-event.v1",
                "settlement": settlement,
                "reservation": reservation,
                "reservation_id": reservation.reservation_id,
                "reservation_digest": reservation.reservation_digest,
                "state": if settlement.usage_known {
                    ConnectorQuotaReservationState::Settled
                } else {
                    ConnectorQuotaReservationState::Unknown
                },
            }),
        )
        .await
    }

    async fn commit_connector_quota_fact(
        &self,
        command_id: RequestId,
        kind: &str,
        aggregate_id: String,
        fact_digest: String,
        expected_version: u64,
        data: Value,
    ) -> Result<bool, PortError> {
        let event = RuntimeEvent::new(command_id, 1, kind, data)
            .map_err(|error| PortError::Failed(error.to_string()))?
            .with_stream_metadata(
                CONNECTOR_QUOTA_STREAM,
                aggregate_id.clone(),
                expected_version.saturating_add(1),
            )
            .with_idempotency_key(format!("connector-quota:{kind}:{fact_digest}"));
        let batch = kiana_domain::TransitionBatch {
            command_id,
            command_digest: journal_digest(&fact_digest),
            expected_versions: vec![kiana_domain::AggregateVersion::new(
                CONNECTOR_QUOTA_STREAM,
                aggregate_id,
                expected_version,
            )],
            events: vec![event],
        };
        super::dispatch::commit_confirmed(self.events.as_ref(), batch).await
    }
}

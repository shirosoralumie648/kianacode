use super::*;
use kiana_domain::{
    CommunicationLifecycleEvent, CommunicationLifecycleStatus, CommunicationMessage,
    CommunicationMessageKind,
};

impl ControlPlane {
    pub(crate) async fn handle_communication_command(
        &self,
        context: RequestContext,
        operation: &str,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        if context.actor_id.as_deref().is_none_or(str::is_empty) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "communication_sender_required",
            ));
        }
        if !context.project_trusted {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "project_untrusted",
            ));
        }
        match operation {
            "communication.send" => self.send_communication(context, arguments).await,
            "communication.ack" | "communication.reject" => {
                self.acknowledge_communication(context, operation, arguments)
                    .await
            }
            "communication.escalate" => self.escalate_communication(context, arguments).await,
            _ => Ok(CoreResponse::blocked(
                context.request_id,
                "communication_operation_invalid",
            )),
        }
    }

    async fn send_communication(
        &self,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let Some(raw) = arguments.get("message") else {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "communication_message_required",
            ));
        };
        let message: CommunicationMessage = match serde_json::from_value(raw.clone()) {
            Ok(message) => message,
            Err(_) => {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "communication_message_invalid",
                ));
            }
        };
        if message.validate().is_err() {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "communication_message_invalid",
            ));
        }
        if RoleSpec::lookup(&context.role_id).is_none() {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "communication_sender_role_invalid",
            ));
        }
        if message.sender_id != context.actor_id.clone().unwrap_or_default() {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "communication_sender_mismatch",
            ));
        }
        let kind = communication_event_kind(message.kind);
        let lifecycle = CommunicationLifecycleEvent::new(
            &message,
            None,
            CommunicationLifecycleStatus::Sent,
            context.actor_id.clone().unwrap_or_default(),
            "",
            Vec::new(),
            1,
        )
        .map_err(|_| PortError::Failed("communication_lifecycle_invalid".to_owned()))?;
        let mut sequence = 1;
        self.record_event(
            context.request_id,
            &mut sequence,
            kind,
            json!({
                "message": message.clone(),
                "message_id": message.message_id,
                "lifecycle": lifecycle,
                "authority_granted": false,
                "project_root": context.project_root,
                "actor_id": context.actor_id,
                "session_id": context.session_id,
                "request_id": context.request_id,
            }),
        )
        .await?;
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"schema": kiana_domain::COMMUNICATION_MESSAGE_SCHEMA, "message": message, "authority_granted": false}),
        ))
    }

    async fn acknowledge_communication(
        &self,
        context: RequestContext,
        operation: &str,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let message_id = arguments
            .get("message_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| PortError::Failed("communication_message_id_required".to_owned()))?;
        let reason = arguments
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let (message, status, mut sequence) = self.load_communication(message_id).await?;
        if message.kind != CommunicationMessageKind::Handoff
            || message.recipient_id.as_deref() != context.actor_id.as_deref()
            || status != Some(CommunicationLifecycleStatus::Sent)
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "communication_handoff_not_pending",
            ));
        }
        let to_status = if operation == "communication.ack" {
            CommunicationLifecycleStatus::Acknowledged
        } else {
            CommunicationLifecycleStatus::Rejected
        };
        let lifecycle = CommunicationLifecycleEvent::new(
            &message,
            Some(CommunicationLifecycleStatus::Sent),
            to_status,
            context.actor_id.clone().unwrap_or_default(),
            reason,
            Vec::new(),
            sequence,
        )
        .map_err(|_| PortError::Failed("communication_handoff_ack_invalid".to_owned()))?;
        let kind = if to_status == CommunicationLifecycleStatus::Acknowledged {
            "communication.handoff_acknowledged"
        } else {
            "communication.handoff_rejected"
        };
        self.record_event(
            context.request_id,
            &mut sequence,
            kind,
            json!({
                "message": message,
                "message_id": message_id,
                "lifecycle": lifecycle.clone(),
                "accepted": to_status == CommunicationLifecycleStatus::Acknowledged,
                "reason": reason,
                "authority_granted": false,
                "project_root": context.project_root,
                "actor_id": context.actor_id,
                "session_id": context.session_id,
                "request_id": context.request_id,
            }),
        )
        .await?;
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"schema": kiana_domain::COMMUNICATION_LIFECYCLE_SCHEMA, "lifecycle": lifecycle, "authority_granted": false}),
        ))
    }

    async fn escalate_communication(
        &self,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let message_id = arguments
            .get("message_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| PortError::Failed("communication_message_id_required".to_owned()))?;
        let reason = arguments
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let evidence_refs = arguments
            .get("evidence_refs")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let (message, status, mut sequence) = self.load_communication(message_id).await?;
        if message.kind != CommunicationMessageKind::Incident
            || message.sender_id != context.actor_id.clone().unwrap_or_default()
            || status != Some(CommunicationLifecycleStatus::Sent)
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "communication_incident_escalation_denied",
            ));
        }
        let lifecycle = CommunicationLifecycleEvent::new(
            &message,
            Some(CommunicationLifecycleStatus::Sent),
            CommunicationLifecycleStatus::Escalated,
            context.actor_id.clone().unwrap_or_default(),
            reason,
            evidence_refs,
            sequence,
        )
        .map_err(|_| PortError::Failed("communication_incident_escalation_invalid".to_owned()))?;
        self.record_event(
            context.request_id,
            &mut sequence,
            "communication.incident_escalated",
            json!({
                "message": message,
                "message_id": message_id,
                "lifecycle": lifecycle.clone(),
                "evidence_refs": lifecycle.evidence_refs.clone(),
                "reason": reason,
                "authority_granted": false,
                "project_root": context.project_root,
                "actor_id": context.actor_id,
                "session_id": context.session_id,
                "request_id": context.request_id,
            }),
        )
        .await?;
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"schema": kiana_domain::COMMUNICATION_LIFECYCLE_SCHEMA, "lifecycle": lifecycle, "authority_granted": false}),
        ))
    }

    async fn load_communication(
        &self,
        message_id: &str,
    ) -> Result<
        (
            CommunicationMessage,
            Option<CommunicationLifecycleStatus>,
            u64,
        ),
        CoreError,
    > {
        let events = self.events.read_stream("communication", message_id).await?;
        let mut message = None;
        let mut status = None;
        let mut sequence = 1;
        for event in events {
            sequence = sequence.max(event.sequence.saturating_add(1));
            if let Some(raw) = event.data.get("message") {
                let candidate: CommunicationMessage = serde_json::from_value(raw.clone())
                    .map_err(|_| PortError::Failed("communication_message_invalid".to_owned()))?;
                if candidate.message_id != message_id {
                    return Err(
                        PortError::Failed("communication_message_id_mismatch".to_owned()).into(),
                    );
                }
                candidate
                    .validate()
                    .map_err(|_| PortError::Failed("communication_message_invalid".to_owned()))?;
                message = Some(candidate);
            }
            if let (Some(raw), Some(current)) = (event.data.get("lifecycle"), message.as_ref()) {
                let lifecycle: CommunicationLifecycleEvent = serde_json::from_value(raw.clone())
                    .map_err(|_| PortError::Failed("communication_lifecycle_invalid".to_owned()))?;
                lifecycle
                    .validate(current)
                    .map_err(|_| PortError::Failed("communication_lifecycle_invalid".to_owned()))?;
                status = Some(lifecycle.to_status);
            } else if status.is_none() && event.kind.starts_with("communication.") {
                status = Some(CommunicationLifecycleStatus::Sent);
            }
        }
        let message = message
            .ok_or_else(|| PortError::Failed("communication_message_not_found".to_owned()))?;
        Ok((message, status, sequence))
    }
}

fn communication_event_kind(kind: CommunicationMessageKind) -> &'static str {
    match kind {
        CommunicationMessageKind::Chat => "communication.chat",
        CommunicationMessageKind::Command => "communication.command",
        CommunicationMessageKind::Handoff => "communication.handoff",
        CommunicationMessageKind::Decision => "communication.decision",
        CommunicationMessageKind::StatusReport => "communication.status_report",
        CommunicationMessageKind::Evidence => "communication.evidence",
        CommunicationMessageKind::Incident => "communication.incident",
    }
}

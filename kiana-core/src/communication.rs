use super::*;
use kiana_domain::{CommunicationMessage, CommunicationMessageKind};

impl ControlPlane {
    pub(crate) async fn handle_communication_command(
        &self,
        context: RequestContext,
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
        if message.sender_id != context.actor_id.clone().unwrap_or_default() {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "communication_sender_mismatch",
            ));
        }
        let kind = match message.kind {
            CommunicationMessageKind::Chat => "communication.chat",
            CommunicationMessageKind::Command => "communication.command",
            CommunicationMessageKind::Handoff => "communication.handoff",
            CommunicationMessageKind::Decision => "communication.decision",
            CommunicationMessageKind::StatusReport => "communication.status_report",
            CommunicationMessageKind::Evidence => "communication.evidence",
            CommunicationMessageKind::Incident => "communication.incident",
        };
        let mut sequence = 1;
        self.record_event(
            context.request_id,
            &mut sequence,
            kind,
            json!({
                "message": message.clone(),
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
}

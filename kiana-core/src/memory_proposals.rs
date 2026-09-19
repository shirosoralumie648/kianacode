//! Terminal/meeting distillation queue. The EventLog is the candidate store;
//! active Memory writes still require the operator's approved memory.review capability.
use super::*;

impl ControlPlane {
    pub(crate) async fn prepare_memory_review(
        &self,
        context: &RequestContext,
        arguments: &mut Value,
    ) -> Result<(), CoreError> {
        if arguments["action"] != "accept_proposal" {
            return Ok(());
        }
        let id = arguments["proposal_id"]
            .as_str()
            .ok_or_else(|| PortError::Failed("memory_proposal_id_required".to_owned()))?;
        let events = self.events.read_all().await?;
        let proposal_event = events
            .iter()
            .find(|event| {
                event.kind == "memory.proposed"
                    && event.data["proposal"]["id"] == id
                    && event.data["project_root"].as_str().is_some_and(|root| {
                        Self::canonical_project_root(root)
                            == Self::canonical_project_root(&context.project_root)
                    })
            })
            .ok_or_else(|| PortError::Failed("memory_proposal_not_found".to_owned()))?;
        let proposal = proposal_event.data["proposal"].clone();
        if proposal["origin"] == json!(MemoryOrigin::Model) {
            let source =
                proposal_event.data.get("source").cloned().ok_or_else(|| {
                    PortError::Failed("memory_proposal_source_required".to_owned())
                })?;
            let source: MemoryDistillationSource = serde_json::from_value(source)
                .map_err(|_| PortError::Failed("memory_proposal_source_invalid".to_owned()))?;
            let proposal_value = serde_json::from_value::<MemoryProposal>(proposal.clone())
                .map_err(|_| PortError::Failed("memory_proposal_invalid".to_owned()))?;
            source
                .validate_proposal_source(&proposal_value)
                .map_err(PortError::Failed)?;
        }
        if let Some(facts) = proposal["facts"].as_array() {
            for run_id in facts
                .iter()
                .flat_map(|fact| fact["evidence"].as_array().into_iter().flatten())
                .filter_map(|evidence| evidence["run_id"].as_str().and_then(RunId::parse_str))
            {
                if self.run_data_revoked(run_id).await? {
                    return Err(PortError::Failed("memory_proposal_data_revoked".to_owned()).into());
                }
            }
        }
        arguments["proposal"] = proposal;
        arguments["proposal_authorized"] = json!(true);
        Ok(())
    }

    pub(crate) async fn pending_memory_proposals(
        &self,
        context: &RequestContext,
        collection: Option<&str>,
    ) -> Result<Vec<Value>, CoreError> {
        let role = RoleSpec::lookup(&context.role_id)
            .ok_or_else(|| PortError::Failed("role_unknown".to_owned()))?;
        let events = self.events.read_all().await?;
        let settled = events
            .iter()
            .filter(|event| event.kind == "capability.completed")
            .flat_map(|event| {
                event
                    .data
                    .get("records")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
            .filter_map(|record| record["id"].as_str())
            .collect::<Vec<_>>();
        Ok(events
            .iter()
            .filter(|event| {
                event.kind == "memory.proposed"
                    && event.data["project_root"].as_str().is_some_and(|root| {
                        Self::canonical_project_root(root)
                            == Self::canonical_project_root(&context.project_root)
                    })
            })
            .map(|event| event.data["proposal"].clone())
            .filter(|proposal| {
                let id = proposal["id"].as_str().unwrap_or_default();
                !settled
                    .iter()
                    .any(|record| record.starts_with(&format!("{id}:")))
                    && proposal["facts"].as_array().is_some_and(|facts| {
                        facts.iter().all(|fact| {
                            fact["collection"].as_str().is_some_and(|name| {
                                collection.is_none_or(|requested| requested == name)
                                    && role.allows_knowledge(name)
                            })
                        })
                    })
            })
            .collect())
    }
    pub(crate) async fn derive_memory_proposal(
        &self,
        context: &RequestContext,
        run_id: Option<RunId>,
        source_kind: &str,
        kind: &str,
        sequence: &mut u64,
    ) {
        let result = self
            .try_derive_memory_proposal(context, run_id, source_kind, kind, sequence)
            .await;
        if let Err(error) = result {
            // Extraction must not rewrite a successful or failed run outcome.
            let _=self.record_event(context.request_id,sequence,"memory.extraction_incident",json!({
                "source_run_id":run_id,"source_kind":source_kind,"reason":super::redaction::redact_event_text(&error.to_string()),
            })).await;
        }
    }
    async fn try_derive_memory_proposal(
        &self,
        context: &RequestContext,
        run_id: Option<RunId>,
        source_kind: &str,
        kind: &str,
        sequence: &mut u64,
    ) -> Result<(), CoreError> {
        let events = self.events.read_request(&context.request_id).await?;
        let Some(source) = events.iter().rev().find(|event| event.kind == source_kind) else {
            return Ok(());
        };
        let _ = sequence;
        self.queue_memory_distillation(context, run_id, source, kind, &events)
            .await
    }
}

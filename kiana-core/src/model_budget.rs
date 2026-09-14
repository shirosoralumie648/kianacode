//! Journal-backed admission runs before the existing Harness calls its provider.
use super::*;
use kiana_domain::{
    derived_request_id, json_digest, AggregateVersion, RuntimeBudget, TransitionBatch,
};

pub struct JournalModelBudget {
    events: Arc<dyn EventStorePort>,
}
impl JournalModelBudget {
    pub fn new(events: Arc<dyn EventStorePort>) -> Self {
        Self { events }
    }
}
fn failed(reason: &str) -> PortError {
    PortError::Failed(reason.to_owned())
}
fn time_ms() -> Result<u64, PortError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .ok_or_else(|| failed("clock_untrusted"))
}

#[async_trait::async_trait]
impl kiana_ports::ModelBudgetPort for JournalModelBudget {
    async fn reserve(
        &self,
        run_id: RunId,
        request_id: RequestId,
        tokens: u64,
    ) -> Result<(), PortError> {
        self.reserve_call(run_id, request_id, tokens, None).await
    }
    async fn reserve_prepared(
        &self,
        prepared: &kiana_domain::PreparedModelCall,
    ) -> Result<kiana_domain::ModelCallPermit, PortError> {
        prepared.validate().map_err(|e| failed(&e.to_string()))?;
        let run_id = prepared
            .spec
            .assignment
            .as_ref()
            .ok_or_else(|| failed("model_assignment_required"))?
            .run_id;
        let now = time_ms()?;
        if prepared.spec.deadline_unix_ms <= now {
            return Err(failed("model_deadline_expired"));
        }
        let permit = kiana_domain::ModelCallPermit {
            schema: "kiana.model-call-permit.v1".to_owned(),
            permit_id: derived_request_id("model.permit", &prepared.spec.attempt_id.to_string()),
            run_id,
            attempt_id: prepared.spec.attempt_id,
            request_hash: prepared.request_hash.clone(),
            expires_at_unix_ms: prepared
                .spec
                .deadline_unix_ms
                .min(now.saturating_add(60_000)),
        };
        self.reserve_call(
            run_id,
            prepared.spec.attempt_id,
            prepared.budget.total,
            Some((prepared, &permit)),
        )
        .await?;
        Ok(permit)
    }
    async fn consume_prepared(
        &self,
        prepared: &kiana_domain::PreparedModelCall,
        permit: &kiana_domain::ModelCallPermit,
    ) -> Result<(), PortError> {
        prepared.validate().map_err(|e| failed(&e.to_string()))?;
        if permit.schema != "kiana.model-call-permit.v1"
            || permit.request_hash != prepared.request_hash
            || permit.attempt_id != prepared.spec.attempt_id
            || prepared
                .spec
                .assignment
                .as_ref()
                .is_none_or(|assignment| assignment.run_id != permit.run_id)
            || time_ms()? >= permit.expires_at_unix_ms
        {
            return Err(failed("model_permit_scope_or_expiry_mismatch"));
        }
        let records = self
            .events
            .read_stream("model_attempt", &permit.attempt_id.to_string())
            .await?;
        if records.len() != 1
            || records[0].kind != "model.prepared"
            || records[0].data["permit"] != json!(permit)
        {
            return Err(failed("model_permit_unavailable"));
        }
        let mut expected: Vec<AggregateVersion> =
            serde_json::from_value(records[0].data["authority_versions"].clone())
                .map_err(|_| failed("model_permit_authority_invalid"))?;
        expected.push(AggregateVersion::new(
            "model_attempt",
            permit.attempt_id.to_string(),
            1,
        ));
        let command_id = derived_request_id("model.dispatch", &permit.attempt_id.to_string());
        let event=RuntimeEvent::new(command_id,1,"model.dispatching",json!({"run_id":permit.run_id,"model_request_id":permit.attempt_id,"request_hash":permit.request_hash,"permit_id":permit.permit_id}))
            .map_err(|e|failed(&e.to_string()))?.with_stream_metadata("model_attempt",permit.attempt_id.to_string(),2);
        let batch = TransitionBatch {
            command_id,
            command_digest: json_digest(&json!({"permit":permit})),
            expected_versions: expected,
            events: vec![event],
        };
        if super::dispatch::commit_confirmed(self.events.as_ref(), batch).await? {
            return Err(failed("model_permit_already_consumed"));
        }
        Ok(())
    }

    async fn settle(
        &self,
        run_id: RunId,
        request_id: RequestId,
        tokens: Option<u64>,
    ) -> Result<(), PortError> {
        let records = self
            .events
            .read_stream("model_budget", &run_id.to_string())
            .await?;
        let reservation = records
            .iter()
            .find(|event| {
                event.kind == "model.reserved"
                    && event.data["model_request_id"] == json!(request_id)
            })
            .ok_or_else(|| failed("model_budget_reservation_missing"))?;
        let upper = reservation.data["tokens"]
            .as_u64()
            .ok_or_else(|| failed("model_budget_record_invalid"))?;
        let charged = tokens.unwrap_or(upper);
        let command_id = derived_request_id("model.settle", &request_id.to_string());
        let version = records
            .iter()
            .filter_map(|e| e.stream_version)
            .max()
            .unwrap_or(0);
        let payload = json!({"run_id":run_id,"model_request_id":request_id,"charged_tokens":charged,
            "reported_tokens":tokens,"usage_known":tokens.is_some(),"reservation_exceeded":charged>upper,"at_unix_ms":time_ms()?});
        let event = RuntimeEvent::new(command_id, 1, "model.settled", payload)
            .map_err(|e| failed(&e.to_string()))?
            .with_stream_metadata("model_budget", run_id.to_string(), version + 1);
        let batch = TransitionBatch {
            command_id,
            command_digest: json_digest(
                &json!({"run_id":run_id,"request_id":request_id,"tokens":tokens}),
            ),
            expected_versions: vec![AggregateVersion {
                aggregate_type: "model_budget".to_owned(),
                aggregate_id: run_id.to_string(),
                version,
            }],
            events: vec![event],
        };
        super::dispatch::commit_confirmed(self.events.as_ref(), batch).await?;
        if charged > upper {
            return Err(failed("model_reported_usage_exceeds_reservation"));
        }
        Ok(())
    }
}

impl JournalModelBudget {
    async fn reserve_call(
        &self,
        run_id: RunId,
        request_id: RequestId,
        tokens: u64,
        prepared: Option<(
            &kiana_domain::PreparedModelCall,
            &kiana_domain::ModelCallPermit,
        )>,
    ) -> Result<(), PortError> {
        let run = self.events.read_stream("run", &run_id.to_string()).await?;
        let authority = run
            .iter()
            .find(|event| event.kind == "run.authorized")
            .ok_or_else(|| failed("model_budget_authority_missing"))?;
        let root = authority.data["project_root"]
            .as_str()
            .ok_or_else(|| failed("model_budget_project_missing"))?;
        let authority_key =
            json_digest(&json!({"project_root":ControlPlane::canonical_project_root(root)}));
        let current_authority = self.events.read_stream("authority", &authority_key).await?;
        let current = current_authority
            .last()
            .ok_or_else(|| failed("model_budget_authority_missing"))?;
        let authority_version = current.stream_version.unwrap_or(0);
        let authority_revision = format!(
            "{}:{}",
            authority_version,
            current.data["revision_digest"].as_str().unwrap_or_default()
        );
        if authority.data["authority_revision"].as_str() != Some(authority_revision.as_str()) {
            return Err(failed("model_budget_authority_changed"));
        }
        if let Some((call, _)) = prepared {
            let assignment = call
                .spec
                .assignment
                .as_ref()
                .ok_or_else(|| failed("model_assignment_required"))?;
            if assignment.run_id != run_id
                || authority.data["role_id"] != assignment.role_id
                || authority.data["model_profile"] != assignment.profile
                || authority.data["authority_revision"]
                    != serde_json::json!(assignment.authority_revision)
            {
                return Err(failed("model_assignment_authority_mismatch"));
            }
        }
        let limit: RuntimeBudget = serde_json::from_value(authority.data["runtime_budget"].clone())
            .map_err(|_| failed("model_budget_authority_missing"))?;
        limit.validate().map_err(failed)?;
        let turn = run
            .iter()
            .rposition(|e| e.kind == "run.prompt")
            .unwrap_or(0);
        if run[turn..].iter().any(|event| {
            matches!(
                event.kind.as_str(),
                "run.cancelling"
                    | "run.cancelled"
                    | "run.result_unknown"
                    | "run.completed"
                    | "run.failed"
            )
        }) {
            return Err(failed("model_budget_run_not_active"));
        }
        let records = self
            .events
            .read_stream("model_budget", &run_id.to_string())
            .await?;
        let mut reserved = HashMap::new();
        let mut calls = 0u64;
        for event in &records {
            let key = event.data["model_request_id"]
                .as_str()
                .ok_or_else(|| failed("model_budget_record_invalid"))?;
            match event.kind.as_str() {
                "model.reserved" => {
                    calls = calls.saturating_add(1);
                    if reserved
                        .insert(
                            key.to_owned(),
                            event.data["tokens"]
                                .as_u64()
                                .ok_or_else(|| failed("model_budget_record_invalid"))?,
                        )
                        .is_some()
                    {
                        return Err(failed("model_budget_duplicate_reservation"));
                    }
                }
                "model.settled" => {
                    let charge = event.data["charged_tokens"]
                        .as_u64()
                        .ok_or_else(|| failed("model_budget_record_invalid"))?;
                    let old = reserved
                        .get_mut(key)
                        .ok_or_else(|| failed("model_budget_settlement_orphan"))?;
                    *old = charge;
                }
                _ => return Err(failed("model_budget_unknown_required_event")),
            }
        }
        let used = reserved
            .values()
            .try_fold(0u64, |sum, n| sum.checked_add(*n))
            .ok_or_else(|| failed("model_budget_overflow"))?;
        let now = time_ms()?;
        if let Some(first) = records.first() {
            let started = first.data["at_unix_ms"]
                .as_u64()
                .ok_or_else(|| failed("model_budget_clock_missing"))?;
            if now < started || now - started >= limit.max_wall_time_ms {
                return Err(failed("model_budget_wall_time_exhausted"));
            }
        }
        if tokens == 0
            || calls >= limit.max_model_calls
            || used
                .checked_add(tokens)
                .is_none_or(|n| n > limit.max_tokens)
        {
            return Err(failed("model_budget_exhausted_before_provider"));
        }
        let version = records
            .iter()
            .filter_map(|e| e.stream_version)
            .max()
            .unwrap_or(0);
        let command_id = derived_request_id("model.reserve", &request_id.to_string());
        let payload = json!({"run_id":run_id,"model_request_id":request_id,"tokens":tokens,"at_unix_ms":now,
            "request_count":calls+1,"charged_and_reserved_tokens":used+tokens,"limit":limit,"basis":"text_bytes_plus_output_limit"});
        let event = RuntimeEvent::new(command_id, 1, "model.reserved", payload.clone())
            .map_err(|e| failed(&e.to_string()))?
            .with_stream_metadata("model_budget", run_id.to_string(), version + 1);
        let mut batch = TransitionBatch {
            command_id,
            command_digest: json_digest(
                &json!({"run_id":run_id,"request_id":request_id,"tokens":tokens}),
            ),
            expected_versions: vec![
                AggregateVersion {
                    aggregate_type: "authority".to_owned(),
                    aggregate_id: authority_key,
                    version: authority_version,
                },
                AggregateVersion {
                    aggregate_type: "model_budget".to_owned(),
                    aggregate_id: run_id.to_string(),
                    version,
                },
                AggregateVersion {
                    aggregate_type: "run".to_owned(),
                    aggregate_id: run_id.to_string(),
                    version: run
                        .iter()
                        .filter_map(|e| e.stream_version)
                        .max()
                        .unwrap_or(0),
                },
            ],
            events: vec![event],
        };
        if let Some((call, permit)) = prepared {
            let authority_versions = batch
                .expected_versions
                .iter()
                .filter(|v| v.aggregate_type != "model_budget")
                .cloned()
                .collect::<Vec<_>>();
            batch.expected_versions.push(AggregateVersion::new(
                "model_attempt",
                request_id.to_string(),
                0,
            ));
            batch.events.push(RuntimeEvent::new(command_id,2,"model.prepared",json!({"permit":permit,"prepared":call.audit(),"authority_versions":authority_versions}))
                .map_err(|e|failed(&e.to_string()))?.with_stream_metadata("model_attempt",request_id.to_string(),1));
            batch.command_digest =
                json_digest(&json!({"request_hash":call.request_hash,"permit":permit}));
        }
        if super::dispatch::commit_confirmed(self.events.as_ref(), batch).await? {
            return Err(failed("model_request_already_reserved"));
        }
        Ok(())
    }
}

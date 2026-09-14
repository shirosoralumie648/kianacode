use super::*;
use std::collections::BTreeMap;

impl ControlPlane {
    pub(crate) async fn handle_version_command(
        &self,
        context: RequestContext,
        name: &str,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        if !context.project_trusted || context.actor_id.as_deref().is_none_or(str::is_empty) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "project_untrusted",
            ));
        }
        let all = self.events.read_all().await?;
        let owned = all
            .iter()
            .filter(|event| {
                event.kind == "run.authorized"
                    && event.data["actor_id"] == json!(context.actor_id)
                    && event.data["project_root"].as_str().is_some_and(|root| {
                        Self::canonical_project_root(root)
                            == Self::canonical_project_root(&context.project_root)
                    })
            })
            .filter_map(|event| event.data["run_id"].as_str())
            .collect::<HashSet<_>>();
        if name == "version.drift" {
            let mut buckets: BTreeMap<String, Value> = BTreeMap::new();
            for event in all.iter().filter(|event| {
                event.kind == "run.model_turn"
                    && event.data["run_id"]
                        .as_str()
                        .is_some_and(|id| owned.contains(id))
            }) {
                let metadata = &event.data;
                let version = json!({"provider":metadata["provider_id"],"model":metadata["model_id"],"prompt_hash":metadata["prompt_hash"],
                    "budget_schema":metadata["budget"]["schema"],"runtime_version":env!("CARGO_PKG_VERSION")});
                let key = kiana_domain::json_digest(&version);
                let bucket=buckets.entry(key).or_insert_with(||json!({"version":version,"turns":0u64,"errors":0u64,"elapsed_ms":0u64,"event_ids":[]}));
                bucket["turns"] = json!(bucket["turns"].as_u64().unwrap_or(0) + 1);
                bucket["elapsed_ms"] = json!(bucket["elapsed_ms"]
                    .as_u64()
                    .unwrap_or(0)
                    .saturating_add(metadata["elapsed_ms"].as_u64().unwrap_or(0)));
                if metadata.get("error").is_some_and(|error| !error.is_null()) {
                    bucket["errors"] = json!(bucket["errors"].as_u64().unwrap_or(0) + 1);
                }
                bucket["event_ids"]
                    .as_array_mut()
                    .expect("array")
                    .push(json!(event.event_id));
            }
            return Ok(CoreResponse::completed(
                context.request_id,
                json!({"schema":"kiana.drift-report.v1","buckets":buckets,
                "automatic_model_switch":false,"measurement":"observed_turns","cost":"unknown"}),
            ));
        }
        if name == "trace.replay" {
            let id = arguments["trace_id"]
                .as_str()
                .ok_or_else(|| PortError::Failed("trace_id_required".to_owned()))?;
            let event = all
                .iter()
                .find(|event| {
                    event.kind == "golden_trace.captured"
                        && event.data["trace_id"] == id
                        && event.data["owner_id"] == json!(context.actor_id)
                        && event.data["project_root"] == context.project_root
                })
                .ok_or_else(|| PortError::Failed("trace_not_found".to_owned()))?;
            let trace = &event.data;
            let events: Vec<RuntimeEvent> = serde_json::from_value(trace["events"].clone())
                .map_err(|_| PortError::Failed("trace_events_invalid".to_owned()))?;
            if kiana_domain::json_digest(&json!(events)) != trace["events_hash"] {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "trace_digest_mismatch",
                ));
            }
            let run_id = serde_json::from_value::<RunId>(trace["run_id"].clone())
                .map_err(|_| PortError::Failed("trace_run_invalid".to_owned()))?;
            if self.run_data_revoked(run_id).await? {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "trace_data_revoked",
                ));
            }
            let history = crate::history::fold_model_visible_history(
                &events,
                trace["session_id"].as_str().unwrap_or_default(),
                Some(run_id),
            );
            let invocations =
                crate::project_invocations(run_id, &events).map_err(PortError::Failed)?;
            return Ok(CoreResponse::completed(
                context.request_id,
                json!({"schema":"kiana.golden-replay.v1","trace_id":id,
                "history":history,"invocations":invocations,"receipt":trace["receipt"],"side_effects":false,"provider_calls":0}),
            ));
        }
        let run_id = arguments["run_id"]
            .as_str()
            .and_then(RunId::parse_str)
            .ok_or_else(|| PortError::Failed("trace_run_required".to_owned()))?;
        if !owned.contains(run_id.to_string().as_str()) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_owner_mismatch",
            ));
        }
        if self.run_data_revoked(run_id).await? {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "trace_data_revoked",
            ));
        }
        let mut manifest = BTreeMap::new();
        let files = arguments["source_files"]
            .as_array()
            .filter(|files| !files.is_empty() && files.len() <= 128)
            .ok_or_else(|| PortError::Failed("trace_source_manifest_required".to_owned()))?;
        for file in files {
            let path = file
                .as_str()
                .ok_or_else(|| PortError::Failed("trace_source_path_invalid".to_owned()))?;
            let contents = crate::artifacts::read_project_artifact(
                Path::new(&context.project_root),
                Path::new(path),
                "trace_source_unavailable",
            )
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
            manifest.insert(path, kiana_domain::prompt_hash(&contents));
        }
        let events = crate::receipts::filter_run_events(&all, run_id)
            .into_iter()
            .filter(|event| event.kind != "run.snapshot")
            .collect::<Vec<_>>();
        let author = events
            .iter()
            .find(|event| event.kind == "run.authorized")
            .ok_or_else(|| PortError::Failed("trace_author_missing".to_owned()))?;
        let session = author.data["session_id"].clone();
        let source_hash = kiana_domain::json_digest(&json!(manifest));
        let events_hash = kiana_domain::json_digest(&json!(events));
        let trace_id =
            kiana_domain::json_digest(&json!({"source":source_hash,"events":events_hash}));
        let input = events
            .iter()
            .filter(|event| event.kind == "run.prompt")
            .map(|event| &event.data)
            .collect::<Vec<_>>();
        let trace = json!({"schema":"kiana.golden-trace.v1","trace_id":trace_id,"owner_id":context.actor_id,
            "project_root":context.project_root,"run_id":run_id,"session_id":session,
            "source_snapshot":source_hash,"source_manifest":manifest,"input_hash":kiana_domain::json_digest(&json!(input)),
            "runtime_version":env!("CARGO_PKG_VERSION"),"events_hash":events_hash,
            "receipt":crate::receipts::receipt_from_events(&context,run_id,"read-only",Value::Null,&events),"events":events});
        self.append_event(
            context.request_id,
            1,
            "golden_trace.captured",
            trace.clone(),
        )
        .await?;
        Ok(CoreResponse::completed(context.request_id, trace))
    }
}

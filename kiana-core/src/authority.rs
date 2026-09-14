use super::*;
use kiana_domain::{json_digest, AggregateVersion, TransitionBatch};

impl ControlPlane {
    pub(crate) async fn authority_revision(&self, root: &str) -> Result<Option<String>, CoreError> {
        let key = json_digest(&json!({"project_root":Self::canonical_project_root(root)}));
        Ok(self
            .events
            .read_stream("authority", &key)
            .await?
            .last()
            .map(|event| {
                format!(
                    "{}:{}",
                    event.stream_version.unwrap_or(0),
                    event.data["revision_digest"].as_str().unwrap_or_default()
                )
            }))
    }

    pub(crate) async fn commit_protected_event(
        &self,
        context: &RequestContext,
        event: RuntimeEvent,
        expected: u64,
    ) -> Result<RuntimeEvent, CoreError> {
        let aggregate_type = event
            .aggregate_type
            .clone()
            .ok_or_else(|| PortError::Failed("protected_event_scope_required".to_owned()))?;
        let aggregate_id = event
            .aggregate_id
            .clone()
            .ok_or_else(|| PortError::Failed("protected_event_scope_required".to_owned()))?;
        let key = event
            .idempotency_key
            .as_deref()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!("{}:{}:{}", aggregate_type, aggregate_id, context.request_id)
            });
        let command_id = kiana_domain::derived_request_id("protected.command", &key);
        let digest = json_digest(&json!({"kind":event.kind,"data":event.data}));
        let authority_key = json_digest(
            &json!({"project_root":Self::canonical_project_root(&context.project_root)}),
        );
        let authority = self.events.read_stream("authority", &authority_key).await?;
        if authority.is_empty() {
            return Err(PortError::Failed("authority_snapshot_missing".to_owned()).into());
        }
        let batch = TransitionBatch {
            command_id,
            command_digest: digest,
            expected_versions: vec![
                AggregateVersion {
                    aggregate_type: aggregate_type.clone(),
                    aggregate_id: aggregate_id.clone(),
                    version: expected,
                },
                AggregateVersion {
                    aggregate_type: "authority".to_owned(),
                    aggregate_id: authority_key,
                    version: authority
                        .iter()
                        .filter_map(|e| e.stream_version)
                        .max()
                        .unwrap_or(0),
                },
            ],
            events: vec![event.clone()],
        };
        if super::dispatch::commit_confirmed(self.events.as_ref(), batch).await? {
            return self
                .events
                .read_stream(&aggregate_type, &aggregate_id)
                .await?
                .into_iter()
                .find(|old| {
                    old.idempotency_key == event.idempotency_key
                        && old.kind == event.kind
                        && old.data == event.data
                })
                .ok_or_else(|| {
                    PortError::Failed("committed_command_result_missing".to_owned()).into()
                });
        }
        Ok(event)
    }

    /// Called only with daemon-authenticated state, before effectful command admission.
    /// Queries deliberately do not refresh authority or create assignments.
    pub async fn synchronize_authority(
        &self,
        context: &RequestContext,
        configuration_revision: &str,
    ) -> Result<(), CoreError> {
        let project_root = Self::canonical_project_root(&context.project_root);
        let key = json_digest(&json!({"project_root":project_root}));
        let value = json!({"schema":"kiana.authority-revision.v1","project_root":project_root,
            "project_trusted":context.project_trusted,"configuration_revision":configuration_revision});
        let revision_digest = json_digest(&value);
        let records = self.events.read_stream("authority", &key).await?;
        if records
            .last()
            .is_some_and(|event| event.data["revision_digest"] == revision_digest)
        {
            return Ok(());
        }
        let version = records
            .iter()
            .filter_map(|event| event.stream_version)
            .max()
            .unwrap_or(0);
        let command_id = RequestId::new();
        let mut payload = value;
        payload["revision_digest"] = json!(revision_digest);
        let event = RuntimeEvent::new(command_id, 1, "authority.revised", payload)?
            .with_stream_metadata("authority", &key, version + 1);
        let batch = TransitionBatch {
            command_id,
            command_digest: revision_digest,
            expected_versions: vec![AggregateVersion {
                aggregate_type: "authority".to_owned(),
                aggregate_id: key,
                version,
            }],
            events: vec![event],
        };
        super::dispatch::commit_confirmed(self.events.as_ref(), batch).await?;
        if version > 0 {
            self.approvals
                .invalidate_project(&context.project_root, "authority_changed")
                .await?;
            self.stop_project_runs(&context.project_root, "authority_changed")
                .await?;
        }
        Ok(())
    }
}

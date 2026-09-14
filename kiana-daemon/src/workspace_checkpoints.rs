use crate::apply_patch::{capture_checkpoint_files, preview_checkpoint, restore_checkpoint};
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{
    AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult, WorkspaceCheckpoint,
};
use kiana_ports::{PortError, WorkspaceCheckpointPort};
use std::sync::Arc;

pub(crate) struct LocalWorkspaceCheckpoints;

#[async_trait::async_trait]
impl WorkspaceCheckpointPort for LocalWorkspaceCheckpoints {
    async fn capture_files(
        &self,
        project_root: &str,
        paths: &[String],
    ) -> Result<Vec<kiana_domain::WorkspaceFileSnapshot>, PortError> {
        let root = project_root.to_owned();
        let paths = paths.to_vec();
        tokio::task::spawn_blocking(move || {
            checkpoint_sources_allowed(&root, &paths)?;
            capture_checkpoint_files(std::path::Path::new(&root), &paths)
        })
        .await
        .map_err(|_| failed("checkpoint_capture_join_failed"))?
    }
    async fn preview(
        &self,
        checkpoint: &WorkspaceCheckpoint,
    ) -> Result<kiana_domain::CheckpointPreview, PortError> {
        let checkpoint = checkpoint.clone();
        tokio::task::spawn_blocking(move || {
            checkpoint_sources_allowed(
                &checkpoint.project_root,
                &checkpoint
                    .files
                    .iter()
                    .map(|file| file.path.clone())
                    .collect::<Vec<_>>(),
            )?;
            preview_checkpoint(&checkpoint)
        })
        .await
        .map_err(|_| failed("checkpoint_preview_join_failed"))?
    }
}

pub(crate) fn register(broker: &mut CapabilityBroker) -> Result<(), PortError> {
    broker.register_static(
        CapabilityKind::Filesystem,
        "workspace.checkpoint.restore",
        Arc::new(CheckpointRestoreHandler),
    )
}
struct CheckpointRestoreHandler;
#[async_trait::async_trait]
impl CapabilityHandler for CheckpointRestoreHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        if request.request.operation != "workspace.checkpoint.restore"
            || request.request.risk != kiana_domain::RiskLevel::Critical
            || request.request.arguments["operator_authorized"] != true
            || request.request.cell_id.is_some()
        {
            return Err(failed("checkpoint_restore_operator_required"));
        }
        let id = request.request.request_id;
        let checkpoint: WorkspaceCheckpoint =
            serde_json::from_value(request.request.arguments["snapshot"].clone())
                .map_err(|_| failed("checkpoint_snapshot_invalid"))?;
        for (key, expected) in [
            ("project_root", checkpoint.project_root.as_str()),
            ("actor_id", checkpoint.actor_id.as_str()),
            ("session_id", checkpoint.session_id.as_str()),
            ("role_id", checkpoint.role_id.as_str()),
        ] {
            if request.request.arguments[key].as_str() != Some(expected) {
                return Err(failed("checkpoint_restore_scope_changed"));
            }
        }
        let paths: Vec<String> =
            serde_json::from_value(request.request.arguments["path_allow"].clone())
                .map_err(|_| failed("checkpoint_scope_required"))?;
        if checkpoint.files.is_empty()
            || checkpoint.files.iter().any(|file| {
                !kiana_domain::allow_list_covers(&paths, &file.path)
                    || (!checkpoint.path_allow.is_empty()
                        && !kiana_domain::allow_list_covers(&checkpoint.path_allow, &file.path))
            })
        {
            return Err(failed("checkpoint_path_denied"));
        }
        let revision = request.request.arguments["expected_revision"]
            .as_str()
            .ok_or_else(|| failed("checkpoint_revision_required"))?
            .to_owned();
        let output = tokio::task::spawn_blocking(move || {
            checkpoint_sources_allowed(
                &checkpoint.project_root,
                &checkpoint
                    .files
                    .iter()
                    .map(|file| file.path.clone())
                    .collect::<Vec<_>>(),
            )?;
            restore_checkpoint(&checkpoint, &revision)
        })
        .await
        .map_err(|_| failed("result_unknown:checkpoint_restore_join_failed"))??;
        Ok(CapabilityResult::success(id, output))
    }
}
fn failed(reason: &str) -> PortError {
    PortError::Failed(reason.to_owned())
}

fn checkpoint_sources_allowed(root: &str, paths: &[String]) -> Result<(), PortError> {
    let policy = crate::data_governance::read_policy(std::path::Path::new(root))?;
    if paths.iter().any(|path| {
        policy
            .revoked_sources
            .iter()
            .any(|source| path == source || path.starts_with(&format!("{source}/")))
    }) {
        return Err(failed("checkpoint_source_revoked"));
    }
    Ok(())
}

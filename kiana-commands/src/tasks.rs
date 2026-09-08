use crate::swarm_process_identity::{
    capture_worker_process_identity, check_worker_process_identity, identity_digest,
    process_identity_backend_capability, ProcessIdentityStatus, WorkerProcessIdentity,
};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use kiana_tasks::{
    append_workflow_event_idempotent_with_unique_data_value, append_workflow_transition,
    build_project_board_at_root, build_swarm_plan, default_workflow_template,
    initialize_local_hmac_key, initialize_workflow_run, inspect_local_hmac_key,
    inspect_workflow_integrity, list_workflow_runs, load_local_hmac_key, persist_swarm_dispatch,
    prepare_swarm_execution, read_evidence_events, read_workflow_events, read_workflow_state,
    resume_workflow_run, seal_legacy_workflow, sha256_prefixed,
    validate_verification_packet_completion, validate_verification_packet_integrity,
    IntegrityEnvelope, LocalHmacKey, SwarmDispatchManifest, SwarmExecutionManifest,
    SwarmExecutionRequest, SwarmWorkPacket, SwarmWorkerBudget, SwarmWorkerLaunch,
    SwarmWorkerLaunchInput, VerificationPacket, VerificationStatus, WorkflowArtifactBatch,
    WorkflowArtifactInput, WorkflowDagTemplate, WorkflowEvent, WorkflowEventKind, WorkflowInit,
    WorkflowInputKind, WorkflowProfile, WorkflowStatus, WorkflowTransitionEvidence,
    WorkflowTransitionInput,
};
#[cfg(target_os = "linux")]
use kiana_tools::bash_sandbox::{bash_sandbox_bwrap_path, strict_bwrap_plan, StrictBwrapPlan};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(target_os = "linux")]
use std::ffi::CString;
#[cfg(unix)]
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
#[cfg(target_os = "linux")]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct TasksCommand;

#[async_trait]
impl Command for TasksCommand {
    fn name(&self) -> &str {
        "tasks"
    }

    fn description(&self) -> &str {
        "Manage tasks"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let (command, rest) = split_word(context.args.trim());
        match command.unwrap_or("list") {
            "" | "list" | "status" => list_tasks(&context, optional_task_list_arg(rest)?),
            "json" => tasks_json(&context, optional_task_list_arg(rest)?),
            "plan" => team_plan(&context, rest),
            "swarm" => swarm_command(&context, rest),
            "workflow" => workflow_command(&context, rest),
            "show" | "get" => show_task(&context, rest),
            "path" => Ok(CommandResult::text(
                task_list_dir(&context, optional_task_list_arg(rest)?)
                    .display()
                    .to_string(),
            )),
            "help" | "--help" | "-h" => Ok(CommandResult::text(usage())),
            other => {
                if looks_like_status(other) {
                    list_tasks_with_status(&context, other, optional_task_list_arg(rest)?)
                } else {
                    Err(anyhow!("unknown tasks command '{}'\n\n{}", other, usage()))
                }
            }
        }
    }
}

fn swarm_command(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let (command, rest) = split_word(rest);
    match command.unwrap_or("help") {
        "plan" => swarm_plan(context, rest),
        "dispatch" => swarm_dispatch(context, rest),
        "start" => swarm_start(context, rest),
        "status" => swarm_status(context, rest),
        "monitor" => swarm_monitor(context, rest),
        "cancel" => swarm_cancel(context, rest),
        "integrate" => swarm_integrate_command(context, rest),
        "cleanup" => swarm_cleanup(context, rest),
        "help" | "--help" | "-h" => Ok(CommandResult::text(swarm_usage())),
        other => Err(anyhow!(
            "unknown swarm command '{}'

{}",
            other,
            swarm_usage()
        )),
    }
}

fn swarm_integrate_command(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let (command, rest) = split_word(rest);
    match command.unwrap_or("help") {
        "plan" => swarm_integrate_plan(context, rest),
        "status" => swarm_integrate_status(context, rest),
        "apply" => swarm_integrate_apply(context, rest),
        "help" | "--help" | "-h" => Ok(CommandResult::text(swarm_usage())),
        other => Err(anyhow!(
            "unknown swarm integrate command '{}'\n\n{}",
            other,
            swarm_usage()
        )),
    }
}

#[derive(Debug)]
struct SwarmIntegrationApplyLease {
    #[cfg(target_os = "linux")]
    file: fs::File,
}

fn acquire_swarm_integration_apply_lease(
    integration_dir: &Path,
) -> Result<SwarmIntegrationApplyLease> {
    #[cfg(target_os = "linux")]
    {
        fs::create_dir_all(integration_dir)?;
        let path = integration_dir.join(".apply.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&path)
            .with_context(|| {
                format!("failed to open integration apply lease {}", path.display())
            })?;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            let raw_error = error.raw_os_error();
            if raw_error == Some(libc::EWOULDBLOCK) || raw_error == Some(libc::EAGAIN) {
                return Err(anyhow!(
                    "swarm integration apply is already active for {}",
                    integration_dir.display()
                ));
            }
            return Err(error).with_context(|| {
                format!(
                    "failed to acquire integration apply lease {}",
                    path.display()
                )
            });
        }
        return Ok(SwarmIntegrationApplyLease { file });
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = integration_dir;
        Err(anyhow!(
            "swarm integration apply lease requires Linux flock support"
        ))
    }
}

#[cfg(target_os = "linux")]
impl Drop for SwarmIntegrationApplyLease {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn swarm_integrate_apply(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let args = parse_swarm_execution_args(rest, "integrate apply")?;
    ensure_secure_swarm_integration_platform()?;
    let artifact_dir =
        ready_swarm_artifact_dir(context, &args.workflow_run_id, "integration apply")?;
    let integration_dir = artifact_dir.join("integrations").join(&args.dispatch_id);
    let _apply_lease = acquire_swarm_integration_apply_lease(&integration_dir)?;
    let packet_path = integration_dir.join("packet.json");
    let plan_path = integration_dir.join("plan.json");
    let plan: Value = read_json_file(&plan_path).with_context(|| {
        format!(
            "integration plan is required for dispatch {}; run integrate plan first",
            args.dispatch_id
        )
    })?;
    if plan["schema"] != "kiana.swarm-integration-plan.v1"
        || plan["dispatch_id"] != args.dispatch_id
    {
        return Err(anyhow!("integration plan identity mismatch"));
    }
    if plan["status"] != "ready" {
        return Err(anyhow!(
            "integration plan is not ready: {}",
            plan["status"].as_str().unwrap_or("unknown")
        ));
    }
    verify_integration_plan_authentication(&artifact_dir, &plan)?;

    let project_root = cwd(context);
    let state_path = integration_dir.join("state.json");
    if let Some(packet) = read_optional_json(&packet_path)? {
        if packet["status"] == "integrated" {
            if !integration_completed_event_exists(&artifact_dir, &plan)? {
                return recover_orphan_completed_integration(
                    &artifact_dir,
                    &integration_dir,
                    &project_root,
                    &plan,
                    &packet_path,
                    packet,
                    args.json_output,
                );
            }
            let packet_sha256 = verify_integration_packet_authentication(
                &artifact_dir,
                &packet_path,
                &plan,
                &packet,
            )?;
            let mut completed_state = completed_integration_state(&plan, &packet)?;
            completed_state["packet_sha256"] = json!(packet_sha256);
            write_json_atomic(&state_path, &completed_state)?;
            return format_swarm_integration_result(packet, args.json_output);
        }
    }

    if let Some(existing_state) = read_optional_json(&state_path)? {
        if matches!(
            existing_state["status"].as_str(),
            Some("applying" | "verifying")
        ) && integration_verification_event_exists(&artifact_dir, &plan)?
        {
            return recover_verified_integration_before_packet(
                &artifact_dir,
                &integration_dir,
                &project_root,
                &plan,
                existing_state,
                args.json_output,
            );
        }
        if matches!(
            existing_state["status"].as_str(),
            Some("applying" | "verifying" | "rolling_back")
        ) {
            return recover_interrupted_swarm_integration(
                &artifact_dir,
                &integration_dir,
                &project_root,
                &plan,
                existing_state,
                args.json_output,
            );
        }
    }
    let current_manifest = file_manifest(&project_root)?;
    let current_git_state = git_state_snapshot(&project_root)?;
    let current_fingerprint = working_tree_fingerprint(&current_git_state, &current_manifest)?;
    let expected_fingerprint = plan["baseline"]["working_tree_fingerprint"]
        .as_str()
        .ok_or_else(|| anyhow!("integration plan baseline fingerprint is missing"))?;
    if current_fingerprint != expected_fingerprint {
        let stale = integration_state_value(
            &plan,
            "stale",
            Vec::new(),
            Vec::new(),
            None,
            Some(json!({
                "reason": "main_tree_drift",
                "expected": expected_fingerprint,
                "actual": current_fingerprint,
            })),
            "rerun_swarm_integrate_plan",
        );
        write_json_atomic(&integration_dir.join("state.json"), &stale)?;
        return format_swarm_integration_result(stale, args.json_output);
    }

    let workers = plan["workers"]
        .as_array()
        .ok_or_else(|| anyhow!("integration plan workers are missing"))?;
    let mut changed_paths = BTreeSet::new();
    let mut verified_patch_bytes = BTreeMap::new();
    for worker in workers {
        let task_id = worker["task_id"]
            .as_str()
            .ok_or_else(|| anyhow!("integration worker task_id is missing"))?;
        let patch_bytes = verify_integration_worker_artifacts(&artifact_dir, worker)?;
        if verified_patch_bytes
            .insert(task_id.to_string(), patch_bytes)
            .is_some()
        {
            return Err(anyhow!("duplicate integration worker task_id {task_id}"));
        }
        for path in worker["changed_files"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            changed_paths.insert(path.to_string());
        }
    }
    let (checkpoint_path, checkpoint_sha256) = create_integration_checkpoint(
        &artifact_dir,
        &project_root,
        &integration_dir,
        &changed_paths.into_iter().collect::<Vec<_>>(),
        &plan,
    )?;
    let integration_id = plan["integration_id"]
        .as_str()
        .ok_or_else(|| anyhow!("integration_id is missing"))?
        .to_string();
    let mut state = integration_state_value(
        &plan,
        "applying",
        Vec::new(),
        Vec::new(),
        None,
        None,
        "continue_integration",
    );
    state["checkpoint_path"] = json!(checkpoint_path.clone());
    state["checkpoint_sha256"] = json!(checkpoint_sha256.clone());
    state["working_tree_fingerprint"] = json!(expected_fingerprint);
    write_json_atomic(&integration_dir.join("state.json"), &state)?;
    append_swarm_integration_started_event(
        &artifact_dir,
        &plan,
        &checkpoint_path,
        &checkpoint_sha256,
        expected_fingerprint,
    )?;

    let mut integrated_task_ids = Vec::new();
    let mut verified_task_ids = Vec::new();
    let mut command_results = Vec::new();
    for worker in workers {
        let task_id = worker["task_id"]
            .as_str()
            .ok_or_else(|| anyhow!("integration worker task_id is missing"))?;
        state["status"] = json!("applying");
        state["current_task_id"] = json!(task_id);
        state["updated_at_ms"] = json!(now_ms());
        write_json_atomic(&integration_dir.join("state.json"), &state)?;
        let patch_bytes = verified_patch_bytes
            .get(task_id)
            .ok_or_else(|| anyhow!("verified patch bytes are missing for worker {task_id}"))?;
        if let Err(error) = git_apply_check_bytes(&project_root, patch_bytes)
            .and_then(|_| git_apply_patch_bytes(&project_root, patch_bytes))
        {
            return finish_failed_swarm_integration(
                &artifact_dir,
                &integration_dir,
                &project_root,
                &plan,
                integrated_task_ids,
                verified_task_ids,
                command_results,
                json!({
                    "task_id": task_id,
                    "phase": "apply",
                    "reason": error.to_string(),
                }),
                args.json_output,
            );
        }
        integrated_task_ids.push(task_id.to_string());
        state["applied_task_ids"] = json!(integrated_task_ids);
        let applied_fingerprint = current_working_tree_fingerprint(&project_root)?;
        state["working_tree_fingerprint"] = json!(applied_fingerprint.clone());
        append_swarm_worker_applied_event(
            &artifact_dir,
            &plan,
            task_id,
            &integrated_task_ids,
            &checkpoint_path,
            &checkpoint_sha256,
            &applied_fingerprint,
        )?;
        let commands = worker["verification_commands"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        state["status"] = json!(if commands.is_empty() {
            "applying"
        } else {
            "verifying"
        });
        state["updated_at_ms"] = json!(now_ms());
        write_json_atomic(&integration_dir.join("state.json"), &state)?;
        if !commands.is_empty() {
            let verification_snapshot = match create_integration_verification_snapshot(
                &project_root,
                &integration_dir,
                task_id,
            ) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    return finish_failed_swarm_integration(
                        &artifact_dir,
                        &integration_dir,
                        &project_root,
                        &plan,
                        integrated_task_ids,
                        verified_task_ids,
                        command_results,
                        json!({
                            "task_id": task_id,
                            "phase": "verification",
                            "reason": "verification_snapshot_failed",
                            "detail": error.to_string(),
                        }),
                        args.json_output,
                    );
                }
            };
            for command in commands.iter().filter_map(Value::as_str) {
                let result = match run_swarm_integration_command(
                    context,
                    &verification_snapshot,
                    &project_root,
                    task_id,
                    command,
                ) {
                    Ok(result) => result,
                    Err(error) => {
                        let _ = remove_integration_verification_snapshot(&verification_snapshot);
                        return finish_failed_swarm_integration(
                            &artifact_dir,
                            &integration_dir,
                            &project_root,
                            &plan,
                            integrated_task_ids,
                            verified_task_ids,
                            command_results,
                            json!({
                                "task_id": task_id,
                                "phase": "verification",
                                "reason": "verification_sandbox_failed",
                                "command": command,
                                "detail": error.to_string(),
                            }),
                            args.json_output,
                        );
                    }
                };
                let passed = result["status"] == "pass";
                command_results.push(result);
                if !passed {
                    let _ = remove_integration_verification_snapshot(&verification_snapshot);
                    return finish_failed_swarm_integration(
                        &artifact_dir,
                        &integration_dir,
                        &project_root,
                        &plan,
                        integrated_task_ids,
                        verified_task_ids,
                        command_results,
                        json!({
                            "task_id": task_id,
                            "phase": "verification",
                            "reason": "verification_command_failed",
                            "command": command,
                        }),
                        args.json_output,
                    );
                }
            }
            if let Err(error) = remove_integration_verification_snapshot(&verification_snapshot) {
                return finish_failed_swarm_integration(
                    &artifact_dir,
                    &integration_dir,
                    &project_root,
                    &plan,
                    integrated_task_ids,
                    verified_task_ids,
                    command_results,
                    json!({
                        "task_id": task_id,
                        "phase": "verification",
                        "reason": "verification_snapshot_cleanup_failed",
                        "detail": error.to_string(),
                    }),
                    args.json_output,
                );
            }
        }
        verified_task_ids.push(task_id.to_string());
        state["verified_task_ids"] = json!(verified_task_ids);
        state["updated_at_ms"] = json!(now_ms());
        write_json_atomic(&integration_dir.join("state.json"), &state)?;
        append_swarm_worker_integrated_event(
            &artifact_dir,
            &integration_id,
            &args.dispatch_id,
            task_id,
        )?;
    }

    let verification = json!({
        "schema": "kiana.swarm-integration-verification.v1",
        "integration_id": integration_id,
        "dispatch_id": args.dispatch_id,
        "status": "pass",
        "checks": command_results,
        "verified_task_ids": verified_task_ids,
        "created_at_ms": now_ms(),
    });
    commit_swarm_integration_verification(
        &artifact_dir,
        &plan,
        &verification,
        &checkpoint_path,
        &checkpoint_sha256,
    )?;
    let packet = commit_completed_integration_packet(&artifact_dir, &plan, &verification)?;
    let packet_sha256 =
        verify_integration_packet_authentication(&artifact_dir, &packet_path, &plan, &packet)?;
    state["status"] = json!("integrated");
    state["current_task_id"] = Value::Null;
    state["packet_sha256"] = json!(packet_sha256);
    state["next_action"] = json!("run_swarm_cleanup_or_review");
    state["updated_at_ms"] = json!(now_ms());
    write_json_atomic(&integration_dir.join("state.json"), &state)?;
    format_swarm_integration_result(packet, args.json_output)
}

fn swarm_integrate_plan(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let args = parse_swarm_execution_args(rest, "integrate plan")?;
    ensure_secure_swarm_integration_platform()?;
    let artifact_dir =
        ready_swarm_artifact_dir(context, &args.workflow_run_id, "integration plan")?;
    let project_root = cwd(context);
    let dispatch_path = artifact_dir
        .join("workpackets")
        .join(&args.dispatch_id)
        .join("manifest.json");
    let dispatch: SwarmDispatchManifest = read_json_file(&dispatch_path)?;
    if dispatch.dispatch_id != args.dispatch_id {
        return Err(anyhow!("dispatch manifest identity mismatch"));
    }
    let execution_path = artifact_dir
        .join("workers")
        .join(&args.dispatch_id)
        .join("manifest.json");
    let execution: SwarmExecutionManifest = read_json_file(&execution_path)?;
    if execution.dispatch_id != args.dispatch_id
        || execution.workflow_id != dispatch.workflow_id
        || execution.run_id != dispatch.run_id
    {
        return Err(anyhow!("execution manifest identity mismatch"));
    }

    let root_manifest = file_manifest(&project_root)?;
    let git_state = git_state_snapshot(&project_root)?;
    let baseline_fingerprint = working_tree_fingerprint(&git_state, &root_manifest)?;
    let integration_base = format!("integrations/{}", dispatch.dispatch_id);
    let patch_base = artifact_dir.join(&integration_base).join("patches");
    fs::create_dir_all(&patch_base)?;

    let mut workers = Vec::new();
    let mut global_blockers = Vec::new();
    let mut paths_by_task = BTreeMap::<String, Vec<String>>::new();
    let mut integration_seed = vec![
        dispatch.dispatch_id.clone(),
        baseline_fingerprint.clone(),
        execution.runner_fingerprint.clone(),
    ];
    let workflow_events = read_workflow_events(&artifact_dir)?;

    for task_id in &dispatch.task_ids {
        let launch_path = artifact_dir
            .join("workers")
            .join(&dispatch.dispatch_id)
            .join(task_id)
            .join("launch.json");
        let launch: SwarmWorkerLaunch = read_json_file(&launch_path)?;
        let result_relative = format!("workers/{}/{task_id}/result.json", dispatch.dispatch_id);
        let result_path = artifact_dir.join(&result_relative);
        let mut worker_blockers = Vec::new();
        let result: Value = match read_optional_json(&result_path)? {
            Some(result) => result,
            None => {
                worker_blockers.push("missing_result_packet".to_string());
                json!({})
            }
        };
        if !result.is_null() && !result.as_object().is_some_and(|value| value.is_empty()) {
            if result["schema"] != "kiana.swarm-result-packet.v1" {
                worker_blockers.push("invalid_result_schema".to_string());
            }
            if result["worker_id"] != launch.worker_id
                || result["dispatch_id"] != dispatch.dispatch_id
                || result["task_id"] != *task_id
            {
                worker_blockers.push("result_identity_mismatch".to_string());
            }
            if result["status"] != "completed" || result["termination_reason"] != "completed" {
                worker_blockers.push("worker_not_completed".to_string());
            }
            if result["scope_deviations"]
                .as_array()
                .is_none_or(|values| !values.is_empty())
            {
                worker_blockers.push("scope_deviation".to_string());
            }
        }

        let changed_files = result["changed_files"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if changed_files.is_empty() {
            worker_blockers.push("empty_result".to_string());
        }
        let packet: SwarmWorkPacket = read_json_file(&artifact_dir.join(&launch.workpacket_path))?;
        let worker_dir = artifact_dir
            .join("workers")
            .join(&dispatch.dispatch_id)
            .join(task_id);
        let worker_baseline: BTreeMap<String, String> =
            read_json_file(&worker_dir.join("baseline.json"))?;
        let isolation_root = project_root.join(&launch.isolation_path);
        let mut patches = Vec::new();

        for changed_file in &changed_files {
            let Some(relative) = safe_swarm_relative_path(Path::new(changed_file)) else {
                worker_blockers.push(format!("unsafe_changed_path:{changed_file}"));
                continue;
            };
            if !path_allowed_by_packet(changed_file, &packet) {
                worker_blockers.push(format!("path_outside_scope:{changed_file}"));
                continue;
            }
            if root_manifest.get(changed_file) != worker_baseline.get(changed_file) {
                worker_blockers.push(format!("main_tree_drift:{changed_file}"));
                continue;
            }
            let baseline_path = project_root.join(&relative);
            let candidate_path = isolation_root.join(&relative);
            let patch = crate::diff::no_index_diff_for_path(
                changed_file,
                &baseline_path,
                baseline_path.exists(),
                &candidate_path,
                candidate_path.exists(),
            )?;
            if patch.trim().is_empty() {
                worker_blockers.push(format!("empty_patch:{changed_file}"));
            } else {
                patches.push(patch);
            }
        }

        let patch_relative = format!("{integration_base}/patches/{task_id}.patch");
        let patch_path = artifact_dir.join(&patch_relative);
        let patch = if patches.is_empty() {
            String::new()
        } else {
            format!("{}\n", patches.join("\n"))
        };
        fs::write(&patch_path, patch.as_bytes())?;
        let patch_bytes = read_regular_file_nofollow(&patch_path)?;
        let patch_sha256 = format!("sha256:{}", sha256_bytes(&patch_bytes));
        let result_sha256 = if result_path.is_file() {
            format!("sha256:{}", sha256_file(&result_path)?)
        } else {
            "missing".to_string()
        };
        let result_event = workflow_events.iter().find(|event| {
            event.kind == WorkflowEventKind::ResultPacketCreated
                && event.data["result_packet_id"] == launch.worker_id
        });
        match result_event {
            Some(event) => {
                if event.data["result_packet_sha256"] != result_sha256 {
                    worker_blockers.push("result_packet_event_mismatch".to_string());
                }
                if event.data["isolation_manifest_sha256"] != result["isolation_manifest_sha256"] {
                    worker_blockers.push("result_manifest_event_mismatch".to_string());
                }
            }
            None => worker_blockers.push("missing_result_packet_event".to_string()),
        }
        let current_isolation_manifest = file_manifest(&isolation_root)?;
        let current_isolation_sha256 = manifest_sha256(&current_isolation_manifest)?;
        if result["isolation_manifest_sha256"].as_str() != Some(current_isolation_sha256.as_str()) {
            worker_blockers.push("isolation_manifest_mismatch".to_string());
        }
        if worker_blockers.is_empty() {
            if let Err(reason) = git_apply_check_bytes(&project_root, &patch_bytes) {
                worker_blockers.push(format!("patch_apply_check_failed:{reason}"));
            }
        }
        paths_by_task.insert(task_id.clone(), changed_files.clone());
        integration_seed.extend([task_id.clone(), result_sha256.clone(), patch_sha256.clone()]);
        workers.push(json!({
            "task_id": task_id,
            "worker_id": launch.worker_id,
            "result_packet_path": result_relative,
            "result_packet_sha256": result_sha256,
            "patch_path": patch_relative,
            "patch_sha256": patch_sha256,
            "changed_files": changed_files,
            "verification_commands": packet.verification_commands,
            "status": if worker_blockers.is_empty() { "ready" } else { "blocked" },
            "blockers": worker_blockers,
        }));
    }

    let conflicts = integration_path_conflicts(&paths_by_task);
    if workers.iter().any(|worker| worker["status"] == "blocked") {
        global_blockers.push("worker_preflight_failed".to_string());
    }
    if !conflicts.is_empty() {
        global_blockers.push("path_conflict".to_string());
    }
    let status = if global_blockers.is_empty() {
        "ready"
    } else {
        "blocked"
    };
    let integration_digest = sha256_text(&integration_seed.join("\n"));
    let integration_id = format!("integration-{}", &integration_digest[..24]);
    let plan_relative = format!("{integration_base}/plan.json");
    let plan_path = artifact_dir.join(&plan_relative);
    let existing_plan = read_optional_json(&plan_path)?;
    let reusable_plan = existing_plan
        .as_ref()
        .filter(|plan| plan["integration_id"].as_str() == Some(integration_id.as_str()));
    let created_at_ms = reusable_plan
        .and_then(|plan| plan["created_at_ms"].as_u64())
        .unwrap_or_else(now_ms);
    let mut plan = json!({
        "schema": "kiana.swarm-integration-plan.v1",
        "integration_id": integration_id,
        "workflow_id": dispatch.workflow_id.clone(),
        "run_id": dispatch.run_id.clone(),
        "dispatch_id": dispatch.dispatch_id.clone(),
        "status": status,
        "platform_capability": swarm_integration_platform_capability(),
        "baseline": {
            "git_state": git_state.clone(),
            "working_tree_fingerprint": baseline_fingerprint.clone(),
            "captured_at_ms": created_at_ms,
        },
        "workers": workers.clone(),
        "conflicts": conflicts.clone(),
        "blockers": global_blockers.clone(),
        "created_at_ms": created_at_ms,
        "plan_path": plan_relative.clone(),
        "plan_sha256": Value::Null,
        "next_action": if status == "ready" { "run_swarm_integrate_apply" } else { "inspect_integration_blockers" },
    });
    let plan_sha256 = integration_plan_sha256(&plan)?;
    plan["plan_sha256"] = json!(plan_sha256);
    if let Some(existing_plan) = reusable_plan {
        if existing_plan != &plan {
            return Err(anyhow!(
                "existing integration plan does not match current integration inputs"
            ));
        }
    } else {
        write_json_atomic(&plan_path, &plan)?;
    }
    append_swarm_integration_plan_event(&artifact_dir, &plan)?;

    if args.json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&plan)?));
    }
    Ok(CommandResult::text(format!(
        "Bounded Swarm integration plan\ndispatch_id: {}\nstatus: {}\nsecure_recovery: {}\nworkers: {}\nblockers: {}\nconflicts: {}\nnext_action: {}",
        args.dispatch_id,
        status,
        plan["platform_capability"]["secure_recovery"]
            .as_str()
            .unwrap_or("unsupported_platform"),
        plan["workers"].as_array().map_or(0, Vec::len),
        plan["blockers"].as_array().map_or(0, Vec::len),
        plan["conflicts"].as_array().map_or(0, Vec::len),
        plan["next_action"].as_str().unwrap_or_default(),
    )))
}

fn swarm_integration_platform_capability() -> Value {
    if cfg!(target_os = "linux") {
        json!({
            "schema": "kiana.swarm-integration-platform-capability.v1",
            "platform": std::env::consts::OS,
            "secure_recovery": "supported",
            "recovery_backend": "linux_openat2_renameat2_journal_v1",
            "supports_parent_symlink_rejection": true,
            "supports_descriptor_verified_exchange": true,
            "supports_crash_resumable_multi_path_rollback": true,
        })
    } else {
        json!({
            "schema": "kiana.swarm-integration-platform-capability.v1",
            "platform": std::env::consts::OS,
            "secure_recovery": "unsupported_platform",
            "recovery_backend": Value::Null,
            "supports_parent_symlink_rejection": false,
            "supports_descriptor_verified_exchange": false,
            "supports_crash_resumable_multi_path_rollback": false,
        })
    }
}

fn ensure_secure_swarm_integration_platform() -> Result<()> {
    if cfg!(target_os = "linux") {
        Ok(())
    } else {
        Err(anyhow!(
            "unsupported_platform: bounded swarm integration requires the Linux openat2/renameat2 recovery backend"
        ))
    }
}

fn append_swarm_integration_plan_event(artifact_dir: &Path, plan: &Value) -> Result<()> {
    let integration_id = plan["integration_id"]
        .as_str()
        .ok_or_else(|| anyhow!("integration plan id is missing"))?
        .to_string();
    let event_integration_id = integration_id.clone();
    let event_dispatch_id = plan["dispatch_id"].clone();
    let event_status = plan["status"].clone();
    let event_plan_path = plan["plan_path"].clone();
    let event_plan_sha256 = plan["plan_sha256"].clone();
    let kind = if plan["status"] == "ready" {
        WorkflowEventKind::SwarmIntegrationPlanned
    } else {
        WorkflowEventKind::SwarmIntegrationBlocked
    };
    let _ = kiana_tasks::append_workflow_event_idempotent_with_unique_data_value(
        artifact_dir,
        kind,
        "swarm_integration",
        "integration_id",
        integration_id,
        move |sequence, at_ms| {
            json!({
                "integration_id": event_integration_id,
                "dispatch_id": event_dispatch_id,
                "status": event_status,
                "plan_path": event_plan_path,
                "plan_sha256": event_plan_sha256,
                "sequence": sequence,
                "created_at_ms": at_ms,
            })
        },
    )?;
    Ok(())
}

fn swarm_integrate_status(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let args = parse_swarm_execution_args(rest, "integrate status")?;
    let artifact_dir =
        ready_swarm_artifact_dir(context, &args.workflow_run_id, "integration status")?;
    let integration_dir = artifact_dir.join("integrations").join(&args.dispatch_id);
    let packet = read_optional_json(&integration_dir.join("packet.json"))?;
    let state = read_optional_json(&integration_dir.join("state.json"))?;
    let plan = read_optional_json(&integration_dir.join("plan.json"))?;
    let result = packet.or(state).or(plan).ok_or_else(|| {
        anyhow!(
            "integration has not been planned for dispatch {}",
            args.dispatch_id
        )
    })?;
    if args.json_output {
        Ok(CommandResult::text(serde_json::to_string_pretty(&result)?))
    } else {
        Ok(CommandResult::text(format!(
            "Bounded Swarm integration status\ndispatch_id: {}\nstatus: {}\nnext_action: {}",
            args.dispatch_id,
            result["status"].as_str().unwrap_or("unknown"),
            result["next_action"]
                .as_str()
                .unwrap_or("inspect_integration"),
        )))
    }
}

fn verify_swarm_cleanup_worker_facts(
    artifact_dir: &Path,
    execution: &SwarmExecutionManifest,
    launch: &SwarmWorkerLaunch,
    state: &Value,
    isolation_path: &Path,
) -> Result<()> {
    let result_relative = format!(
        "workers/{}/{}/result.json",
        execution.dispatch_id, launch.task_id
    );
    if state["result_path"].as_str() != Some(result_relative.as_str()) {
        return Err(anyhow!(
            "cleanup worker state ResultPacket path mismatch for {}",
            launch.worker_id
        ));
    }
    let result_path = artifact_dir.join(&result_relative);
    let result_bytes = fs::read(&result_path)
        .with_context(|| format!("worker {} has no persisted ResultPacket", launch.worker_id))?;
    let result_sha256 = format!("sha256:{}", sha256_bytes(&result_bytes));
    let result: Value = serde_json::from_slice(&result_bytes)?;
    if result["schema"] != "kiana.swarm-result-packet.v1"
        || result["workflow_id"].as_str() != Some(execution.workflow_id.as_str())
        || result["run_id"].as_str() != Some(execution.run_id.as_str())
        || result["dispatch_id"].as_str() != Some(execution.dispatch_id.as_str())
        || result["task_id"].as_str() != Some(launch.task_id.as_str())
        || result["worker_id"].as_str() != Some(launch.worker_id.as_str())
    {
        return Err(anyhow!(
            "cleanup ResultPacket identity mismatch for {}",
            launch.worker_id
        ));
    }
    let event = read_workflow_events(artifact_dir)?
        .into_iter()
        .find(|event| {
            event.kind == WorkflowEventKind::ResultPacketCreated
                && event.data["worker_id"] == launch.worker_id
                && event.data["dispatch_id"] == execution.dispatch_id
        })
        .ok_or_else(|| {
            anyhow!(
                "cleanup ResultPacket event missing for {}",
                launch.worker_id
            )
        })?;
    if event.data["result_path"].as_str() != Some(result_relative.as_str())
        || event.data["result_packet_sha256"].as_str() != Some(result_sha256.as_str())
    {
        return Err(anyhow!(
            "cleanup ResultPacket event hash mismatch for {}",
            launch.worker_id
        ));
    }
    let recorded_manifest_sha256 = result["isolation_manifest_sha256"]
        .as_str()
        .ok_or_else(|| anyhow!("cleanup ResultPacket manifest hash missing"))?;
    if event.data["isolation_manifest_sha256"].as_str() != Some(recorded_manifest_sha256) {
        return Err(anyhow!(
            "cleanup ResultPacket manifest event mismatch for {}",
            launch.worker_id
        ));
    }
    if !isolation_path.is_dir() {
        return Err(anyhow!(
            "cleanup isolation is missing before verification for {}",
            launch.worker_id
        ));
    }
    let current_manifest_sha256 = manifest_sha256(&file_manifest(isolation_path)?)?;
    if current_manifest_sha256 != recorded_manifest_sha256 {
        return Err(anyhow!(
            "cleanup isolation manifest mismatch for {}: expected {}, found {}",
            launch.worker_id,
            recorded_manifest_sha256,
            current_manifest_sha256
        ));
    }
    Ok(())
}

fn swarm_cleanup(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let args = parse_swarm_execution_args(rest, "cleanup")?;
    let artifact_dir = ready_swarm_artifact_dir(context, &args.workflow_run_id, "cleanup")?;
    let integration_dir = artifact_dir.join("integrations").join(&args.dispatch_id);
    let cleanup_path = integration_dir.join("cleanup.json");
    if let Some(mut previous) = read_optional_json(&cleanup_path)? {
        previous["removed"] = json!(0);
        previous["reused"] = json!(true);
        previous["next_action"] = json!("review_or_archive_workflow");
        return format_swarm_cleanup_result(previous, args.json_output);
    }

    let integration = read_optional_json(&integration_dir.join("packet.json"))?
        .or(read_optional_json(&integration_dir.join("state.json"))?)
        .or(read_optional_json(&integration_dir.join("plan.json"))?)
        .ok_or_else(|| {
            anyhow!(
                "integration facts are required before cleanup for dispatch {}",
                args.dispatch_id
            )
        })?;
    let integration_status = integration["status"].as_str().unwrap_or("unknown");
    if !matches!(
        integration_status,
        "integrated" | "blocked" | "rolled_back" | "rollback_failed"
    ) {
        return Err(anyhow!(
            "integration status {integration_status} does not allow cleanup"
        ));
    }

    let execution_path = artifact_dir
        .join("workers")
        .join(&args.dispatch_id)
        .join("manifest.json");
    let execution: SwarmExecutionManifest = read_json_file(&execution_path)?;
    if execution.dispatch_id != args.dispatch_id {
        return Err(anyhow!("execution manifest identity mismatch"));
    }
    let project_root = cwd(context);
    let canonical_root = fs::canonicalize(&project_root).unwrap_or_else(|_| project_root.clone());
    let mut cleanup_candidates = Vec::new();
    for launch_relative in &execution.launch_paths {
        let launch: SwarmWorkerLaunch = read_json_file(&artifact_dir.join(launch_relative))?;
        let worker_dir = artifact_dir
            .join("workers")
            .join(&args.dispatch_id)
            .join(&launch.task_id);
        let state: Value = read_json_file(&worker_dir.join("state.json"))?;
        let status = state["status"].as_str().unwrap_or("unknown");
        if !is_terminal_worker_status(status) {
            return Err(anyhow!(
                "worker {} is not terminal: {status}",
                launch.worker_id
            ));
        }
        let expected = format!(
            ".kiana/swarm-worktrees/{}/{}",
            args.dispatch_id, launch.task_id
        );
        if launch.isolation_path != expected {
            return Err(anyhow!(
                "worker isolation path mismatch: expected {expected}, found {}",
                launch.isolation_path
            ));
        }
        let relative = safe_swarm_relative_path(Path::new(&launch.isolation_path))
            .ok_or_else(|| anyhow!("unsafe worker isolation path {}", launch.isolation_path))?;
        let isolation_path = project_root.join(relative);
        if !isolation_path.is_dir() {
            return Err(anyhow!(
                "worker isolation is missing before cleanup: {}",
                isolation_path.display()
            ));
        }
        let canonical = fs::canonicalize(&isolation_path)?;
        if !canonical.starts_with(&canonical_root) {
            return Err(anyhow!(
                "worker isolation path escaped project root: {}",
                isolation_path.display()
            ));
        }
        verify_swarm_cleanup_worker_facts(
            &artifact_dir,
            &execution,
            &launch,
            &state,
            &isolation_path,
        )?;
        cleanup_candidates.push((launch, isolation_path));
    }

    let mut removed = 0usize;
    let mut workers = Vec::new();
    for (launch, isolation_path) in cleanup_candidates {
        match launch.isolation_strategy.as_str() {
            "git_worktree" => remove_swarm_git_worktree(&project_root, &isolation_path)?,
            "snapshot_copy" => fs::remove_dir_all(&isolation_path)?,
            other => return Err(anyhow!("unsupported isolation strategy {other}")),
        }
        removed += 1;
        workers.push(json!({
            "worker_id": launch.worker_id,
            "task_id": launch.task_id,
            "isolation_strategy": launch.isolation_strategy,
            "isolation_path": launch.isolation_path,
            "status": "removed",
            "result_packet_preserved": true,
        }));
    }
    let cleanup_id = format!("cleanup-{}", args.dispatch_id);
    let result = json!({
        "schema": "kiana.swarm-cleanup-result.v1",
        "cleanup_id": cleanup_id,
        "workflow_id": execution.workflow_id,
        "run_id": execution.run_id,
        "dispatch_id": args.dispatch_id,
        "status": "cleaned",
        "integration_status": integration_status,
        "removed": removed,
        "workers": workers,
        "facts_preserved": true,
        "reused": false,
        "created_at_ms": now_ms(),
        "next_action": "review_or_archive_workflow",
    });
    write_json_atomic(&cleanup_path, &result)?;
    let cleanup_id_for_event = cleanup_id.clone();
    let dispatch_id_for_event = args.dispatch_id.clone();
    let _ = kiana_tasks::append_workflow_event_idempotent_with_unique_data_value(
        &artifact_dir,
        WorkflowEventKind::SwarmIsolationCleaned,
        "swarm_integration",
        "cleanup_id",
        cleanup_id,
        move |sequence, at_ms| {
            json!({
                "cleanup_id": cleanup_id_for_event,
                "dispatch_id": dispatch_id_for_event,
                "removed": removed,
                "facts_preserved": true,
                "sequence": sequence,
                "created_at_ms": at_ms,
            })
        },
    )?;
    format_swarm_cleanup_result(result, args.json_output)
}

fn remove_swarm_git_worktree(project_root: &Path, isolation_path: &Path) -> Result<()> {
    let output = ProcessCommand::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(isolation_path)
        .current_dir(project_root)
        .output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "failed to remove git worktree {}: {}",
            isolation_path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let _ = ProcessCommand::new("git")
        .args(["worktree", "prune"])
        .current_dir(project_root)
        .status();
    Ok(())
}

fn format_swarm_cleanup_result(value: Value, json_output: bool) -> Result<CommandResult> {
    if json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&value)?));
    }
    Ok(CommandResult::text(format!(
        "Bounded Swarm cleanup\ndispatch_id: {}\nstatus: {}\nremoved: {}\nfacts_preserved: {}",
        value["dispatch_id"].as_str().unwrap_or("unknown"),
        value["status"].as_str().unwrap_or("unknown"),
        value["removed"].as_u64().unwrap_or(0),
        value["facts_preserved"].as_bool().unwrap_or(false),
    )))
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct GitStateSnapshot {
    repository: bool,
    head: String,
    index_diff_sha256: String,
    worktree_diff_sha256: String,
    status_sha256: String,
}

fn git_state_snapshot(root: &Path) -> Result<GitStateSnapshot> {
    let repository = ProcessCommand::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(root)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !repository {
        let absent = sha256_bytes(b"not-a-git-repository");
        return Ok(GitStateSnapshot {
            repository: false,
            head: "none".to_string(),
            index_diff_sha256: absent.clone(),
            worktree_diff_sha256: absent.clone(),
            status_sha256: absent,
        });
    }

    let head = ProcessCommand::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "none".to_string());
    let exclusions = [
        ".",
        ":(exclude).kiana/**",
        ":(exclude)target/**",
        ":(exclude)node_modules/**",
        ":(exclude).venv/**",
        ":(exclude)dist/**",
        ":(exclude)build/**",
    ];
    let mut index_args = vec!["diff", "--cached", "--binary", "--no-ext-diff", "--"];
    index_args.extend(exclusions);
    let mut worktree_args = vec!["diff", "--binary", "--no-ext-diff", "--"];
    worktree_args.extend(exclusions);
    let mut status_args = vec![
        "status",
        "--porcelain=v2",
        "-z",
        "--untracked-files=all",
        "--",
    ];
    status_args.extend(exclusions);

    Ok(GitStateSnapshot {
        repository: true,
        head,
        index_diff_sha256: sha256_bytes(&git_output_bytes(root, &index_args)?),
        worktree_diff_sha256: sha256_bytes(&git_output_bytes(root, &worktree_args)?),
        status_sha256: release_status_sha256(root, &status_args, &exclusions)?,
    })
}

fn release_status_sha256(root: &Path, status_args: &[&str], exclusions: &[&str]) -> Result<String> {
    let status = git_output_bytes(root, status_args)?;
    let mut untracked_args = vec!["ls-files", "--others", "--exclude-standard", "-z", "--"];
    untracked_args.extend(exclusions.iter().copied());
    let untracked = git_output_bytes(root, &untracked_args)?;
    let mut paths = untracked
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    paths.sort();
    if paths.is_empty() {
        return Ok(sha256_bytes(&status));
    }

    fn update_field(digest: &mut Sha256, value: &[u8]) {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value);
    }

    let mut digest = Sha256::new();
    update_field(&mut digest, b"kiana.release-git-status.v2");
    update_field(&mut digest, &status);
    digest.update((paths.len() as u64).to_be_bytes());
    for raw_path in paths {
        #[cfg(unix)]
        let relative = PathBuf::from(OsString::from_vec(raw_path.clone()));
        #[cfg(not(unix))]
        let relative = PathBuf::from(
            String::from_utf8(raw_path.clone())
                .context("release workflow untracked path is not valid UTF-8")?,
        );
        let path = root.join(&relative);
        let metadata = fs::symlink_metadata(&path).with_context(|| {
            format!(
                "release workflow untracked file could not be read: {}",
                relative.display()
            )
        })?;
        let (mode, content): (&[u8], Vec<u8>) = if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path).with_context(|| {
                format!(
                    "release workflow untracked symlink could not be read: {}",
                    relative.display()
                )
            })?;
            #[cfg(unix)]
            let content = target.as_os_str().as_bytes().to_vec();
            #[cfg(not(unix))]
            let content = target.to_string_lossy().as_bytes().to_vec();
            (b"120000", content)
        } else if metadata.file_type().is_file() {
            #[cfg(unix)]
            let executable = metadata.permissions().mode() & 0o111 != 0;
            #[cfg(not(unix))]
            let executable = false;
            let mode = if executable { b"100755" } else { b"100644" };
            let content = fs::read(&path).with_context(|| {
                format!(
                    "release workflow untracked file could not be read: {}",
                    relative.display()
                )
            })?;
            (mode, content)
        } else {
            return Err(anyhow!(
                "release workflow untracked path is not a regular file or symlink: {}",
                relative.display()
            ));
        };
        update_field(&mut digest, &raw_path);
        update_field(&mut digest, mode);
        update_field(&mut digest, metadata.len().to_string().as_bytes());
        update_field(&mut digest, sha256_bytes(&content).as_bytes());
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn git_output_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = ProcessCommand::new("git")
        .args(args)
        .current_dir(root)
        .output()?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn working_tree_fingerprint(
    git_state: &GitStateSnapshot,
    manifest: &BTreeMap<String, String>,
) -> Result<String> {
    let payload = serde_json::to_vec(&(git_state, manifest))?;
    Ok(format!("sha256:{}", sha256_bytes(&payload)))
}

fn current_working_tree_fingerprint(root: &Path) -> Result<String> {
    let manifest = file_manifest(root)?;
    let git_state = git_state_snapshot(root)?;
    working_tree_fingerprint(&git_state, &manifest)
}

fn manifest_sha256(manifest: &BTreeMap<String, String>) -> Result<String> {
    Ok(format!(
        "sha256:{}",
        sha256_bytes(&serde_json::to_vec(manifest)?)
    ))
}

fn integration_plan_sha256(plan: &Value) -> Result<String> {
    let mut payload = plan.clone();
    if let Some(object) = payload.as_object_mut() {
        object.remove("plan_sha256");
    }
    let canonical = canonical_json_value(&payload);
    Ok(format!(
        "sha256:{}",
        sha256_bytes(&serde_json::to_vec(&canonical)?)
    ))
}

fn canonical_json_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            let mut canonical = serde_json::Map::new();
            for key in keys {
                canonical.insert(key.clone(), canonical_json_value(&object[key]));
            }
            Value::Object(canonical)
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_json_value).collect()),
        other => other.clone(),
    }
}

fn verify_integration_plan_authentication(artifact_dir: &Path, plan: &Value) -> Result<String> {
    let expected = plan["plan_sha256"]
        .as_str()
        .ok_or_else(|| anyhow!("integration plan hash is missing"))?;
    let actual = integration_plan_sha256(plan)?;
    if actual != expected {
        return Err(anyhow!(
            "integration plan hash mismatch: expected {expected}, found {actual}"
        ));
    }
    let integration_id = plan["integration_id"]
        .as_str()
        .ok_or_else(|| anyhow!("integration plan id is missing"))?;
    let event = read_workflow_events(artifact_dir)?
        .into_iter()
        .find(|event| {
            event.kind == WorkflowEventKind::SwarmIntegrationPlanned
                && event.data["integration_id"] == integration_id
        })
        .ok_or_else(|| anyhow!("integration plan event is missing"))?;
    if event.data["plan_sha256"].as_str() != Some(expected) {
        return Err(anyhow!("integration plan event hash mismatch"));
    }
    Ok(expected.to_string())
}

fn integration_completed_event_exists(artifact_dir: &Path, plan: &Value) -> Result<bool> {
    let integration_id = plan["integration_id"]
        .as_str()
        .ok_or_else(|| anyhow!("integration plan id is missing"))?;
    let integration_event_id = format!("{integration_id}:integrated");
    Ok(read_workflow_events(artifact_dir)?
        .into_iter()
        .any(|event| {
            event.kind == WorkflowEventKind::SwarmIntegrationCompleted
                && event.data["integration_event_id"].as_str()
                    == Some(integration_event_id.as_str())
        }))
}

fn integration_verification_event_exists(artifact_dir: &Path, plan: &Value) -> Result<bool> {
    let integration_id = plan["integration_id"]
        .as_str()
        .ok_or_else(|| anyhow!("integration plan id is missing"))?;
    let verification_event_id = format!("{integration_id}:verified");
    Ok(read_workflow_events(artifact_dir)?
        .into_iter()
        .any(|event| {
            event.kind == WorkflowEventKind::VerificationCompleted
                && event.data["verification_event_id"].as_str()
                    == Some(verification_event_id.as_str())
        }))
}

fn expected_integration_task_ids(plan: &Value) -> Result<Vec<String>> {
    plan["workers"]
        .as_array()
        .ok_or_else(|| anyhow!("integration plan workers are missing"))?
        .iter()
        .map(|worker| {
            worker["task_id"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| anyhow!("integration worker task id is missing"))
        })
        .collect()
}

fn commit_swarm_integration_verification(
    artifact_dir: &Path,
    plan: &Value,
    verification: &Value,
    checkpoint_path: &str,
    checkpoint_sha256: &str,
) -> Result<String> {
    build_completed_integration_packet(plan, verification)?;
    let verification_bytes = serde_json::to_vec_pretty(verification)?;
    let verification_sha256 = format!("sha256:{}", sha256_bytes(&verification_bytes));
    let integration_id = plan["integration_id"]
        .as_str()
        .ok_or_else(|| anyhow!("integration plan id is missing"))?
        .to_string();
    let verification_event_id = format!("{integration_id}:verified");
    let event_verification_event_id = verification_event_id.clone();
    let event_integration_id = plan["integration_id"].clone();
    let event_workflow_id = plan["workflow_id"].clone();
    let event_run_id = plan["run_id"].clone();
    let event_dispatch_id = plan["dispatch_id"].clone();
    let event_plan_sha256 = plan["plan_sha256"].clone();
    let event_verified_task_ids = verification["verified_task_ids"].clone();
    let event_verification_created_at_ms = verification["created_at_ms"].clone();
    let event_verification_sha256 = verification_sha256.clone();
    let event_checkpoint_path = checkpoint_path.to_string();
    let event_checkpoint_sha256 = checkpoint_sha256.to_string();
    let dispatch_id = plan["dispatch_id"].as_str().unwrap_or_default();
    let verification_relative = format!("integrations/{dispatch_id}/verification.json");
    let event_verification_relative = verification_relative.clone();
    kiana_tasks::commit_immutable_artifacts_with_unique_event(
        artifact_dir,
        WorkflowEventKind::VerificationCompleted,
        "swarm_integration",
        "verification_event_id",
        verification_event_id,
        move |sequence, created_at_ms| {
            Ok(WorkflowArtifactBatch {
                artifacts: vec![WorkflowArtifactInput {
                    relative_path: event_verification_relative.clone(),
                    contents: verification_bytes.clone(),
                }],
                event_data: json!({
                    "verification_event_id": event_verification_event_id,
                    "integration_id": event_integration_id,
                    "workflow_id": event_workflow_id,
                    "run_id": event_run_id,
                    "dispatch_id": event_dispatch_id,
                    "status": "pass",
                    "verification_path": event_verification_relative,
                    "verification_sha256": event_verification_sha256,
                    "plan_sha256": event_plan_sha256,
                    "checkpoint_path": event_checkpoint_path,
                    "checkpoint_sha256": event_checkpoint_sha256,
                    "verified_task_ids": event_verified_task_ids,
                    "verification_created_at_ms": event_verification_created_at_ms,
                    "sequence": sequence,
                    "created_at_ms": created_at_ms,
                }),
            })
        },
    )?;
    Ok(verification_sha256)
}

fn verify_swarm_integration_verification_authentication(
    artifact_dir: &Path,
    plan: &Value,
    verification: &Value,
) -> Result<String> {
    build_completed_integration_packet(plan, verification)?;
    let dispatch_id = plan["dispatch_id"]
        .as_str()
        .ok_or_else(|| anyhow!("integration dispatch id is missing"))?;
    let verification_relative = format!("integrations/{dispatch_id}/verification.json");
    let verification_path = artifact_dir.join(&verification_relative);
    let verification_bytes = fs::read(&verification_path)?;
    let persisted_verification: Value = serde_json::from_slice(&verification_bytes)?;
    if persisted_verification != *verification {
        return Err(anyhow!("integration verification artifact mismatch"));
    }
    let verification_sha256 = format!("sha256:{}", sha256_bytes(&verification_bytes));
    let integration_id = plan["integration_id"]
        .as_str()
        .ok_or_else(|| anyhow!("integration plan id is missing"))?;
    let verification_event_id = format!("{integration_id}:verified");
    let integration_event_id = format!("{integration_id}:applying");
    let events = read_workflow_events(artifact_dir)?;
    let event = events
        .iter()
        .find(|event| {
            event.kind == WorkflowEventKind::VerificationCompleted
                && event.data["verification_event_id"].as_str()
                    == Some(verification_event_id.as_str())
        })
        .ok_or_else(|| anyhow!("integration verification event is missing"))?;
    let started = events
        .iter()
        .find(|event| {
            event.kind == WorkflowEventKind::SwarmIntegrationStarted
                && event.data["integration_event_id"].as_str()
                    == Some(integration_event_id.as_str())
        })
        .ok_or_else(|| anyhow!("integration started event is missing"))?;
    if event.seq <= started.seq
        || event.data["integration_id"] != plan["integration_id"]
        || event.data["workflow_id"] != plan["workflow_id"]
        || event.data["run_id"] != plan["run_id"]
        || event.data["dispatch_id"] != plan["dispatch_id"]
        || event.data["status"] != "pass"
        || event.data["verification_path"].as_str() != Some(verification_relative.as_str())
        || event.data["verification_sha256"].as_str() != Some(verification_sha256.as_str())
        || event.data["plan_sha256"] != plan["plan_sha256"]
        || event.data["checkpoint_path"] != started.data["checkpoint_path"]
        || event.data["checkpoint_sha256"] != started.data["checkpoint_sha256"]
        || event.data["verified_task_ids"] != verification["verified_task_ids"]
        || event.data["verification_created_at_ms"] != verification["created_at_ms"]
    {
        return Err(anyhow!(
            "integration verification event authentication mismatch"
        ));
    }
    Ok(verification_sha256)
}

fn build_completed_integration_packet(plan: &Value, verification: &Value) -> Result<Value> {
    if verification["schema"] != "kiana.swarm-integration-verification.v1"
        || verification["integration_id"] != plan["integration_id"]
        || verification["dispatch_id"] != plan["dispatch_id"]
        || verification["status"] != "pass"
    {
        return Err(anyhow!("integration verification identity mismatch"));
    }
    let expected_task_ids = expected_integration_task_ids(plan)?;
    let verified_task_ids = verification["verified_task_ids"]
        .as_array()
        .ok_or_else(|| anyhow!("integration verification task ids are missing"))?
        .iter()
        .map(|task_id| {
            task_id
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| anyhow!("integration verification task id is invalid"))
        })
        .collect::<Result<Vec<_>>>()?;
    if verified_task_ids != expected_task_ids {
        return Err(anyhow!("integration verification task ids mismatch"));
    }
    let dispatch_id = plan["dispatch_id"]
        .as_str()
        .ok_or_else(|| anyhow!("integration dispatch id is missing"))?;
    let created_at_ms = verification["created_at_ms"]
        .as_u64()
        .ok_or_else(|| anyhow!("integration verification timestamp is missing"))?;
    Ok(json!({
        "schema": "kiana.swarm-integration-packet.v1",
        "integration_id": plan["integration_id"],
        "workflow_id": plan["workflow_id"],
        "run_id": plan["run_id"],
        "dispatch_id": plan["dispatch_id"],
        "status": "integrated",
        "plan_sha256": plan["plan_sha256"],
        "integrated_task_ids": verified_task_ids,
        "rejected_task_ids": [],
        "commands": verification["checks"],
        "verification_packet_path": format!("integrations/{dispatch_id}/verification.json"),
        "rollback": Value::Null,
        "automatic_commit": false,
        "automatic_push": false,
        "automatic_merge": false,
        "automatic_deploy": false,
        "created_at_ms": created_at_ms,
        "next_action": "run_swarm_cleanup_or_review",
    }))
}

fn commit_completed_integration_packet(
    artifact_dir: &Path,
    plan: &Value,
    verification: &Value,
) -> Result<Value> {
    let packet = build_completed_integration_packet(plan, verification)?;
    let verification_sha256 =
        verify_swarm_integration_verification_authentication(artifact_dir, plan, verification)?;
    let packet_bytes = serde_json::to_vec_pretty(&packet)?;
    let packet_sha256 = format!("sha256:{}", sha256_bytes(&packet_bytes));
    let integration_id = plan["integration_id"]
        .as_str()
        .ok_or_else(|| anyhow!("integration plan id is missing"))?
        .to_string();
    let integration_event_id = format!("{integration_id}:integrated");
    let event_integration_event_id = integration_event_id.clone();
    let event_integration_id = plan["integration_id"].clone();
    let event_workflow_id = plan["workflow_id"].clone();
    let event_run_id = plan["run_id"].clone();
    let event_dispatch_id = plan["dispatch_id"].clone();
    let event_plan_sha256 = plan["plan_sha256"].clone();
    let event_packet_sha256 = packet_sha256.clone();
    let event_packet_created_at_ms = packet["created_at_ms"].clone();
    let event_verification_sha256 = verification_sha256.clone();
    let dispatch_id = plan["dispatch_id"].as_str().unwrap_or_default();
    let packet_relative = format!("integrations/{dispatch_id}/packet.json");
    let event_packet_relative = packet_relative.clone();
    let verification_relative = format!("integrations/{dispatch_id}/verification.json");
    let event_verification_relative = verification_relative.clone();
    kiana_tasks::commit_immutable_artifacts_with_unique_event(
        artifact_dir,
        WorkflowEventKind::SwarmIntegrationCompleted,
        "swarm_integration",
        "integration_event_id",
        integration_event_id,
        move |sequence, created_at_ms| {
            Ok(WorkflowArtifactBatch {
                artifacts: vec![WorkflowArtifactInput {
                    relative_path: event_packet_relative.clone(),
                    contents: packet_bytes.clone(),
                }],
                event_data: json!({
                    "integration_event_id": event_integration_event_id,
                    "integration_id": event_integration_id,
                    "workflow_id": event_workflow_id,
                    "run_id": event_run_id,
                    "dispatch_id": event_dispatch_id,
                    "status": "integrated",
                    "packet_path": event_packet_relative,
                    "packet_sha256": event_packet_sha256,
                    "plan_sha256": event_plan_sha256,
                    "verification_path": event_verification_relative,
                    "verification_sha256": event_verification_sha256,
                    "packet_created_at_ms": event_packet_created_at_ms,
                    "sequence": sequence,
                    "created_at_ms": created_at_ms,
                }),
            })
        },
    )?;
    Ok(packet)
}

fn recover_orphan_completed_integration(
    artifact_dir: &Path,
    integration_dir: &Path,
    project_root: &Path,
    plan: &Value,
    packet_path: &Path,
    packet: Value,
    json_output: bool,
) -> Result<CommandResult> {
    let state_path = integration_dir.join("state.json");
    let state = read_optional_json(&state_path)?
        .ok_or_else(|| anyhow!("integration packet event is missing"))?;
    if !matches!(state["status"].as_str(), Some("applying" | "verifying"))
        || state["integration_id"] != plan["integration_id"]
        || state["dispatch_id"] != plan["dispatch_id"]
        || state["plan_sha256"] != plan["plan_sha256"]
    {
        return Err(anyhow!("integration packet event is missing"));
    }
    let checkpoint = verify_integration_checkpoint_authentication(
        artifact_dir,
        integration_dir,
        project_root,
        plan,
        &state,
    )?;
    let actual_fingerprint = current_working_tree_fingerprint(project_root)?;
    if actual_fingerprint != checkpoint.expected_fingerprint {
        return Err(anyhow!("integration orphan tree fingerprint mismatch"));
    }
    let dispatch_id = plan["dispatch_id"].as_str().unwrap_or_default();
    let verification_path = artifact_dir
        .join("integrations")
        .join(dispatch_id)
        .join("verification.json");
    let verification: Value = read_json_file(&verification_path)?;
    verify_swarm_integration_verification_authentication(artifact_dir, plan, &verification)?;
    let expected_packet = build_completed_integration_packet(plan, &verification)?;
    if packet != expected_packet {
        return Err(anyhow!(
            "integration orphan packet does not match verified completion"
        ));
    }
    let expected_task_ids = expected_integration_task_ids(plan)?;
    let applied_task_ids = state["applied_task_ids"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let verified_task_ids = state["verified_task_ids"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if applied_task_ids
        != json!(expected_task_ids)
            .as_array()
            .cloned()
            .unwrap_or_default()
        || verified_task_ids
            != json!(expected_task_ids)
                .as_array()
                .cloned()
                .unwrap_or_default()
    {
        return Err(anyhow!("integration orphan state task ids mismatch"));
    }
    let committed_packet = commit_completed_integration_packet(artifact_dir, plan, &verification)?;
    let packet_sha256 = verify_integration_packet_authentication(
        artifact_dir,
        packet_path,
        plan,
        &committed_packet,
    )?;
    let mut completed_state = completed_integration_state(plan, &committed_packet)?;
    completed_state["packet_sha256"] = json!(packet_sha256);
    write_json_atomic(&state_path, &completed_state)?;
    format_swarm_integration_result(committed_packet, json_output)
}

fn recover_verified_integration_before_packet(
    artifact_dir: &Path,
    integration_dir: &Path,
    project_root: &Path,
    plan: &Value,
    state: Value,
    json_output: bool,
) -> Result<CommandResult> {
    if !matches!(state["status"].as_str(), Some("applying" | "verifying"))
        || state["integration_id"] != plan["integration_id"]
        || state["dispatch_id"] != plan["dispatch_id"]
        || state["plan_sha256"] != plan["plan_sha256"]
    {
        return Err(anyhow!("integration verified recovery state mismatch"));
    }
    let checkpoint = verify_integration_checkpoint_authentication(
        artifact_dir,
        integration_dir,
        project_root,
        plan,
        &state,
    )?;
    let actual_fingerprint = current_working_tree_fingerprint(project_root)?;
    if actual_fingerprint != checkpoint.expected_fingerprint {
        return Err(anyhow!(
            "integration verified recovery tree fingerprint mismatch"
        ));
    }
    let dispatch_id = plan["dispatch_id"].as_str().unwrap_or_default();
    let verification_path = integration_dir.join("verification.json");
    let verification: Value = read_json_file(&verification_path)?;
    verify_swarm_integration_verification_authentication(artifact_dir, plan, &verification)?;
    let committed_packet = commit_completed_integration_packet(artifact_dir, plan, &verification)?;
    let packet_path = integration_dir.join("packet.json");
    let packet_sha256 = verify_integration_packet_authentication(
        artifact_dir,
        &packet_path,
        plan,
        &committed_packet,
    )?;
    let mut completed_state = completed_integration_state(plan, &committed_packet)?;
    completed_state["packet_sha256"] = json!(packet_sha256);
    write_json_atomic(&integration_dir.join("state.json"), &completed_state)?;
    debug_assert_eq!(
        committed_packet["verification_packet_path"],
        format!("integrations/{dispatch_id}/verification.json")
    );
    format_swarm_integration_result(committed_packet, json_output)
}

fn verify_integration_packet_authentication(
    artifact_dir: &Path,
    packet_path: &Path,
    plan: &Value,
    packet: &Value,
) -> Result<String> {
    if packet["schema"] != "kiana.swarm-integration-packet.v1"
        || packet["status"] != "integrated"
        || packet["integration_id"] != plan["integration_id"]
        || packet["workflow_id"] != plan["workflow_id"]
        || packet["run_id"] != plan["run_id"]
        || packet["dispatch_id"] != plan["dispatch_id"]
    {
        return Err(anyhow!("integration packet identity mismatch"));
    }
    let plan_sha256 = plan["plan_sha256"]
        .as_str()
        .ok_or_else(|| anyhow!("integration plan hash is missing"))?;
    if packet["plan_sha256"].as_str() != Some(plan_sha256) {
        return Err(anyhow!("integration packet plan hash mismatch"));
    }
    let dispatch_id = plan["dispatch_id"]
        .as_str()
        .ok_or_else(|| anyhow!("integration dispatch id is missing"))?;
    let verification_relative = format!("integrations/{dispatch_id}/verification.json");
    if packet["verification_packet_path"].as_str() != Some(verification_relative.as_str()) {
        return Err(anyhow!("integration packet verification path mismatch"));
    }
    let verification: Value = read_json_file(&artifact_dir.join(&verification_relative))?;
    let verification_sha256 =
        verify_swarm_integration_verification_authentication(artifact_dir, plan, &verification)?;
    let packet_bytes = fs::read(packet_path)?;
    let packet_sha256 = format!("sha256:{}", sha256_bytes(&packet_bytes));
    let integration_id = plan["integration_id"]
        .as_str()
        .ok_or_else(|| anyhow!("integration plan id is missing"))?;
    let integration_event_id = format!("{integration_id}:integrated");
    let event = read_workflow_events(artifact_dir)?
        .into_iter()
        .find(|event| {
            event.kind == WorkflowEventKind::SwarmIntegrationCompleted
                && event.data["integration_event_id"].as_str()
                    == Some(integration_event_id.as_str())
        })
        .ok_or_else(|| anyhow!("integration packet event is missing"))?;
    let expected_packet_path = format!(
        "integrations/{}/packet.json",
        plan["dispatch_id"].as_str().unwrap_or_default()
    );
    if event.data["integration_id"] != plan["integration_id"]
        || event.data["workflow_id"] != plan["workflow_id"]
        || event.data["run_id"] != plan["run_id"]
        || event.data["dispatch_id"] != plan["dispatch_id"]
        || event.data["status"] != "integrated"
        || event.data["packet_path"].as_str() != Some(expected_packet_path.as_str())
        || event.data["packet_sha256"].as_str() != Some(packet_sha256.as_str())
        || event.data["plan_sha256"].as_str() != Some(plan_sha256)
        || event.data["verification_path"].as_str() != Some(verification_relative.as_str())
        || event.data["verification_sha256"].as_str() != Some(verification_sha256.as_str())
        || event.data["packet_created_at_ms"] != packet["created_at_ms"]
    {
        return Err(anyhow!("integration packet event authentication mismatch"));
    }
    Ok(packet_sha256)
}

fn completed_integration_state(plan: &Value, packet: &Value) -> Result<Value> {
    let integrated_task_ids = packet["integrated_task_ids"]
        .as_array()
        .ok_or_else(|| anyhow!("integration packet task ids are missing"))?
        .iter()
        .map(|task_id| {
            task_id
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| anyhow!("integration packet task id is invalid"))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(integration_state_value(
        plan,
        "integrated",
        integrated_task_ids.clone(),
        integrated_task_ids,
        None,
        None,
        "run_swarm_cleanup_or_review",
    ))
}

fn sha256_text(value: &str) -> String {
    sha256_bytes(value.as_bytes())
}

fn sha256_bytes(value: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(value);
    format!("{:x}", digest.finalize())
}

fn read_regular_file_nofollow(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect regular file {}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(anyhow!(
            "regular file must not be a symbolic link: {}",
            path.display()
        ));
    }
    if !metadata.is_file() {
        return Err(anyhow!("expected regular file: {}", path.display()));
    }

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = options
        .open(path)
        .with_context(|| format!("failed to open regular file {}", path.display()))?;
    if !file.metadata()?.is_file() {
        return Err(anyhow!("expected regular file: {}", path.display()));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn git_apply_check_bytes(root: &Path, patch_bytes: &[u8]) -> Result<()> {
    git_apply_bytes(root, &["apply", "--check", "--binary"], patch_bytes)
}

fn git_apply_patch_bytes(root: &Path, patch_bytes: &[u8]) -> Result<()> {
    git_apply_bytes(root, &["apply", "--binary"], patch_bytes)
}

fn git_apply_bytes(root: &Path, args: &[&str], patch_bytes: &[u8]) -> Result<()> {
    let mut child = ProcessCommand::new("git")
        .args(args)
        .arg("-")
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let write_result = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("git apply stdin was not available"))?
        .write_all(patch_bytes);
    let output = child.wait_with_output()?;
    if output.status.success() {
        write_result.map_err(Into::into)
    } else {
        Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn verify_integration_worker_artifacts(artifact_dir: &Path, worker: &Value) -> Result<Vec<u8>> {
    if worker["status"] != "ready" {
        return Err(anyhow!(
            "worker {} is not ready for integration",
            worker["task_id"].as_str().unwrap_or("unknown")
        ));
    }
    let result_relative = worker["result_packet_path"]
        .as_str()
        .ok_or_else(|| anyhow!("integration worker result_packet_path is missing"))?;
    let result_safe = safe_swarm_relative_path(Path::new(result_relative))
        .ok_or_else(|| anyhow!("unsafe integration artifact path {result_relative}"))?;
    let result_actual = format!("sha256:{}", sha256_file(&artifact_dir.join(result_safe))?);
    let result_expected = worker["result_packet_sha256"]
        .as_str()
        .ok_or_else(|| anyhow!("integration worker result_packet_sha256 is missing"))?;
    if result_actual != result_expected {
        return Err(anyhow!(
            "integration artifact changed for {}: expected {}, found {}",
            result_relative,
            result_expected,
            result_actual
        ));
    }

    let patch_relative = worker["patch_path"]
        .as_str()
        .ok_or_else(|| anyhow!("integration worker patch_path is missing"))?;
    let patch_safe = safe_swarm_relative_path(Path::new(patch_relative))
        .ok_or_else(|| anyhow!("unsafe integration artifact path {patch_relative}"))?;
    let patch_bytes = read_regular_file_nofollow(&artifact_dir.join(patch_safe))?;
    let patch_actual = format!("sha256:{}", sha256_bytes(&patch_bytes));
    let patch_expected = worker["patch_sha256"]
        .as_str()
        .ok_or_else(|| anyhow!("integration worker patch_sha256 is missing"))?;
    if patch_actual != patch_expected {
        return Err(anyhow!(
            "integration artifact changed for {}: expected {}, found {}",
            patch_relative,
            patch_expected,
            patch_actual
        ));
    }
    Ok(patch_bytes)
}

fn create_integration_checkpoint(
    artifact_dir: &Path,
    root: &Path,
    integration_dir: &Path,
    changed_paths: &[String],
    plan: &Value,
) -> Result<(String, String)> {
    archive_completed_integration_recovery_journal(artifact_dir, integration_dir)?;
    let checkpoint_dir = integration_dir.join("checkpoint");
    let files_dir = checkpoint_dir.join("files");
    fs::create_dir_all(&files_dir)?;
    let mut files = Vec::new();
    for path in changed_paths {
        let relative = safe_swarm_relative_path(Path::new(path))
            .ok_or_else(|| anyhow!("unsafe checkpoint path {path}"))?;
        let source = root.join(&relative);
        let metadata = fs::symlink_metadata(&source).ok();
        let kind = metadata.as_ref().map(|metadata| {
            if metadata.file_type().is_symlink() {
                "symlink"
            } else if metadata.is_file() {
                "file"
            } else {
                "unsupported"
            }
        });
        let mut entry = json!({
            "path": path,
            "existed": metadata.is_some(),
            "kind": kind,
            "backup_path": Value::Null,
            "backup_sha256": Value::Null,
            "backup_mode": Value::Null,
            "symlink_target": Value::Null,
            "metadata": Value::Null,
        });
        match kind {
            Some("file") => {
                entry["metadata"] = serde_json::to_value(checkpoint_metadata_at_path(&source)?)?;
                let backup = files_dir.join(&relative);
                if let Some(parent) = backup.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&source, &backup)?;
                entry["backup_path"] = json!(backup
                    .strip_prefix(integration_dir.parent().unwrap_or(integration_dir))
                    .unwrap_or(&backup)
                    .to_string_lossy()
                    .replace('\\', "/"));
                entry["backup_sha256"] = json!(format!("sha256:{}", sha256_file(&backup)?));
                entry["backup_mode"] = json!(file_mode_fingerprint(&backup)?);
            }
            Some("symlink") => {
                entry["metadata"] = serde_json::to_value(checkpoint_metadata_at_path(&source)?)?;
                entry["symlink_target"] =
                    json!(fs::read_link(&source)?.to_string_lossy().to_string());
            }
            Some("unsupported") => {
                return Err(anyhow!("unsupported checkpoint path type: {path}"));
            }
            _ => {}
        }
        files.push(entry);
    }
    let manifest = json!({
        "schema": "kiana.swarm-integration-checkpoint.v2",
        "integration_id": plan["integration_id"],
        "dispatch_id": plan["dispatch_id"],
        "plan_sha256": plan["plan_sha256"],
        "baseline_fingerprint": plan["baseline"]["working_tree_fingerprint"],
        "files": files,
        "created_at_ms": now_ms(),
    });
    let manifest_path = checkpoint_dir.join("manifest.json");
    write_json_atomic(&manifest_path, &manifest)?;
    Ok((
        "checkpoint/manifest.json".to_string(),
        format!("sha256:{}", sha256_file(&manifest_path)?),
    ))
}

enum VerifiedCheckpointContent {
    Missing,
    File {
        contents: Vec<u8>,
        permissions: fs::Permissions,
        metadata: CheckpointMetadata,
    },
    Symlink {
        target: PathBuf,
        metadata: CheckpointMetadata,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct CheckpointXattr {
    name_hex: String,
    value_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct CheckpointMetadata {
    uid: u32,
    gid: u32,
    xattrs: Vec<CheckpointXattr>,
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn hex_to_bytes(value: &str) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return Err(anyhow!("checkpoint xattr hex length is invalid"));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char)
                .to_digit(16)
                .ok_or_else(|| anyhow!("checkpoint xattr hex is invalid"))?;
            let low = (pair[1] as char)
                .to_digit(16)
                .ok_or_else(|| anyhow!("checkpoint xattr hex is invalid"))?;
            Ok(((high << 4) | low) as u8)
        })
        .collect()
}

fn checkpoint_xattrs_fingerprint(xattrs: &[CheckpointXattr]) -> String {
    let mut payload = Vec::new();
    for xattr in xattrs {
        payload.extend_from_slice(xattr.name_hex.as_bytes());
        payload.push(0);
        payload.extend_from_slice(xattr.value_hex.as_bytes());
        payload.push(0xff);
    }
    sha256_bytes(&payload)
}

fn checkpoint_metadata_descriptor(metadata: &CheckpointMetadata) -> String {
    format!(
        "uid={}:gid={}:xattrs_sha256={}",
        metadata.uid,
        metadata.gid,
        checkpoint_xattrs_fingerprint(&metadata.xattrs)
    )
}

fn checkpoint_metadata_at_path(path: &Path) -> Result<CheckpointMetadata> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::symlink_metadata(path)?;
        return Ok(CheckpointMetadata {
            uid: metadata.uid(),
            gid: metadata.gid(),
            xattrs: linux_list_xattrs_path(path)?,
        });
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Ok(CheckpointMetadata {
            uid: 0,
            gid: 0,
            xattrs: Vec::new(),
        })
    }
}

#[cfg(target_os = "linux")]
fn linux_list_xattrs_path(path: &Path) -> Result<Vec<CheckpointXattr>> {
    let path = linux_cstring(path)?;
    linux_list_xattrs_cpath(&path)
}

#[cfg(target_os = "linux")]
fn linux_list_xattrs_cpath(path: &CString) -> Result<Vec<CheckpointXattr>> {
    let length = unsafe { libc::llistxattr(path.as_ptr(), std::ptr::null_mut(), 0) };
    if length < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENOTSUP) {
            return Ok(Vec::new());
        }
        return Err(error.into());
    }
    if length == 0 {
        return Ok(Vec::new());
    }
    let mut names = vec![0u8; length as usize];
    let read = unsafe { libc::llistxattr(path.as_ptr(), names.as_mut_ptr().cast(), names.len()) };
    if read < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    names.truncate(read as usize);
    let mut xattrs = Vec::new();
    for name in names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let name_c = CString::new(name)?;
        let value_length =
            unsafe { libc::lgetxattr(path.as_ptr(), name_c.as_ptr(), std::ptr::null_mut(), 0) };
        if value_length < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut value = vec![0u8; value_length as usize];
        if value_length > 0 {
            let value_read = unsafe {
                libc::lgetxattr(
                    path.as_ptr(),
                    name_c.as_ptr(),
                    value.as_mut_ptr().cast(),
                    value.len(),
                )
            };
            if value_read < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            value.truncate(value_read as usize);
        }
        xattrs.push(CheckpointXattr {
            name_hex: bytes_to_hex(name),
            value_hex: bytes_to_hex(&value),
        });
    }
    xattrs.sort_by(|left, right| left.name_hex.cmp(&right.name_hex));
    Ok(xattrs)
}

#[cfg(target_os = "linux")]
fn linux_list_xattrs_fd(fd: RawFd) -> Result<Vec<CheckpointXattr>> {
    let length = unsafe { libc::flistxattr(fd, std::ptr::null_mut(), 0) };
    if length < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENOTSUP) {
            return Ok(Vec::new());
        }
        return Err(error.into());
    }
    if length == 0 {
        return Ok(Vec::new());
    }
    let mut names = vec![0u8; length as usize];
    let read = unsafe { libc::flistxattr(fd, names.as_mut_ptr().cast(), names.len()) };
    if read < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    names.truncate(read as usize);
    let mut xattrs = Vec::new();
    for name in names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let name_c = CString::new(name)?;
        let value_length = unsafe { libc::fgetxattr(fd, name_c.as_ptr(), std::ptr::null_mut(), 0) };
        if value_length < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut value = vec![0u8; value_length as usize];
        if value_length > 0 {
            let value_read = unsafe {
                libc::fgetxattr(fd, name_c.as_ptr(), value.as_mut_ptr().cast(), value.len())
            };
            if value_read < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            value.truncate(value_read as usize);
        }
        xattrs.push(CheckpointXattr {
            name_hex: bytes_to_hex(name),
            value_hex: bytes_to_hex(&value),
        });
    }
    xattrs.sort_by(|left, right| left.name_hex.cmp(&right.name_hex));
    Ok(xattrs)
}

#[cfg(target_os = "linux")]
fn linux_procfd_child_path(parent_fd: RawFd, name: &CString) -> Result<CString> {
    let mut path = format!("/proc/self/fd/{parent_fd}/").into_bytes();
    path.extend_from_slice(name.as_bytes());
    CString::new(path).map_err(|_| anyhow!("secure recovery procfd path contains NUL"))
}

#[cfg(target_os = "linux")]
fn linux_checkpoint_metadata_at(parent_fd: RawFd, name: &CString) -> Result<CheckpointMetadata> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            parent_fd,
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    let stat = unsafe { stat.assume_init() };
    let path = linux_procfd_child_path(parent_fd, name)?;
    Ok(CheckpointMetadata {
        uid: stat.st_uid,
        gid: stat.st_gid,
        xattrs: linux_list_xattrs_cpath(&path)?,
    })
}

#[cfg(target_os = "linux")]
fn linux_replace_xattrs_fd(fd: RawFd, metadata: &CheckpointMetadata) -> Result<()> {
    let desired = metadata
        .xattrs
        .iter()
        .map(|xattr| {
            Ok((
                hex_to_bytes(&xattr.name_hex)?,
                hex_to_bytes(&xattr.value_hex)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    for current in linux_list_xattrs_fd(fd)? {
        let name = hex_to_bytes(&current.name_hex)?;
        if !desired.contains_key(&name) {
            let name = CString::new(name)?;
            if unsafe { libc::fremovexattr(fd, name.as_ptr()) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
    }
    for (name, value) in desired {
        let name = CString::new(name)?;
        if unsafe { libc::fsetxattr(fd, name.as_ptr(), value.as_ptr().cast(), value.len(), 0) } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_replace_xattrs_symlink(
    parent_fd: RawFd,
    name: &CString,
    metadata: &CheckpointMetadata,
) -> Result<()> {
    let path = linux_procfd_child_path(parent_fd, name)?;
    let desired = metadata
        .xattrs
        .iter()
        .map(|xattr| {
            Ok((
                hex_to_bytes(&xattr.name_hex)?,
                hex_to_bytes(&xattr.value_hex)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    for current in linux_list_xattrs_cpath(&path)? {
        let name_bytes = hex_to_bytes(&current.name_hex)?;
        if !desired.contains_key(&name_bytes) {
            let xattr_name = CString::new(name_bytes)?;
            if unsafe { libc::lremovexattr(path.as_ptr(), xattr_name.as_ptr()) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
    }
    for (name_bytes, value) in desired {
        let xattr_name = CString::new(name_bytes)?;
        if unsafe {
            libc::lsetxattr(
                path.as_ptr(),
                xattr_name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_apply_file_metadata(fd: RawFd, mode: u32, metadata: &CheckpointMetadata) -> Result<()> {
    if unsafe { libc::fchown(fd, metadata.uid, metadata.gid) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if unsafe { libc::fchmod(fd, mode as libc::mode_t) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    linux_replace_xattrs_fd(fd, metadata)
}

#[cfg(target_os = "linux")]
fn linux_apply_symlink_metadata(
    parent_fd: RawFd,
    name: &CString,
    metadata: &CheckpointMetadata,
) -> Result<()> {
    if unsafe {
        libc::fchownat(
            parent_fd,
            name.as_ptr(),
            metadata.uid,
            metadata.gid,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    linux_replace_xattrs_symlink(parent_fd, name, metadata)
}

struct VerifiedCheckpointEntry {
    relative: PathBuf,
    expected_current: String,
    content: VerifiedCheckpointContent,
}

struct VerifiedIntegrationCheckpoint {
    expected_fingerprint: String,
    entries: Vec<VerifiedCheckpointEntry>,
}

const INTEGRATION_RECOVERY_JOURNAL_SCHEMA: &str = "kiana.swarm-integration-recovery-journal.v2";
const INTEGRATION_RECOVERY_JOURNAL_DOMAIN: &str = "swarm-integration-recovery-journal";
const INTEGRATION_RECOVERY_JOURNAL_GENESIS_SHA256: &str =
    "sha256:0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum IntegrationRecoveryOperation {
    Remove,
    InstallFile,
    InstallSymlink,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum IntegrationRecoveryEntryStatus {
    Pending,
    Restoring,
    Completed,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct IntegrationRecoveryJournalEntry {
    path: String,
    source_descriptor: String,
    restored_descriptor: String,
    operation: IntegrationRecoveryOperation,
    auxiliary_name: String,
    status: IntegrationRecoveryEntryStatus,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct IntegrationRecoveryJournal {
    schema: String,
    revision: u64,
    integration_id: String,
    dispatch_id: String,
    plan_sha256: String,
    checkpoint_path: String,
    checkpoint_sha256: String,
    expected_pre_restore_fingerprint: String,
    expected_post_restore_fingerprint: String,
    source_manifest_sha256: String,
    source_git_repository: bool,
    source_git_head: String,
    source_git_index_diff_sha256: String,
    status: String,
    next_entry_index: usize,
    entries: Vec<IntegrationRecoveryJournalEntry>,
    last_error: Option<String>,
    created_at_ms: u64,
    updated_at_ms: u64,
    #[serde(default)]
    integrity: Option<IntegrityEnvelope>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct SwarmRecoveryIntegrityReport {
    schema: String,
    status: String,
    active_journal_count: u64,
    verified_active_journal_count: u64,
    archived_journal_count: u64,
    verified_archived_journal_count: u64,
    recoverable_unanchored_tail_count: u64,
    legacy_unsigned_count: u64,
    mismatch_count: u64,
    key_id: Option<String>,
}

fn integration_recovery_journal_path(integration_dir: &Path) -> PathBuf {
    integration_dir.join("recovery").join("journal.json")
}

fn archive_completed_integration_recovery_journal(
    artifact_dir: &Path,
    integration_dir: &Path,
) -> Result<()> {
    let journal_path = integration_recovery_journal_path(integration_dir);
    if !journal_path.exists() {
        return Ok(());
    }
    let journal = authenticated_integration_recovery_journal(artifact_dir, &journal_path)?;
    if journal.status != "completed" {
        return Err(anyhow!(
            "integration recovery journal is not complete; resume recovery before creating a new checkpoint"
        ));
    }
    let integration_relative = integration_dir
        .strip_prefix(artifact_dir)
        .map_err(|_| anyhow!("integration directory is outside workflow artifacts"))?
        .to_string_lossy()
        .replace('\\', "/");
    let archive_relative = format!(
        "{integration_relative}/recovery/history/journal-{}-{}-r{}.json",
        journal.created_at_ms,
        journal.checkpoint_sha256.trim_start_matches("sha256:"),
        journal.revision,
    );
    let archive_id = format!(
        "{}:{}:{}",
        journal.integration_id, journal.checkpoint_sha256, journal.revision
    );
    let event_archive_id = archive_id.clone();
    let event_integration_id = journal.integration_id.clone();
    let event_dispatch_id = journal.dispatch_id.clone();
    let event_checkpoint_sha256 = journal.checkpoint_sha256.clone();
    let event_archive_relative = archive_relative.clone();
    let event_record_sha256 = integration_recovery_journal_record_sha256(&journal)?;
    let event_binding_sha256 = integration_recovery_journal_binding_sha256(&journal)?;
    let event_revision = journal.revision;
    let archive_bytes = serde_json::to_vec_pretty(&journal)?;
    kiana_tasks::commit_immutable_artifacts_with_unique_event(
        artifact_dir,
        WorkflowEventKind::SwarmIntegrationRecoveryArchived,
        "swarm_integration",
        "recovery_archive_id",
        archive_id,
        move |sequence, created_at_ms| {
            Ok(WorkflowArtifactBatch {
                artifacts: vec![WorkflowArtifactInput {
                    relative_path: event_archive_relative.clone(),
                    contents: archive_bytes.clone(),
                }],
                event_data: json!({
                    "recovery_archive_id": event_archive_id,
                    "integration_id": event_integration_id,
                    "dispatch_id": event_dispatch_id,
                    "checkpoint_sha256": event_checkpoint_sha256,
                    "journal_path": format!("{integration_relative}/recovery/journal.json"),
                    "archive_path": event_archive_relative,
                    "revision": event_revision,
                    "record_sha256": event_record_sha256,
                    "journal_binding_sha256": event_binding_sha256,
                    "sequence": sequence,
                    "created_at_ms": created_at_ms,
                }),
            })
        },
    )?;
    fs::remove_file(&journal_path)?;
    #[cfg(unix)]
    {
        fs::File::open(integration_dir.join("recovery"))?.sync_all()?;
    }
    Ok(())
}

fn verified_checkpoint_content_descriptor(content: &VerifiedCheckpointContent) -> String {
    match content {
        VerifiedCheckpointContent::Missing => "missing".to_string(),
        VerifiedCheckpointContent::File {
            contents,
            permissions,
            metadata,
        } => {
            #[cfg(unix)]
            let mode = format!("{:04o}", permissions.mode() & 0o7777);
            #[cfg(not(unix))]
            let mode = if permissions.readonly() {
                "readonly".to_string()
            } else {
                "writable".to_string()
            };
            format!(
                "file:mode={mode}:{}:sha256:{}",
                checkpoint_metadata_descriptor(metadata),
                sha256_bytes(contents)
            )
        }
        VerifiedCheckpointContent::Symlink { target, metadata } => format!(
            "symlink:{}:target_sha256:{}",
            checkpoint_metadata_descriptor(metadata),
            sha256_bytes(target.to_string_lossy().as_bytes())
        ),
    }
}

fn integration_recovery_auxiliary_name(journal_id: &str, index: usize) -> String {
    format!(".kiana-recovery-journal-{journal_id}-{index}")
}

fn integration_recovery_auxiliary_path(entry: &IntegrationRecoveryJournalEntry) -> Result<PathBuf> {
    if !entry.auxiliary_name.starts_with(".kiana-recovery-journal-")
        || entry.auxiliary_name.contains('/')
        || entry.auxiliary_name.contains('\\')
    {
        return Err(anyhow!(
            "integration recovery journal auxiliary name is invalid"
        ));
    }
    let relative = safe_swarm_relative_path(Path::new(&entry.path))
        .ok_or_else(|| anyhow!("integration recovery journal path is unsafe"))?;
    Ok(relative
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(&entry.auxiliary_name))
}

fn create_integration_recovery_journal(
    root: &Path,
    plan: &Value,
    state: &Value,
    checkpoint: &VerifiedIntegrationCheckpoint,
) -> Result<IntegrationRecoveryJournal> {
    let manifest = file_manifest(root)?;
    let git_state = git_state_snapshot(root)?;
    let fingerprint = working_tree_fingerprint(&git_state, &manifest)?;
    if fingerprint != checkpoint.expected_fingerprint {
        return Err(anyhow!(
            "integration recovery journal source fingerprint mismatch"
        ));
    }
    let integration_id = plan["integration_id"]
        .as_str()
        .ok_or_else(|| anyhow!("integration recovery journal integration id is missing"))?;
    let dispatch_id = plan["dispatch_id"]
        .as_str()
        .ok_or_else(|| anyhow!("integration recovery journal dispatch id is missing"))?;
    let plan_sha256 = plan["plan_sha256"]
        .as_str()
        .ok_or_else(|| anyhow!("integration recovery journal plan hash is missing"))?;
    let checkpoint_path = state["checkpoint_path"]
        .as_str()
        .ok_or_else(|| anyhow!("integration recovery journal checkpoint path is missing"))?;
    let checkpoint_sha256 = state["checkpoint_sha256"]
        .as_str()
        .ok_or_else(|| anyhow!("integration recovery journal checkpoint hash is missing"))?;
    let journal_id = sha256_text(&format!(
        "{integration_id}:{dispatch_id}:{plan_sha256}:{checkpoint_sha256}"
    ));
    let journal_id = &journal_id[..20];
    let entries = checkpoint
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| IntegrationRecoveryJournalEntry {
            path: entry.relative.to_string_lossy().replace('\\', "/"),
            source_descriptor: entry.expected_current.clone(),
            restored_descriptor: verified_checkpoint_content_descriptor(&entry.content),
            operation: match entry.content {
                VerifiedCheckpointContent::Missing => IntegrationRecoveryOperation::Remove,
                VerifiedCheckpointContent::File { .. } => IntegrationRecoveryOperation::InstallFile,
                VerifiedCheckpointContent::Symlink { .. } => {
                    IntegrationRecoveryOperation::InstallSymlink
                }
            },
            auxiliary_name: integration_recovery_auxiliary_name(journal_id, index),
            status: IntegrationRecoveryEntryStatus::Pending,
        })
        .collect();
    let created_at_ms = now_ms();
    Ok(IntegrationRecoveryJournal {
        schema: INTEGRATION_RECOVERY_JOURNAL_SCHEMA.to_string(),
        revision: 0,
        integration_id: integration_id.to_string(),
        dispatch_id: dispatch_id.to_string(),
        plan_sha256: plan_sha256.to_string(),
        checkpoint_path: checkpoint_path.to_string(),
        checkpoint_sha256: checkpoint_sha256.to_string(),
        expected_pre_restore_fingerprint: checkpoint.expected_fingerprint.clone(),
        expected_post_restore_fingerprint: plan["baseline"]["working_tree_fingerprint"]
            .as_str()
            .ok_or_else(|| anyhow!("integration recovery baseline fingerprint is missing"))?
            .to_string(),
        source_manifest_sha256: manifest_sha256(&manifest)?,
        source_git_repository: git_state.repository,
        source_git_head: git_state.head,
        source_git_index_diff_sha256: git_state.index_diff_sha256,
        status: "prepared".to_string(),
        next_entry_index: 0,
        entries,
        last_error: None,
        created_at_ms,
        updated_at_ms: created_at_ms,
        integrity: None,
    })
}

fn validate_integration_recovery_journal_binding(
    journal: &IntegrationRecoveryJournal,
    plan: &Value,
    state: &Value,
    checkpoint: &VerifiedIntegrationCheckpoint,
) -> Result<()> {
    if journal.schema != INTEGRATION_RECOVERY_JOURNAL_SCHEMA
        || journal.integration_id != plan["integration_id"].as_str().unwrap_or_default()
        || journal.dispatch_id != plan["dispatch_id"].as_str().unwrap_or_default()
        || journal.plan_sha256 != plan["plan_sha256"].as_str().unwrap_or_default()
        || journal.checkpoint_path != state["checkpoint_path"].as_str().unwrap_or_default()
        || journal.checkpoint_sha256 != state["checkpoint_sha256"].as_str().unwrap_or_default()
        || journal.expected_pre_restore_fingerprint != checkpoint.expected_fingerprint
        || journal.expected_post_restore_fingerprint
            != plan["baseline"]["working_tree_fingerprint"]
                .as_str()
                .unwrap_or_default()
        || journal.entries.len() != checkpoint.entries.len()
    {
        return Err(anyhow!("integration recovery journal identity mismatch"));
    }
    for (index, (journal_entry, checkpoint_entry)) in
        journal.entries.iter().zip(&checkpoint.entries).enumerate()
    {
        let path = checkpoint_entry
            .relative
            .to_string_lossy()
            .replace('\\', "/");
        let expected_operation = match checkpoint_entry.content {
            VerifiedCheckpointContent::Missing => IntegrationRecoveryOperation::Remove,
            VerifiedCheckpointContent::File { .. } => IntegrationRecoveryOperation::InstallFile,
            VerifiedCheckpointContent::Symlink { .. } => {
                IntegrationRecoveryOperation::InstallSymlink
            }
        };
        if journal_entry.path != path
            || journal_entry.source_descriptor != checkpoint_entry.expected_current
            || journal_entry.restored_descriptor
                != verified_checkpoint_content_descriptor(&checkpoint_entry.content)
            || journal_entry.operation != expected_operation
            || journal_entry.auxiliary_name
                != integration_recovery_auxiliary_name(
                    &sha256_text(&format!(
                        "{}:{}:{}:{}",
                        journal.integration_id,
                        journal.dispatch_id,
                        journal.plan_sha256,
                        journal.checkpoint_sha256
                    ))[..20],
                    index,
                )
        {
            return Err(anyhow!("integration recovery journal entry mismatch"));
        }
    }
    Ok(())
}

fn validate_integration_recovery_journal_tree(
    root: &Path,
    journal: &IntegrationRecoveryJournal,
) -> Result<()> {
    let git_state = git_state_snapshot(root)?;
    if git_state.repository != journal.source_git_repository
        || git_state.head != journal.source_git_head
        || git_state.index_diff_sha256 != journal.source_git_index_diff_sha256
    {
        return Err(anyhow!("integration recovery journal git anchor changed"));
    }
    let mut manifest = file_manifest(root)?;
    for entry in &journal.entries {
        let relative = safe_swarm_relative_path(Path::new(&entry.path))
            .ok_or_else(|| anyhow!("integration recovery journal path is unsafe"))?;
        let current = integration_path_descriptor(&root.join(&relative))?;
        if current != entry.source_descriptor && current != entry.restored_descriptor {
            return Err(anyhow!(
                "integration recovery target changed outside journal: {}",
                entry.path
            ));
        }
        if entry.source_descriptor == "missing" {
            manifest.remove(&entry.path);
        } else {
            manifest.insert(entry.path.clone(), entry.source_descriptor.clone());
        }
        let auxiliary = integration_recovery_auxiliary_path(entry)?
            .to_string_lossy()
            .replace('\\', "/");
        manifest.remove(&auxiliary);
    }
    if manifest_sha256(&manifest)? != journal.source_manifest_sha256 {
        return Err(anyhow!(
            "integration recovery journal detected unrelated working tree drift"
        ));
    }
    Ok(())
}

fn write_integration_recovery_journal(
    path: &Path,
    journal: &IntegrationRecoveryJournal,
) -> Result<()> {
    write_json_durable_atomic(path, &serde_json::to_value(journal)?)
}

fn integration_recovery_journal_payload(journal: &IntegrationRecoveryJournal) -> Result<Vec<u8>> {
    let mut payload = serde_json::to_value(journal)?;
    payload
        .as_object_mut()
        .ok_or_else(|| anyhow!("integration recovery journal must be an object"))?
        .remove("integrity");
    Ok(serde_json::to_vec(&payload)?)
}

fn integration_recovery_journal_record_sha256(
    journal: &IntegrationRecoveryJournal,
) -> Result<String> {
    Ok(sha256_prefixed(&serde_json::to_vec(journal)?))
}

fn verify_integration_recovery_journal_mac(
    journal: &IntegrationRecoveryJournal,
    key: &LocalHmacKey,
) -> Result<()> {
    if journal.schema != INTEGRATION_RECOVERY_JOURNAL_SCHEMA || journal.revision == 0 {
        return Err(anyhow!("integration_recovery_integrity_downgrade"));
    }
    let integrity = journal
        .integrity
        .as_ref()
        .ok_or_else(|| anyhow!("integration_recovery_integrity_downgrade"))?;
    key.verify_payload(
        INTEGRATION_RECOVERY_JOURNAL_DOMAIN,
        &integration_recovery_journal_payload(journal)?,
        integrity,
    )?;
    Ok(())
}

fn sign_integration_recovery_journal(
    journal: &mut IntegrationRecoveryJournal,
    key: &LocalHmacKey,
    revision: u64,
    previous_record_sha256: &str,
) -> Result<()> {
    if revision == 0 {
        return Err(anyhow!("integration recovery revision must be positive"));
    }
    journal.schema = INTEGRATION_RECOVERY_JOURNAL_SCHEMA.to_string();
    journal.revision = revision;
    journal.integrity = None;
    let payload = integration_recovery_journal_payload(journal)?;
    journal.integrity = Some(key.sign_payload(
        INTEGRATION_RECOVERY_JOURNAL_DOMAIN,
        &payload,
        previous_record_sha256,
    )?);
    verify_integration_recovery_journal_mac(journal, key)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecoveryRevisionAnchor {
    revision: u64,
    record_sha256: String,
    previous_record_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryRevisionState {
    Anchored,
    RecoverableUnanchoredTail,
}

fn classify_integration_recovery_journal_revision(
    journal: &IntegrationRecoveryJournal,
    latest_anchor: Option<&RecoveryRevisionAnchor>,
) -> Result<RecoveryRevisionState> {
    match latest_anchor {
        Some(anchor) if journal.revision < anchor.revision => {
            return Err(anyhow!("integration_recovery_revision_replay"));
        }
        Some(anchor) if journal.revision > anchor.revision.saturating_add(1) => {
            return Err(anyhow!("integration_recovery_revision_gap"));
        }
        None if journal.revision != 1 => {
            return Err(anyhow!("integration_recovery_revision_gap"));
        }
        _ => {}
    }

    let integrity = journal
        .integrity
        .as_ref()
        .ok_or_else(|| anyhow!("integration_recovery_integrity_downgrade"))?;
    let record_sha256 = integration_recovery_journal_record_sha256(journal)?;
    match latest_anchor {
        Some(anchor) if journal.revision == anchor.revision => {
            if record_sha256 != anchor.record_sha256
                || integrity.previous_record_sha256 != anchor.previous_record_sha256
            {
                return Err(anyhow!("integration_recovery_integrity_mismatch"));
            }
            Ok(RecoveryRevisionState::Anchored)
        }
        Some(anchor) => {
            if journal.revision != anchor.revision + 1
                || integrity.previous_record_sha256 != anchor.record_sha256
            {
                return Err(anyhow!("integration_recovery_integrity_mismatch"));
            }
            Ok(RecoveryRevisionState::RecoverableUnanchoredTail)
        }
        None => {
            if integrity.previous_record_sha256 != INTEGRATION_RECOVERY_JOURNAL_GENESIS_SHA256 {
                return Err(anyhow!("integration_recovery_integrity_mismatch"));
            }
            Ok(RecoveryRevisionState::RecoverableUnanchoredTail)
        }
    }
}

fn latest_recovery_revision_anchor_from_events(
    events: &[WorkflowEvent],
    journal: &IntegrationRecoveryJournal,
) -> Result<Option<RecoveryRevisionAnchor>> {
    let mut latest: Option<RecoveryRevisionAnchor> = None;
    for event in events {
        if event.kind != WorkflowEventKind::SwarmIntegrationRecoveryRevisionCommitted
            || event.data["integration_id"].as_str() != Some(journal.integration_id.as_str())
            || event.data["checkpoint_sha256"].as_str() != Some(journal.checkpoint_sha256.as_str())
        {
            continue;
        }
        let revision = event.data["revision"]
            .as_u64()
            .filter(|revision| *revision > 0)
            .ok_or_else(|| anyhow!("integration recovery revision event is invalid"))?;
        let candidate = RecoveryRevisionAnchor {
            revision,
            record_sha256: event.data["record_sha256"]
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("integration recovery revision record hash is missing"))?
                .to_string(),
            previous_record_sha256: event.data["previous_record_sha256"]
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    anyhow!("integration recovery revision previous record hash is missing")
                })?
                .to_string(),
        };
        match latest.as_ref() {
            Some(current) if candidate.revision < current.revision => {}
            Some(current) if candidate.revision == current.revision => {
                if candidate != *current {
                    return Err(anyhow!("integration_recovery_integrity_mismatch"));
                }
            }
            _ => latest = Some(candidate),
        }
    }
    Ok(latest)
}

fn latest_recovery_revision_anchor(
    artifact_dir: &Path,
    journal: &IntegrationRecoveryJournal,
) -> Result<Option<RecoveryRevisionAnchor>> {
    latest_recovery_revision_anchor_from_events(&read_workflow_events(artifact_dir)?, journal)
}

fn append_integration_recovery_revision_event(
    artifact_dir: &Path,
    journal_path: &Path,
    journal: &IntegrationRecoveryJournal,
) -> Result<()> {
    let integrity = journal
        .integrity
        .as_ref()
        .ok_or_else(|| anyhow!("integration_recovery_integrity_downgrade"))?;
    let journal_relative_path = journal_path
        .strip_prefix(artifact_dir)
        .map_err(|_| anyhow!("integration recovery journal is outside workflow artifacts"))?
        .to_string_lossy()
        .replace('\\', "/");
    let journal_revision_id = format!(
        "{}:{}:{}",
        journal.integration_id, journal.checkpoint_sha256, journal.revision
    );
    let event_revision_id = journal_revision_id.clone();
    let integration_id = journal.integration_id.clone();
    let dispatch_id = journal.dispatch_id.clone();
    let checkpoint_sha256 = journal.checkpoint_sha256.clone();
    let binding_sha256 = integration_recovery_journal_binding_sha256(journal)?;
    let record_sha256 = integration_recovery_journal_record_sha256(journal)?;
    let payload_sha256 = integrity.payload_sha256.clone();
    let previous_record_sha256 = integrity.previous_record_sha256.clone();
    let key_id = integrity.key_id.clone();
    let revision = journal.revision;
    let status = journal.status.clone();
    let next_entry_index = journal.next_entry_index;
    let _ = append_workflow_event_idempotent_with_unique_data_value(
        artifact_dir,
        WorkflowEventKind::SwarmIntegrationRecoveryRevisionCommitted,
        "swarm_integration",
        "journal_revision_id",
        journal_revision_id,
        move |sequence, at_ms| {
            json!({
                "journal_revision_id": event_revision_id,
                "integration_id": integration_id,
                "dispatch_id": dispatch_id,
                "checkpoint_sha256": checkpoint_sha256,
                "journal_path": journal_relative_path,
                "journal_binding_sha256": binding_sha256,
                "revision": revision,
                "record_sha256": record_sha256,
                "payload_sha256": payload_sha256,
                "previous_record_sha256": previous_record_sha256,
                "key_id": key_id,
                "status": status,
                "next_entry_index": next_entry_index,
                "sequence": sequence,
                "created_at_ms": at_ms,
            })
        },
    )?;
    Ok(())
}

fn authenticated_integration_recovery_journal(
    artifact_dir: &Path,
    journal_path: &Path,
) -> Result<IntegrationRecoveryJournal> {
    let key = load_local_hmac_key()
        .map_err(|error| anyhow!(error))?
        .ok_or_else(|| anyhow!("integration_recovery_integrity_key_missing"))?;
    let journal: IntegrationRecoveryJournal = read_json_file(journal_path)?;
    verify_integration_recovery_journal_mac(&journal, &key)
        .map_err(|error| anyhow!("integration_recovery_integrity_mismatch: {error}"))?;
    let latest = latest_recovery_revision_anchor(artifact_dir, &journal)?;
    if classify_integration_recovery_journal_revision(&journal, latest.as_ref())?
        == RecoveryRevisionState::RecoverableUnanchoredTail
    {
        append_integration_recovery_revision_event(artifact_dir, journal_path, &journal)?;
    }
    Ok(journal)
}

fn commit_integration_recovery_journal_revision(
    artifact_dir: &Path,
    journal_path: &Path,
    journal: &mut IntegrationRecoveryJournal,
) -> Result<()> {
    let key = load_local_hmac_key()
        .map_err(|error| anyhow!(error))?
        .ok_or_else(|| anyhow!("integration_recovery_integrity_key_missing"))?;
    let (revision, previous_record_sha256) = if journal_path.exists() {
        let current = authenticated_integration_recovery_journal(artifact_dir, journal_path)?;
        if integration_recovery_journal_binding_sha256(&current)?
            != integration_recovery_journal_binding_sha256(journal)?
        {
            return Err(anyhow!("integration_recovery_integrity_mismatch"));
        }
        (
            current.revision.saturating_add(1),
            integration_recovery_journal_record_sha256(&current)?,
        )
    } else {
        (1, INTEGRATION_RECOVERY_JOURNAL_GENESIS_SHA256.to_string())
    };
    sign_integration_recovery_journal(journal, &key, revision, &previous_record_sha256)?;
    write_integration_recovery_journal(journal_path, journal)?;
    append_integration_recovery_revision_event(artifact_dir, journal_path, journal)
}

pub(crate) fn inspect_swarm_recovery_integrity(
    artifact_dir: &Path,
) -> Result<SwarmRecoveryIntegrityReport> {
    let integrations_dir = artifact_dir.join("integrations");
    let mut active_paths = Vec::new();
    let mut archive_paths = Vec::new();
    if integrations_dir.exists() {
        for integration in fs::read_dir(&integrations_dir)? {
            let integration = integration?;
            let integration_path = integration.path();
            let metadata = fs::symlink_metadata(&integration_path)?;
            if metadata.file_type().is_symlink() {
                return Err(anyhow!(
                    "integration_recovery_integrity_mismatch: symlinked integration directory"
                ));
            }
            if !metadata.is_dir() {
                continue;
            }
            let recovery_dir = integration_path.join("recovery");
            let active = recovery_dir.join("journal.json");
            if active.exists() {
                active_paths.push(active);
            }
            let history = recovery_dir.join("history");
            if history.exists() {
                for archive in fs::read_dir(&history)? {
                    let archive = archive?;
                    let path = archive.path();
                    let metadata = fs::symlink_metadata(&path)?;
                    if metadata.file_type().is_symlink() || !metadata.is_file() {
                        return Err(anyhow!(
                            "integration_recovery_archive_mismatch: {}",
                            path.display()
                        ));
                    }
                    if path.extension().and_then(|value| value.to_str()) == Some("json") {
                        archive_paths.push(path);
                    }
                }
            }
        }
    }
    active_paths.sort();
    archive_paths.sort();
    if active_paths.is_empty() && archive_paths.is_empty() {
        let eventlog_path = artifact_dir.join("eventlog.jsonl");
        let missing_archive = fs::read_to_string(&eventlog_path)?
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|event| event["kind"] == "swarm_integration_recovery_archived")
            .map(|event| {
                event["data"]["archive_path"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string()
            });
        if let Some(relative) = missing_archive {
            return Err(anyhow!("integration_recovery_archive_missing: {relative}"));
        }
        return Ok(SwarmRecoveryIntegrityReport {
            schema: "kiana.swarm-recovery-integrity-report.v1".to_string(),
            status: "verified".to_string(),
            active_journal_count: 0,
            verified_active_journal_count: 0,
            archived_journal_count: 0,
            verified_archived_journal_count: 0,
            recoverable_unanchored_tail_count: 0,
            legacy_unsigned_count: 0,
            mismatch_count: 0,
            key_id: load_local_hmac_key()
                .ok()
                .flatten()
                .map(|key| key.key_id().to_string()),
        });
    }
    let key = load_local_hmac_key()
        .map_err(|error| anyhow!(error))?
        .ok_or_else(|| anyhow!("integration_recovery_integrity_key_missing"))?;
    let events = read_workflow_events(artifact_dir)?;
    let archived_relative_paths = archive_paths
        .iter()
        .map(|path| {
            path.strip_prefix(artifact_dir)
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                .map_err(|_| anyhow!("integration_recovery_archive_mismatch: outside workflow"))
        })
        .collect::<Result<BTreeSet<_>>>()?;
    for event in events
        .iter()
        .filter(|event| event.kind == WorkflowEventKind::SwarmIntegrationRecoveryArchived)
    {
        let relative = event.data["archive_path"].as_str().ok_or_else(|| {
            anyhow!("integration_recovery_archive_mismatch: missing archive_path")
        })?;
        let relative_path = Path::new(relative);
        if relative_path.is_absolute()
            || relative_path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            return Err(anyhow!(
                "integration_recovery_archive_mismatch: invalid archive_path"
            ));
        }
        if !archived_relative_paths.contains(relative)
            || !artifact_dir.join(relative_path).is_file()
        {
            return Err(anyhow!("integration_recovery_archive_missing: {relative}"));
        }
    }
    for path in &active_paths {
        let journal: IntegrationRecoveryJournal = read_json_file(path)?;
        verify_integration_recovery_journal_mac(&journal, &key)
            .map_err(|error| anyhow!("integration_recovery_integrity_mismatch: {error}"))?;
        let latest = latest_recovery_revision_anchor_from_events(&events, &journal)?;
        if classify_integration_recovery_journal_revision(&journal, latest.as_ref())?
            != RecoveryRevisionState::Anchored
        {
            return Err(anyhow!("integration_recovery_revision_unanchored"));
        }
    }
    for path in &archive_paths {
        let journal: IntegrationRecoveryJournal = read_json_file(path)?;
        verify_integration_recovery_journal_mac(&journal, &key)
            .map_err(|error| anyhow!("integration_recovery_archive_mismatch: {error}"))?;
        if journal.status != "completed" {
            return Err(anyhow!(
                "integration_recovery_archive_mismatch: incomplete journal"
            ));
        }
        let relative = path
            .strip_prefix(artifact_dir)
            .map_err(|_| anyhow!("integration_recovery_archive_mismatch: outside workflow"))?
            .to_string_lossy()
            .replace('\\', "/");
        let record_sha256 = integration_recovery_journal_record_sha256(&journal)?;
        let anchored = events.iter().any(|event| {
            event.kind == WorkflowEventKind::SwarmIntegrationRecoveryArchived
                && event.data["archive_path"].as_str() == Some(relative.as_str())
                && event.data["revision"].as_u64() == Some(journal.revision)
                && event.data["record_sha256"].as_str() == Some(record_sha256.as_str())
        });
        if !anchored {
            return Err(anyhow!(
                "integration_recovery_archive_mismatch: missing archive event"
            ));
        }
    }
    Ok(SwarmRecoveryIntegrityReport {
        schema: "kiana.swarm-recovery-integrity-report.v1".to_string(),
        status: "verified".to_string(),
        active_journal_count: active_paths.len() as u64,
        verified_active_journal_count: active_paths.len() as u64,
        archived_journal_count: archive_paths.len() as u64,
        verified_archived_journal_count: archive_paths.len() as u64,
        recoverable_unanchored_tail_count: 0,
        legacy_unsigned_count: 0,
        mismatch_count: 0,
        key_id: Some(key.key_id().to_string()),
    })
}

fn integration_recovery_journal_binding_sha256(
    journal: &IntegrationRecoveryJournal,
) -> Result<String> {
    let mut binding = serde_json::to_value(journal)?;
    let object = binding
        .as_object_mut()
        .ok_or_else(|| anyhow!("integration recovery journal must be an object"))?;
    object.remove("revision");
    object.remove("integrity");
    object.remove("status");
    object.remove("next_entry_index");
    object.remove("last_error");
    object.remove("updated_at_ms");
    if let Some(entries) = object.get_mut("entries").and_then(Value::as_array_mut) {
        for entry in entries {
            entry
                .as_object_mut()
                .ok_or_else(|| anyhow!("integration recovery journal entry must be an object"))?
                .remove("status");
        }
    }
    Ok(format!(
        "sha256:{}",
        sha256_bytes(&serde_json::to_vec(&binding)?)
    ))
}

fn append_swarm_integration_recovery_started_event(
    artifact_dir: &Path,
    journal: &IntegrationRecoveryJournal,
) -> Result<()> {
    let binding_sha256 = integration_recovery_journal_binding_sha256(journal)?;
    let recovery_event_id = format!(
        "{}:recovery:{}",
        journal.integration_id, journal.checkpoint_sha256
    );
    let event_recovery_id = recovery_event_id.clone();
    let integration_id = journal.integration_id.clone();
    let dispatch_id = journal.dispatch_id.clone();
    let plan_sha256 = journal.plan_sha256.clone();
    let checkpoint_path = journal.checkpoint_path.clone();
    let checkpoint_sha256 = journal.checkpoint_sha256.clone();
    let expected_pre_restore_fingerprint = journal.expected_pre_restore_fingerprint.clone();
    let expected_post_restore_fingerprint = journal.expected_post_restore_fingerprint.clone();
    let entry_count = journal.entries.len();
    let recovery_created_at_ms = journal.created_at_ms;
    let _ = kiana_tasks::append_workflow_event_idempotent_with_unique_data_value(
        artifact_dir,
        WorkflowEventKind::SwarmIntegrationRecoveryStarted,
        "swarm_integration",
        "recovery_event_id",
        recovery_event_id,
        move |sequence, at_ms| {
            json!({
                "recovery_event_id": event_recovery_id,
                "integration_id": integration_id,
                "dispatch_id": dispatch_id,
                "plan_sha256": plan_sha256,
                "checkpoint_path": checkpoint_path,
                "checkpoint_sha256": checkpoint_sha256,
                "journal_path": "recovery/journal.json",
                "journal_binding_sha256": binding_sha256,
                "expected_pre_restore_fingerprint": expected_pre_restore_fingerprint,
                "expected_post_restore_fingerprint": expected_post_restore_fingerprint,
                "entry_count": entry_count,
                "recovery_created_at_ms": recovery_created_at_ms,
                "sequence": sequence,
                "created_at_ms": at_ms,
            })
        },
    )?;
    Ok(())
}

fn integration_path_descriptor(path: &Path) -> Result<String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok("missing".to_string())
        }
        Err(error) => return Err(error.into()),
    };
    let checkpoint_metadata = checkpoint_metadata_at_path(path)?;
    if metadata.file_type().is_symlink() {
        return Ok(format!(
            "symlink:{}:target_sha256:{}",
            checkpoint_metadata_descriptor(&checkpoint_metadata),
            sha256_bytes(fs::read_link(path)?.to_string_lossy().as_bytes())
        ));
    }
    if metadata.is_file() {
        return Ok(format!(
            "file:mode={}:{}:sha256:{}",
            file_mode_fingerprint(path)?,
            checkpoint_metadata_descriptor(&checkpoint_metadata),
            sha256_file(path)?
        ));
    }
    Ok("unsupported".to_string())
}

fn verify_integration_checkpoint_authentication(
    artifact_dir: &Path,
    integration_dir: &Path,
    project_root: &Path,
    plan: &Value,
    state: &Value,
) -> Result<VerifiedIntegrationCheckpoint> {
    if state["schema"] != "kiana.swarm-integration-state.v1"
        || state["integration_id"] != plan["integration_id"]
        || state["dispatch_id"] != plan["dispatch_id"]
        || state["plan_sha256"] != plan["plan_sha256"]
    {
        return Err(anyhow!("integration recovery state identity mismatch"));
    }
    let integration_id = plan["integration_id"]
        .as_str()
        .ok_or_else(|| anyhow!("integration plan id is missing"))?;
    let integration_event_id = format!("{integration_id}:applying");
    let events = read_workflow_events(artifact_dir)?;
    let started = events
        .iter()
        .find(|event| {
            event.kind == WorkflowEventKind::SwarmIntegrationStarted
                && event.data["integration_event_id"].as_str()
                    == Some(integration_event_id.as_str())
        })
        .ok_or_else(|| anyhow!("integration started event is missing"))?;
    if started.data["integration_id"] != plan["integration_id"]
        || started.data["dispatch_id"] != plan["dispatch_id"]
        || started.data["plan_sha256"] != plan["plan_sha256"]
        || started.data["checkpoint_path"].as_str() != Some("checkpoint/manifest.json")
        || started.data["working_tree_fingerprint"] != plan["baseline"]["working_tree_fingerprint"]
    {
        return Err(anyhow!("integration started event authentication mismatch"));
    }
    let expected_sha256 = started.data["checkpoint_sha256"]
        .as_str()
        .ok_or_else(|| anyhow!("integration started checkpoint hash is missing"))?;
    if state["checkpoint_path"] != started.data["checkpoint_path"]
        || state["checkpoint_sha256"] != started.data["checkpoint_sha256"]
    {
        return Err(anyhow!("integration checkpoint state projection mismatch"));
    }
    let manifest_path = integration_dir.join("checkpoint/manifest.json");
    let actual_sha256 = format!("sha256:{}", sha256_file(&manifest_path)?);
    if actual_sha256 != expected_sha256 {
        return Err(anyhow!(
            "integration checkpoint hash mismatch: expected {expected_sha256}, found {actual_sha256}"
        ));
    }
    let manifest: Value = read_json_file(&manifest_path)?;
    if manifest["schema"] != "kiana.swarm-integration-checkpoint.v2"
        || manifest["integration_id"] != plan["integration_id"]
        || manifest["dispatch_id"] != plan["dispatch_id"]
        || manifest["plan_sha256"] != plan["plan_sha256"]
        || manifest["baseline_fingerprint"] != plan["baseline"]["working_tree_fingerprint"]
    {
        return Err(anyhow!("integration checkpoint identity mismatch"));
    }
    let recovery_journal_path = integration_recovery_journal_path(integration_dir);
    let recovery_journal = if recovery_journal_path.exists() {
        Some(authenticated_integration_recovery_journal(
            artifact_dir,
            &recovery_journal_path,
        )?)
    } else {
        None
    };
    if let Some(journal) = recovery_journal.as_ref() {
        if journal.schema != INTEGRATION_RECOVERY_JOURNAL_SCHEMA
            || journal.integration_id != integration_id
            || journal.dispatch_id != plan["dispatch_id"].as_str().unwrap_or_default()
            || journal.plan_sha256 != plan["plan_sha256"].as_str().unwrap_or_default()
            || journal.checkpoint_path != state["checkpoint_path"].as_str().unwrap_or_default()
            || journal.checkpoint_sha256 != expected_sha256
            || journal.expected_pre_restore_fingerprint != started.data["working_tree_fingerprint"]
            || journal.expected_post_restore_fingerprint
                != plan["baseline"]["working_tree_fingerprint"]
                    .as_str()
                    .unwrap_or_default()
        {
            return Err(anyhow!("integration recovery journal identity mismatch"));
        }
    }
    let mut entries = Vec::new();
    for (index, file) in manifest["files"]
        .as_array()
        .ok_or_else(|| anyhow!("integration checkpoint files are missing"))?
        .iter()
        .enumerate()
    {
        let path = file["path"]
            .as_str()
            .ok_or_else(|| anyhow!("checkpoint path is missing"))?;
        let relative = safe_swarm_relative_path(Path::new(path))
            .ok_or_else(|| anyhow!("unsafe checkpoint path {path}"))?;
        let content = if !file["existed"].as_bool().unwrap_or(false) {
            VerifiedCheckpointContent::Missing
        } else {
            let metadata: CheckpointMetadata = serde_json::from_value(file["metadata"].clone())
                .with_context(|| format!("checkpoint metadata is invalid for {path}"))?;
            match file["kind"].as_str() {
                Some("file") => {
                    let backup = integration_dir.join("checkpoint/files").join(&relative);
                    let actual_backup_sha256 = format!("sha256:{}", sha256_file(&backup)?);
                    let expected_backup_sha256 = file["backup_sha256"]
                        .as_str()
                        .ok_or_else(|| anyhow!("checkpoint backup hash is missing for {path}"))?;
                    if actual_backup_sha256 != expected_backup_sha256 {
                        return Err(anyhow!(
                            "checkpoint backup hash mismatch for {path}: expected {expected_backup_sha256}, found {actual_backup_sha256}"
                        ));
                    }
                    let actual_backup_mode = file_mode_fingerprint(&backup)?;
                    let expected_backup_mode = file["backup_mode"]
                        .as_str()
                        .ok_or_else(|| anyhow!("checkpoint backup mode is missing for {path}"))?;
                    if actual_backup_mode != expected_backup_mode {
                        return Err(anyhow!(
                            "checkpoint backup mode mismatch for {path}: expected {expected_backup_mode}, found {actual_backup_mode}"
                        ));
                    }
                    VerifiedCheckpointContent::File {
                        contents: fs::read(&backup)?,
                        permissions: fs::metadata(&backup)?.permissions(),
                        metadata,
                    }
                }
                Some("symlink") => VerifiedCheckpointContent::Symlink {
                    target: PathBuf::from(
                        file["symlink_target"]
                            .as_str()
                            .ok_or_else(|| anyhow!("checkpoint symlink target is missing"))?,
                    ),
                    metadata,
                },
                other => return Err(anyhow!("unsupported checkpoint kind {other:?}")),
            }
        };
        let actual_current = integration_path_descriptor(&project_root.join(&relative))?;
        let expected_current = if let Some(journal) = recovery_journal.as_ref() {
            let journal_entry = journal
                .entries
                .get(index)
                .ok_or_else(|| anyhow!("integration recovery journal entries are missing"))?;
            let restored_descriptor = verified_checkpoint_content_descriptor(&content);
            if journal_entry.path != path
                || journal_entry.restored_descriptor != restored_descriptor
                || (actual_current != journal_entry.source_descriptor
                    && actual_current != journal_entry.restored_descriptor)
            {
                return Err(anyhow!(
                    "integration recovery journal current path state mismatch: {path}"
                ));
            }
            journal_entry.source_descriptor.clone()
        } else {
            actual_current
        };
        entries.push(VerifiedCheckpointEntry {
            relative,
            expected_current,
            content,
        });
    }
    let expected_fingerprint = events
        .iter()
        .filter(|event| {
            event.kind == WorkflowEventKind::SwarmWorkerApplied
                && event.data["integration_id"] == plan["integration_id"]
        })
        .max_by_key(|event| event.seq)
        .map(|event| {
            if event.data["dispatch_id"] != plan["dispatch_id"]
                || event.data["plan_sha256"] != plan["plan_sha256"]
                || event.data["checkpoint_path"] != started.data["checkpoint_path"]
                || event.data["checkpoint_sha256"] != started.data["checkpoint_sha256"]
            {
                return Err(anyhow!("integration applied event authentication mismatch"));
            }
            event.data["working_tree_fingerprint"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| anyhow!("integration applied event fingerprint is missing"))
        })
        .transpose()?
        .unwrap_or_else(|| {
            started.data["working_tree_fingerprint"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        });
    if expected_fingerprint.is_empty() {
        return Err(anyhow!("integration recovery fingerprint is missing"));
    }
    let checkpoint = VerifiedIntegrationCheckpoint {
        expected_fingerprint,
        entries,
    };
    if let Some(journal) = recovery_journal.as_ref() {
        validate_integration_recovery_journal_binding(journal, plan, state, &checkpoint)?;
    }
    Ok(checkpoint)
}

#[cfg(test)]
fn restore_verified_integration_checkpoint(
    root: &Path,
    checkpoint: &VerifiedIntegrationCheckpoint,
) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        return restore_verified_integration_checkpoint_linux(root, checkpoint);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (root, checkpoint);
        Err(anyhow!(
            "secure integration recovery is unsupported on this platform"
        ))
    }
}

fn restore_verified_integration_checkpoint_with_journal(
    artifact_dir: &Path,
    root: &Path,
    integration_dir: &Path,
    plan: &Value,
    state: &Value,
    checkpoint: &VerifiedIntegrationCheckpoint,
) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let journal_path = integration_recovery_journal_path(integration_dir);
        let mut journal = if journal_path.exists() {
            authenticated_integration_recovery_journal(artifact_dir, &journal_path)?
        } else {
            let mut journal = create_integration_recovery_journal(root, plan, state, checkpoint)?;
            commit_integration_recovery_journal_revision(
                artifact_dir,
                &journal_path,
                &mut journal,
            )?;
            journal
        };
        validate_integration_recovery_journal_binding(&journal, plan, state, checkpoint)?;
        validate_integration_recovery_journal_tree(root, &journal)?;
        append_swarm_integration_recovery_started_event(artifact_dir, &journal)?;
        journal.status = "restoring".to_string();
        journal.last_error = None;
        journal.updated_at_ms = now_ms();
        commit_integration_recovery_journal_revision(artifact_dir, &journal_path, &mut journal)?;

        let root_fd = linux_open_project_root(root)?;
        for index in 0..checkpoint.entries.len() {
            journal.entries[index].status = IntegrationRecoveryEntryStatus::Restoring;
            journal.next_entry_index = index;
            journal.updated_at_ms = now_ms();
            commit_integration_recovery_journal_revision(
                artifact_dir,
                &journal_path,
                &mut journal,
            )?;
            if let Err(error) = linux_restore_journal_entry(
                root_fd.as_raw_fd(),
                &checkpoint.entries[index],
                &journal.entries[index],
            ) {
                journal.status = "blocked".to_string();
                journal.last_error = Some(error.to_string());
                journal.updated_at_ms = now_ms();
                if let Err(commit_error) = commit_integration_recovery_journal_revision(
                    artifact_dir,
                    &journal_path,
                    &mut journal,
                ) {
                    return Err(anyhow!(
                        "{error}; failed to persist authenticated recovery failure: {commit_error}"
                    ));
                }
                return Err(error);
            }
            journal.entries[index].status = IntegrationRecoveryEntryStatus::Completed;
            journal.next_entry_index = index + 1;
            journal.updated_at_ms = now_ms();
            commit_integration_recovery_journal_revision(
                artifact_dir,
                &journal_path,
                &mut journal,
            )?;
        }
        journal.status = "completed".to_string();
        journal.next_entry_index = journal.entries.len();
        journal.last_error = None;
        journal.updated_at_ms = now_ms();
        commit_integration_recovery_journal_revision(artifact_dir, &journal_path, &mut journal)?;
        return Ok(());
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (artifact_dir, root, integration_dir, plan, state, checkpoint);
        Err(anyhow!(
            "secure integration recovery is unsupported on this platform"
        ))
    }
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct LinuxOpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

#[cfg(target_os = "linux")]
const LINUX_RESOLVE_NO_MAGICLINKS: u64 = 0x02;
#[cfg(target_os = "linux")]
const LINUX_RESOLVE_NO_SYMLINKS: u64 = 0x04;
#[cfg(target_os = "linux")]
const LINUX_RESOLVE_BENEATH: u64 = 0x08;
#[cfg(target_os = "linux")]
const LINUX_RENAME_NOREPLACE: u32 = 1;
#[cfg(target_os = "linux")]
const LINUX_RENAME_EXCHANGE: u32 = 2;

#[cfg(all(target_os = "linux", test))]
fn restore_verified_integration_checkpoint_linux(
    root: &Path,
    checkpoint: &VerifiedIntegrationCheckpoint,
) -> Result<()> {
    let root_fd = linux_open_project_root(root)?;
    for entry in &checkpoint.entries {
        linux_restore_verified_entry(root_fd.as_raw_fd(), entry)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_open_project_root(root: &Path) -> Result<OwnedFd> {
    let root = linux_cstring(root)?;
    let fd = unsafe {
        libc::open(
            root.as_ptr(),
            libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(anyhow!(
            "secure integration recovery cannot open project root: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

#[cfg(target_os = "linux")]
fn linux_open_parent_beneath(root_fd: RawFd, relative: &Path) -> Result<(OwnedFd, CString)> {
    let name = relative
        .file_name()
        .ok_or_else(|| anyhow!("secure integration recovery path has no file name"))?;
    let parent = relative
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = linux_cstring(parent)?;
    let how = LinuxOpenHow {
        flags: (libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) as u64,
        mode: 0,
        resolve: LINUX_RESOLVE_BENEATH | LINUX_RESOLVE_NO_SYMLINKS | LINUX_RESOLVE_NO_MAGICLINKS,
    };
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            root_fd,
            parent.as_ptr(),
            &how,
            std::mem::size_of::<LinuxOpenHow>(),
        ) as libc::c_int
    };
    if fd < 0 {
        return Err(anyhow!(
            "secure integration recovery rejected parent {}: {}",
            relative.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok((
        unsafe { OwnedFd::from_raw_fd(fd) },
        linux_cstring(Path::new(name))?,
    ))
}

#[cfg(target_os = "linux")]
fn linux_cstring(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| anyhow!("secure integration recovery path contains NUL"))
}

#[cfg(target_os = "linux")]
fn linux_descriptor_at(parent_fd: RawFd, name: &CString) -> Result<String> {
    let fd = unsafe {
        libc::openat(
            parent_fd,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd >= 0 {
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe { libc::fstat(fd.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let stat = unsafe { stat.assume_init() };
        if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Ok("unsupported".to_string());
        }
        let checkpoint_metadata = CheckpointMetadata {
            uid: stat.st_uid,
            gid: stat.st_gid,
            xattrs: linux_list_xattrs_fd(fd.as_raw_fd())?,
        };
        let mut file = fs::File::from(fd);
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        return Ok(format!(
            "file:mode={:04o}:{}:sha256:{}",
            stat.st_mode & 0o7777,
            checkpoint_metadata_descriptor(&checkpoint_metadata),
            sha256_bytes(&contents)
        ));
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ENOENT) => Ok("missing".to_string()),
        Some(libc::ELOOP) => {
            let target = linux_readlink_at(parent_fd, name)?;
            let checkpoint_metadata = linux_checkpoint_metadata_at(parent_fd, name)?;
            Ok(format!(
                "symlink:{}:target_sha256:{}",
                checkpoint_metadata_descriptor(&checkpoint_metadata),
                sha256_bytes(&target)
            ))
        }
        _ => Err(error.into()),
    }
}

#[cfg(target_os = "linux")]
fn linux_readlink_at(parent_fd: RawFd, name: &CString) -> Result<Vec<u8>> {
    let mut capacity = 256usize;
    loop {
        let mut buffer = vec![0u8; capacity];
        let length = unsafe {
            libc::readlinkat(
                parent_fd,
                name.as_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
        if length < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let length = length as usize;
        if length < capacity {
            buffer.truncate(length);
            return Ok(buffer);
        }
        capacity = capacity
            .checked_mul(2)
            .ok_or_else(|| anyhow!("secure integration recovery symlink target is too large"))?;
    }
}

#[cfg(all(target_os = "linux", test))]
fn linux_restore_verified_entry(root_fd: RawFd, entry: &VerifiedCheckpointEntry) -> Result<()> {
    let (parent_fd, name) = linux_open_parent_beneath(root_fd, &entry.relative)?;
    let current = linux_descriptor_at(parent_fd.as_raw_fd(), &name)?;
    if current != entry.expected_current {
        return Err(anyhow!(
            "secure integration recovery target changed before restore: {}",
            entry.relative.display()
        ));
    }
    match &entry.content {
        VerifiedCheckpointContent::Missing => linux_remove_expected_entry(
            parent_fd.as_raw_fd(),
            &name,
            &entry.expected_current,
            &entry.relative,
        ),
        VerifiedCheckpointContent::File {
            contents,
            permissions,
            ..
        } => {
            let temp = linux_create_temp_file(
                parent_fd.as_raw_fd(),
                contents,
                permissions.mode() & 0o7777,
            )?;
            linux_install_temp_entry(
                parent_fd.as_raw_fd(),
                &temp,
                &name,
                &entry.expected_current,
                &entry.relative,
            )
        }
        VerifiedCheckpointContent::Symlink { target, .. } => {
            let temp = linux_create_temp_symlink(parent_fd.as_raw_fd(), target)?;
            linux_install_temp_entry(
                parent_fd.as_raw_fd(),
                &temp,
                &name,
                &entry.expected_current,
                &entry.relative,
            )
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_restore_journal_entry(
    root_fd: RawFd,
    checkpoint_entry: &VerifiedCheckpointEntry,
    journal_entry: &IntegrationRecoveryJournalEntry,
) -> Result<()> {
    let (parent_fd, target) = linux_open_parent_beneath(root_fd, &checkpoint_entry.relative)?;
    let auxiliary = CString::new(journal_entry.auxiliary_name.as_bytes())
        .map_err(|_| anyhow!("integration recovery auxiliary name contains NUL"))?;
    let mut current = linux_descriptor_at(parent_fd.as_raw_fd(), &target)?;
    let mut auxiliary_descriptor = linux_descriptor_at(parent_fd.as_raw_fd(), &auxiliary)?;

    if current == journal_entry.restored_descriptor {
        if auxiliary_descriptor != "missing" {
            let expected_auxiliary = match journal_entry.operation {
                IntegrationRecoveryOperation::Remove => &journal_entry.source_descriptor,
                IntegrationRecoveryOperation::InstallFile
                | IntegrationRecoveryOperation::InstallSymlink => {
                    if journal_entry.source_descriptor == "missing" {
                        &journal_entry.restored_descriptor
                    } else {
                        &journal_entry.source_descriptor
                    }
                }
            };
            if auxiliary_descriptor != *expected_auxiliary {
                return Err(anyhow!(
                    "integration recovery auxiliary changed after mutation: {}",
                    journal_entry.path
                ));
            }
            linux_unlink_name(parent_fd.as_raw_fd(), &auxiliary)?;
            linux_sync_directory(parent_fd.as_raw_fd())?;
        }
        return Ok(());
    }
    if current != journal_entry.source_descriptor {
        return Err(anyhow!(
            "integration recovery target changed outside journal: {}",
            journal_entry.path
        ));
    }

    match (&journal_entry.operation, &checkpoint_entry.content) {
        (IntegrationRecoveryOperation::Remove, VerifiedCheckpointContent::Missing) => {
            if journal_entry.source_descriptor == "missing" {
                return Ok(());
            }
            if auxiliary_descriptor == "missing" {
                linux_renameat2(
                    parent_fd.as_raw_fd(),
                    &target,
                    parent_fd.as_raw_fd(),
                    &auxiliary,
                    LINUX_RENAME_NOREPLACE,
                )
                .with_context(|| {
                    format!(
                        "integration recovery target changed before journaled removal {}",
                        journal_entry.path
                    )
                })?;
                linux_sync_directory(parent_fd.as_raw_fd())?;
                current = linux_descriptor_at(parent_fd.as_raw_fd(), &target)?;
                auxiliary_descriptor = linux_descriptor_at(parent_fd.as_raw_fd(), &auxiliary)?;
            }
            if current != "missing" || auxiliary_descriptor != journal_entry.source_descriptor {
                return Err(anyhow!(
                    "integration recovery journaled removal state mismatch: {}",
                    journal_entry.path
                ));
            }
            linux_unlink_name(parent_fd.as_raw_fd(), &auxiliary)?;
            linux_sync_directory(parent_fd.as_raw_fd())
        }
        (
            IntegrationRecoveryOperation::InstallFile,
            VerifiedCheckpointContent::File {
                contents,
                permissions,
                metadata,
            },
        ) => {
            if auxiliary_descriptor == "missing" {
                linux_create_named_file(
                    parent_fd.as_raw_fd(),
                    &auxiliary,
                    contents,
                    permissions.mode() & 0o7777,
                    metadata,
                )?;
                linux_sync_directory(parent_fd.as_raw_fd())?;
                auxiliary_descriptor = linux_descriptor_at(parent_fd.as_raw_fd(), &auxiliary)?;
            }
            if auxiliary_descriptor != journal_entry.restored_descriptor {
                return Err(anyhow!(
                    "integration recovery prepared file changed: {}",
                    journal_entry.path
                ));
            }
            linux_install_journal_auxiliary(
                parent_fd.as_raw_fd(),
                &auxiliary,
                &target,
                journal_entry,
            )
        }
        (
            IntegrationRecoveryOperation::InstallSymlink,
            VerifiedCheckpointContent::Symlink {
                target: target_value,
                metadata,
            },
        ) => {
            if auxiliary_descriptor == "missing" {
                linux_create_named_symlink(
                    parent_fd.as_raw_fd(),
                    &auxiliary,
                    target_value,
                    metadata,
                )?;
                linux_sync_directory(parent_fd.as_raw_fd())?;
                auxiliary_descriptor = linux_descriptor_at(parent_fd.as_raw_fd(), &auxiliary)?;
            }
            if auxiliary_descriptor != journal_entry.restored_descriptor {
                return Err(anyhow!(
                    "integration recovery prepared symlink changed: {}",
                    journal_entry.path
                ));
            }
            linux_install_journal_auxiliary(
                parent_fd.as_raw_fd(),
                &auxiliary,
                &target,
                journal_entry,
            )
        }
        _ => Err(anyhow!(
            "integration recovery journal operation does not match checkpoint content"
        )),
    }
}

#[cfg(target_os = "linux")]
fn linux_install_journal_auxiliary(
    parent_fd: RawFd,
    auxiliary: &CString,
    target: &CString,
    entry: &IntegrationRecoveryJournalEntry,
) -> Result<()> {
    if entry.source_descriptor == "missing" {
        linux_renameat2(
            parent_fd,
            auxiliary,
            parent_fd,
            target,
            LINUX_RENAME_NOREPLACE,
        )
        .with_context(|| {
            format!(
                "integration recovery target changed before journaled install {}",
                entry.path
            )
        })?;
        linux_sync_directory(parent_fd)?;
        if linux_descriptor_at(parent_fd, target)? != entry.restored_descriptor {
            return Err(anyhow!(
                "integration recovery installed descriptor mismatch: {}",
                entry.path
            ));
        }
        return Ok(());
    }

    linux_renameat2(
        parent_fd,
        auxiliary,
        parent_fd,
        target,
        LINUX_RENAME_EXCHANGE,
    )
    .with_context(|| {
        format!(
            "integration recovery target changed before journaled exchange {}",
            entry.path
        )
    })?;
    linux_sync_directory(parent_fd)?;
    let installed = linux_descriptor_at(parent_fd, target)?;
    let displaced = linux_descriptor_at(parent_fd, auxiliary)?;
    if installed != entry.restored_descriptor || displaced != entry.source_descriptor {
        if installed == entry.source_descriptor && displaced == entry.restored_descriptor {
            return Err(anyhow!(
                "integration recovery exchange did not mutate target: {}",
                entry.path
            ));
        }
        return Err(anyhow!(
            "integration recovery journaled exchange state mismatch: {}",
            entry.path
        ));
    }
    linux_unlink_name(parent_fd, auxiliary)?;
    linux_sync_directory(parent_fd)
}

#[cfg(target_os = "linux")]
fn linux_create_named_file(
    parent_fd: RawFd,
    name: &CString,
    contents: &[u8],
    mode: u32,
    metadata: &CheckpointMetadata,
) -> Result<()> {
    let fd = unsafe {
        libc::openat(
            parent_fd,
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut file = fs::File::from(fd);
    file.write_all(contents)?;
    file.flush()?;
    linux_apply_file_metadata(file.as_raw_fd(), mode, metadata)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_create_named_symlink(
    parent_fd: RawFd,
    name: &CString,
    target: &Path,
    metadata: &CheckpointMetadata,
) -> Result<()> {
    let target = linux_cstring(target)?;
    if unsafe { libc::symlinkat(target.as_ptr(), parent_fd, name.as_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    linux_apply_symlink_metadata(parent_fd, name, metadata)
}

#[cfg(all(target_os = "linux", test))]
fn linux_create_temp_file(parent_fd: RawFd, contents: &[u8], mode: u32) -> Result<CString> {
    for attempt in 0..32u32 {
        let name = linux_recovery_name("prepared", attempt)?;
        let fd = unsafe {
            libc::openat(
                parent_fd,
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EEXIST) {
                continue;
            }
            return Err(error.into());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let mut file = fs::File::from(fd);
        file.write_all(contents)?;
        file.flush()?;
        if unsafe { libc::fchmod(file.as_raw_fd(), mode as libc::mode_t) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        file.sync_all()?;
        return Ok(name);
    }
    Err(anyhow!(
        "secure integration recovery could not allocate temporary file"
    ))
}

#[cfg(all(target_os = "linux", test))]
fn linux_create_temp_symlink(parent_fd: RawFd, target: &Path) -> Result<CString> {
    let target = linux_cstring(target)?;
    for attempt in 0..32u32 {
        let name = linux_recovery_name("prepared", attempt)?;
        let result = unsafe { libc::symlinkat(target.as_ptr(), parent_fd, name.as_ptr()) };
        if result == 0 {
            return Ok(name);
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EEXIST) {
            return Err(error.into());
        }
    }
    Err(anyhow!(
        "secure integration recovery could not allocate temporary symlink"
    ))
}

#[cfg(all(target_os = "linux", test))]
fn linux_install_temp_entry(
    parent_fd: RawFd,
    temp: &CString,
    target: &CString,
    expected_current: &str,
    relative: &Path,
) -> Result<()> {
    if expected_current == "missing" {
        if let Err(error) =
            linux_renameat2(parent_fd, temp, parent_fd, target, LINUX_RENAME_NOREPLACE)
        {
            let _ = linux_unlink_name(parent_fd, temp);
            return Err(anyhow!(
                "secure integration recovery target changed before install {}: {error}",
                relative.display()
            ));
        }
        return linux_sync_directory(parent_fd);
    }

    if let Err(error) = linux_renameat2(parent_fd, temp, parent_fd, target, LINUX_RENAME_EXCHANGE) {
        let _ = linux_unlink_name(parent_fd, temp);
        return Err(anyhow!(
            "secure integration recovery target changed before exchange {}: {error}",
            relative.display()
        ));
    }
    let displaced = linux_descriptor_at(parent_fd, temp)?;
    if displaced != expected_current {
        linux_renameat2(parent_fd, temp, parent_fd, target, LINUX_RENAME_EXCHANGE).with_context(
            || {
                format!(
                    "secure integration recovery could not restore raced target {}",
                    relative.display()
                )
            },
        )?;
        linux_unlink_name(parent_fd, temp)?;
        linux_sync_directory(parent_fd)?;
        return Err(anyhow!(
            "secure integration recovery target changed during exchange: {}",
            relative.display()
        ));
    }
    linux_unlink_name(parent_fd, temp)?;
    linux_sync_directory(parent_fd)
}

#[cfg(all(target_os = "linux", test))]
fn linux_remove_expected_entry(
    parent_fd: RawFd,
    target: &CString,
    expected_current: &str,
    relative: &Path,
) -> Result<()> {
    if expected_current == "missing" {
        return Ok(());
    }
    for attempt in 0..32u32 {
        let quarantine = linux_recovery_name("removed", attempt)?;
        match linux_renameat2(
            parent_fd,
            target,
            parent_fd,
            &quarantine,
            LINUX_RENAME_NOREPLACE,
        ) {
            Ok(()) => {
                let displaced = linux_descriptor_at(parent_fd, &quarantine)?;
                if displaced != expected_current {
                    linux_renameat2(
                        parent_fd,
                        &quarantine,
                        parent_fd,
                        target,
                        LINUX_RENAME_NOREPLACE,
                    )
                    .with_context(|| {
                        format!(
                            "secure integration recovery could not restore raced removal {}",
                            relative.display()
                        )
                    })?;
                    linux_sync_directory(parent_fd)?;
                    return Err(anyhow!(
                        "secure integration recovery target changed during removal: {}",
                        relative.display()
                    ));
                }
                linux_unlink_name(parent_fd, &quarantine)?;
                return linux_sync_directory(parent_fd);
            }
            Err(error) if error.raw_os_error() == Some(libc::EEXIST) => continue,
            Err(error) => {
                return Err(anyhow!(
                    "secure integration recovery target changed before removal {}: {error}",
                    relative.display()
                ))
            }
        }
    }
    Err(anyhow!(
        "secure integration recovery could not allocate quarantine path"
    ))
}

#[cfg(all(target_os = "linux", test))]
fn linux_recovery_name(kind: &str, attempt: u32) -> Result<CString> {
    CString::new(format!(
        ".kiana-recovery-{kind}-{}-{}-{attempt}",
        std::process::id(),
        now_ms()
    ))
    .map_err(|_| anyhow!("secure integration recovery generated an invalid name"))
}

#[cfg(target_os = "linux")]
fn linux_renameat2(
    old_parent: RawFd,
    old_name: &CString,
    new_parent: RawFd,
    new_name: &CString,
    flags: u32,
) -> std::io::Result<()> {
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            old_parent,
            old_name.as_ptr(),
            new_parent,
            new_name.as_ptr(),
            flags,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn linux_unlink_name(parent_fd: RawFd, name: &CString) -> Result<()> {
    if unsafe { libc::unlinkat(parent_fd, name.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().into())
    }
}

#[cfg(target_os = "linux")]
fn linux_sync_directory(parent_fd: RawFd) -> Result<()> {
    if unsafe { libc::fsync(parent_fd) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().into())
    }
}

fn create_integration_verification_snapshot(
    project_root: &Path,
    integration_dir: &Path,
    task_id: &str,
) -> Result<PathBuf> {
    let relative = safe_swarm_relative_path(Path::new(task_id))
        .ok_or_else(|| anyhow!("unsafe verification task id {task_id}"))?;
    let snapshot = integration_dir
        .join("verification-snapshots")
        .join(relative);
    if snapshot.exists() {
        fs::remove_dir_all(&snapshot)?;
    }
    fs::create_dir_all(&snapshot)?;
    copy_swarm_snapshot(project_root, &snapshot, true)?;
    Ok(snapshot)
}

fn remove_integration_verification_snapshot(snapshot: &Path) -> Result<()> {
    if snapshot.exists() {
        fs::remove_dir_all(snapshot)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn run_swarm_integration_command(
    context: &CommandContext,
    snapshot_root: &Path,
    project_root: &Path,
    task_id: &str,
    command: &str,
) -> Result<Value> {
    let bwrap = bash_sandbox_bwrap_path(&context.app_state).ok_or_else(|| {
        anyhow!("swarm integration verification requires bubblewrap (bwrap) on Linux")
    })?;
    let plan = strict_bwrap_plan(
        &bwrap,
        snapshot_root,
        &[project_root.to_path_buf()],
        &[snapshot_root.to_path_buf()],
        std::ffi::OsString::from(integration_shell_program()),
        integration_shell_args(command)
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect(),
    )?;
    let output = ProcessCommand::new(&plan.program)
        .args(&plan.args)
        .current_dir(snapshot_root)
        .output()
        .with_context(|| format!("failed to run sandboxed integration verification: {command}"))?;
    Ok(json!({
        "task_id": task_id,
        "command": command,
        "sandbox": "bwrap",
        "network": "isolated",
        "status": if output.status.success() { "pass" } else { "fail" },
        "exit_code": output.status.code(),
        "stdout": bounded_output_tail(&output.stdout),
        "stderr": bounded_output_tail(&output.stderr),
    }))
}

#[cfg(not(target_os = "linux"))]
fn run_swarm_integration_command(
    _context: &CommandContext,
    _snapshot_root: &Path,
    _project_root: &Path,
    _task_id: &str,
    _command: &str,
) -> Result<Value> {
    Err(anyhow!(
        "swarm integration verification sandbox is unavailable on this platform"
    ))
}

#[cfg(unix)]
fn integration_shell_program() -> &'static str {
    "bash"
}

#[cfg(unix)]
fn integration_shell_args(command: &str) -> [&str; 2] {
    ["-lc", command]
}

#[cfg(windows)]
fn integration_shell_program() -> &'static str {
    "cmd"
}

#[cfg(windows)]
fn integration_shell_args(command: &str) -> [&str; 2] {
    ["/C", command]
}

fn bounded_output_tail(output: &[u8]) -> String {
    const LIMIT: usize = 8 * 1024;
    let start = output.len().saturating_sub(LIMIT);
    String::from_utf8_lossy(&output[start..]).to_string()
}

fn integration_state_value(
    plan: &Value,
    status: &str,
    applied_task_ids: Vec<String>,
    verified_task_ids: Vec<String>,
    current_task_id: Option<&str>,
    failure: Option<Value>,
    next_action: &str,
) -> Value {
    json!({
        "schema": "kiana.swarm-integration-state.v1",
        "integration_id": plan["integration_id"],
        "dispatch_id": plan["dispatch_id"],
        "status": status,
        "plan_sha256": plan["plan_sha256"],
        "checkpoint_path": Value::Null,
        "checkpoint_sha256": Value::Null,
        "working_tree_fingerprint": Value::Null,
        "applied_task_ids": applied_task_ids,
        "verified_task_ids": verified_task_ids,
        "current_task_id": current_task_id,
        "failure": failure,
        "updated_at_ms": now_ms(),
        "next_action": next_action,
    })
}

fn recover_interrupted_swarm_integration(
    artifact_dir: &Path,
    integration_dir: &Path,
    project_root: &Path,
    plan: &Value,
    mut state: Value,
    json_output: bool,
) -> Result<CommandResult> {
    let previous_status = state["status"].as_str().unwrap_or("unknown").to_string();
    let recovery_anchor = verify_integration_checkpoint_authentication(
        artifact_dir,
        integration_dir,
        project_root,
        plan,
        &state,
    );
    let actual_known_fingerprint = current_working_tree_fingerprint(project_root)?;
    let checkpoint = match recovery_anchor {
        Ok(checkpoint) => checkpoint,
        Err(error) => {
            return block_interrupted_swarm_integration(
                artifact_dir,
                integration_dir,
                plan,
                state,
                &previous_status,
                "interrupted_integration_checkpoint_invalid",
                json!({"detail": error.to_string()}),
                json_output,
            );
        }
    };
    let journal_path = integration_recovery_journal_path(integration_dir);
    if journal_path.exists() {
        let journal = authenticated_integration_recovery_journal(artifact_dir, &journal_path)?;
        if let Err(error) =
            validate_integration_recovery_journal_binding(&journal, plan, &state, &checkpoint)
                .and_then(|_| validate_integration_recovery_journal_tree(project_root, &journal))
        {
            return block_interrupted_swarm_integration(
                artifact_dir,
                integration_dir,
                plan,
                state,
                &previous_status,
                "interrupted_integration_journal_invalid",
                json!({"detail": error.to_string()}),
                json_output,
            );
        }
    } else if actual_known_fingerprint != checkpoint.expected_fingerprint {
        return block_interrupted_swarm_integration(
            artifact_dir,
            integration_dir,
            plan,
            state,
            &previous_status,
            "interrupted_integration_tree_drift",
            json!({
                "expected": checkpoint.expected_fingerprint,
                "actual": actual_known_fingerprint,
            }),
            json_output,
        );
    }
    state["plan_sha256"] = plan["plan_sha256"].clone();
    state["status"] = json!("rolling_back");
    state["failure"] = json!({
        "reason": "interrupted_integration_detected",
        "previous_status": previous_status,
    });
    state["recovery_journal_path"] = json!("recovery/journal.json");
    state["next_action"] = json!("recover_checkpoint");
    state["updated_at_ms"] = json!(now_ms());
    write_json_atomic(&integration_dir.join("state.json"), &state)?;

    let verification_snapshots = integration_dir.join("verification-snapshots");
    let cleanup_error = if verification_snapshots.exists() {
        fs::remove_dir_all(&verification_snapshots)
            .err()
            .map(|error| error.to_string())
    } else {
        None
    };
    let pre_restore_fingerprint = current_working_tree_fingerprint(project_root)?;
    if journal_path.exists() {
        let journal = authenticated_integration_recovery_journal(artifact_dir, &journal_path)?;
        if let Err(error) = validate_integration_recovery_journal_tree(project_root, &journal) {
            return block_interrupted_swarm_integration(
                artifact_dir,
                integration_dir,
                plan,
                state,
                &previous_status,
                "interrupted_integration_journal_drift_before_restore",
                json!({"detail": error.to_string()}),
                json_output,
            );
        }
    } else if pre_restore_fingerprint != checkpoint.expected_fingerprint {
        return block_interrupted_swarm_integration(
            artifact_dir,
            integration_dir,
            plan,
            state,
            &previous_status,
            "interrupted_integration_tree_drift_before_restore",
            json!({
                "expected": checkpoint.expected_fingerprint,
                "actual": pre_restore_fingerprint,
            }),
            json_output,
        );
    }
    let restore_error = restore_verified_integration_checkpoint_with_journal(
        artifact_dir,
        project_root,
        integration_dir,
        plan,
        &state,
        &checkpoint,
    )
    .err()
    .map(|error| error.to_string());
    let actual_fingerprint = (|| -> Result<String> {
        let manifest = file_manifest(project_root)?;
        let git_state = git_state_snapshot(project_root)?;
        working_tree_fingerprint(&git_state, &manifest)
    })()
    .map_err(|error| error.to_string());
    let expected_fingerprint = plan["baseline"]["working_tree_fingerprint"]
        .as_str()
        .unwrap_or_default();
    let rolled_back = cleanup_error.is_none()
        && restore_error.is_none()
        && actual_fingerprint.as_deref() == Ok(expected_fingerprint);
    let status = if rolled_back {
        "rolled_back"
    } else {
        "rollback_failed"
    };
    state["status"] = json!(status);
    state["current_task_id"] = Value::Null;
    state["failure"] = if rolled_back {
        json!({
            "reason": "interrupted_integration_recovered",
            "previous_status": previous_status,
        })
    } else {
        json!({
            "reason": "interrupted_integration_recovery_failed",
            "previous_status": previous_status,
            "cleanup_error": cleanup_error,
            "restore_error": restore_error,
            "expected": expected_fingerprint,
            "actual": actual_fingerprint.ok(),
        })
    };
    state["next_action"] = json!(if rolled_back {
        "rerun_swarm_integrate_apply"
    } else {
        "manual_recovery_required"
    });
    state["updated_at_ms"] = json!(now_ms());
    write_json_atomic(&integration_dir.join("state.json"), &state)?;
    append_swarm_integration_event(
        artifact_dir,
        WorkflowEventKind::SwarmIntegrationRolledBack,
        plan["integration_id"].as_str().unwrap_or("unknown"),
        plan["dispatch_id"].as_str().unwrap_or("unknown"),
        status,
        None,
    )?;
    format_swarm_integration_result(state, json_output)
}

fn block_interrupted_swarm_integration(
    artifact_dir: &Path,
    integration_dir: &Path,
    plan: &Value,
    mut state: Value,
    previous_status: &str,
    reason: &str,
    detail: Value,
    json_output: bool,
) -> Result<CommandResult> {
    state["status"] = json!("recovery_blocked");
    state["current_task_id"] = Value::Null;
    state["failure"] = json!({
        "reason": reason,
        "previous_status": previous_status,
        "detail": detail,
    });
    state["next_action"] = json!("manual_recovery_required");
    state["updated_at_ms"] = json!(now_ms());
    write_json_atomic(&integration_dir.join("state.json"), &state)?;
    append_swarm_integration_event(
        artifact_dir,
        WorkflowEventKind::SwarmIntegrationBlocked,
        plan["integration_id"].as_str().unwrap_or("unknown"),
        plan["dispatch_id"].as_str().unwrap_or("unknown"),
        "recovery_blocked",
        None,
    )?;
    format_swarm_integration_result(state, json_output)
}

fn finish_failed_swarm_integration(
    artifact_dir: &Path,
    integration_dir: &Path,
    project_root: &Path,
    plan: &Value,
    integrated_task_ids: Vec<String>,
    verified_task_ids: Vec<String>,
    commands: Vec<Value>,
    failure: Value,
    json_output: bool,
) -> Result<CommandResult> {
    let restore_error = (|| -> Result<()> {
        let state_path = integration_dir.join("state.json");
        let mut state: Value = read_json_file(&state_path)?;
        state["status"] = json!("rolling_back");
        state["current_task_id"] = Value::Null;
        state["recovery_journal_path"] = json!("recovery/journal.json");
        state["next_action"] = json!("recover_checkpoint");
        state["updated_at_ms"] = json!(now_ms());
        write_json_atomic(&state_path, &state)?;
        let checkpoint = verify_integration_checkpoint_authentication(
            artifact_dir,
            integration_dir,
            project_root,
            plan,
            &state,
        )?;
        restore_verified_integration_checkpoint_with_journal(
            artifact_dir,
            project_root,
            integration_dir,
            plan,
            &state,
            &checkpoint,
        )
    })()
    .err();
    let manifest = file_manifest(project_root)?;
    let git_state = git_state_snapshot(project_root)?;
    let fingerprint = working_tree_fingerprint(&git_state, &manifest)?;
    let expected = plan["baseline"]["working_tree_fingerprint"]
        .as_str()
        .unwrap_or_default();
    let rolled_back = restore_error.is_none() && fingerprint == expected;
    let status = if rolled_back {
        "rolled_back"
    } else {
        "rollback_failed"
    };
    let next_action = if rolled_back {
        "fix_and_redispatch"
    } else {
        "manual_recovery_required"
    };
    let packet = json!({
        "schema": "kiana.swarm-integration-packet.v1",
        "integration_id": plan["integration_id"],
        "workflow_id": plan["workflow_id"],
        "run_id": plan["run_id"],
        "dispatch_id": plan["dispatch_id"],
        "status": status,
        "integrated_task_ids": integrated_task_ids,
        "rejected_task_ids": [],
        "commands": commands,
        "verification_packet_path": Value::Null,
        "rollback": {
            "restored": rolled_back,
            "recovery_journal_path": "recovery/journal.json",
            "expected_fingerprint": expected,
            "actual_fingerprint": fingerprint,
            "error": restore_error.map(|error| error.to_string()),
        },
        "failure": failure,
        "verified_task_ids_before_failure": verified_task_ids,
        "automatic_commit": false,
        "automatic_push": false,
        "automatic_merge": false,
        "automatic_deploy": false,
        "created_at_ms": now_ms(),
        "next_action": next_action,
    });
    write_json_atomic(&integration_dir.join("packet.json"), &packet)?;
    let state = integration_state_value(
        plan,
        status,
        Vec::new(),
        Vec::new(),
        None,
        Some(failure),
        next_action,
    );
    write_json_atomic(&integration_dir.join("state.json"), &state)?;
    append_swarm_integration_event(
        artifact_dir,
        WorkflowEventKind::SwarmIntegrationRolledBack,
        plan["integration_id"].as_str().unwrap_or("unknown"),
        plan["dispatch_id"].as_str().unwrap_or("unknown"),
        status,
        Some("packet.json"),
    )?;
    format_swarm_integration_result(packet, json_output)
}

fn append_swarm_integration_event(
    artifact_dir: &Path,
    kind: WorkflowEventKind,
    integration_id: &str,
    dispatch_id: &str,
    status: &str,
    packet_path: Option<&str>,
) -> Result<()> {
    let integration_event_id = format!("{integration_id}:{status}");
    let integration_id_for_event = integration_id.to_string();
    let integration_event_id_for_event = integration_event_id.clone();
    let dispatch_id_for_event = dispatch_id.to_string();
    let status_for_event = status.to_string();
    let packet_path_for_event = packet_path.map(str::to_string);
    let _ = kiana_tasks::append_workflow_event_idempotent_with_unique_data_value(
        artifact_dir,
        kind,
        "swarm_integration",
        "integration_event_id",
        integration_event_id,
        move |sequence, at_ms| {
            json!({
                "integration_event_id": integration_event_id_for_event,
                "integration_id": integration_id_for_event,
                "dispatch_id": dispatch_id_for_event,
                "status": status_for_event,
                "packet_path": packet_path_for_event,
                "sequence": sequence,
                "created_at_ms": at_ms,
            })
        },
    )?;
    Ok(())
}

fn append_swarm_integration_started_event(
    artifact_dir: &Path,
    plan: &Value,
    checkpoint_path: &str,
    checkpoint_sha256: &str,
    working_tree_fingerprint: &str,
) -> Result<()> {
    let integration_id = plan["integration_id"]
        .as_str()
        .ok_or_else(|| anyhow!("integration plan id is missing"))?
        .to_string();
    let integration_event_id = format!("{integration_id}:applying");
    let event_integration_event_id = integration_event_id.clone();
    let event_integration_id = integration_id.clone();
    let event_dispatch_id = plan["dispatch_id"].clone();
    let event_plan_sha256 = plan["plan_sha256"].clone();
    let event_checkpoint_path = checkpoint_path.to_string();
    let event_checkpoint_sha256 = checkpoint_sha256.to_string();
    let event_fingerprint = working_tree_fingerprint.to_string();
    let _ = kiana_tasks::append_workflow_event_idempotent_with_unique_data_value(
        artifact_dir,
        WorkflowEventKind::SwarmIntegrationStarted,
        "swarm_integration",
        "integration_event_id",
        integration_event_id,
        move |sequence, at_ms| {
            json!({
                "integration_event_id": event_integration_event_id,
                "integration_id": event_integration_id,
                "dispatch_id": event_dispatch_id,
                "status": "applying",
                "packet_path": Value::Null,
                "plan_sha256": event_plan_sha256,
                "checkpoint_path": event_checkpoint_path,
                "checkpoint_sha256": event_checkpoint_sha256,
                "working_tree_fingerprint": event_fingerprint,
                "sequence": sequence,
                "created_at_ms": at_ms,
            })
        },
    )?;
    Ok(())
}

fn append_swarm_worker_applied_event(
    artifact_dir: &Path,
    plan: &Value,
    task_id: &str,
    applied_task_ids: &[String],
    checkpoint_path: &str,
    checkpoint_sha256: &str,
    working_tree_fingerprint: &str,
) -> Result<()> {
    let integration_id = plan["integration_id"]
        .as_str()
        .ok_or_else(|| anyhow!("integration plan id is missing"))?
        .to_string();
    let worker_apply_id = format!("{integration_id}:{task_id}:applied");
    let event_worker_apply_id = worker_apply_id.clone();
    let event_integration_id = integration_id;
    let event_dispatch_id = plan["dispatch_id"].clone();
    let event_task_id = task_id.to_string();
    let event_plan_sha256 = plan["plan_sha256"].clone();
    let event_applied_task_ids = applied_task_ids.to_vec();
    let event_checkpoint_path = checkpoint_path.to_string();
    let event_checkpoint_sha256 = checkpoint_sha256.to_string();
    let event_fingerprint = working_tree_fingerprint.to_string();
    let _ = kiana_tasks::append_workflow_event_idempotent_with_unique_data_value(
        artifact_dir,
        WorkflowEventKind::SwarmWorkerApplied,
        "swarm_integration",
        "worker_apply_id",
        worker_apply_id,
        move |sequence, at_ms| {
            json!({
                "worker_apply_id": event_worker_apply_id,
                "integration_id": event_integration_id,
                "dispatch_id": event_dispatch_id,
                "task_id": event_task_id,
                "plan_sha256": event_plan_sha256,
                "checkpoint_path": event_checkpoint_path,
                "checkpoint_sha256": event_checkpoint_sha256,
                "working_tree_fingerprint": event_fingerprint,
                "applied_task_ids": event_applied_task_ids,
                "sequence": sequence,
                "created_at_ms": at_ms,
            })
        },
    )?;
    Ok(())
}

fn append_swarm_worker_integrated_event(
    artifact_dir: &Path,
    integration_id: &str,
    dispatch_id: &str,
    task_id: &str,
) -> Result<()> {
    let unique = format!("{integration_id}:{task_id}");
    let integration_id_for_event = integration_id.to_string();
    let dispatch_id_for_event = dispatch_id.to_string();
    let task_id_for_event = task_id.to_string();
    let _ = kiana_tasks::append_workflow_event_idempotent_with_unique_data_value(
        artifact_dir,
        WorkflowEventKind::SwarmWorkerIntegrated,
        "swarm_integration",
        "worker_integration_id",
        unique.clone(),
        move |sequence, at_ms| {
            json!({
                "worker_integration_id": unique,
                "integration_id": integration_id_for_event,
                "dispatch_id": dispatch_id_for_event,
                "task_id": task_id_for_event,
                "status": "integrated",
                "sequence": sequence,
                "created_at_ms": at_ms,
            })
        },
    )?;
    Ok(())
}

fn format_swarm_integration_result(value: Value, json_output: bool) -> Result<CommandResult> {
    if json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&value)?));
    }
    Ok(CommandResult::text(format!(
        "Bounded Swarm integration\ndispatch_id: {}\nstatus: {}\nnext_action: {}",
        value["dispatch_id"].as_str().unwrap_or("unknown"),
        value["status"].as_str().unwrap_or("unknown"),
        value["next_action"]
            .as_str()
            .unwrap_or("inspect_integration"),
    )))
}

fn integration_path_conflicts(paths_by_task: &BTreeMap<String, Vec<String>>) -> Vec<Value> {
    let mut owners = BTreeMap::<String, BTreeSet<String>>::new();
    for (task_id, paths) in paths_by_task {
        for path in paths {
            owners
                .entry(path.clone())
                .or_default()
                .insert(task_id.clone());
        }
    }
    owners
        .into_iter()
        .filter(|(_, task_ids)| task_ids.len() > 1)
        .map(|(path, task_ids)| {
            json!({
                "type": "path_conflict",
                "path": path,
                "task_ids": task_ids.into_iter().collect::<Vec<_>>(),
            })
        })
        .collect()
}

fn safe_swarm_relative_path(path: &Path) -> Option<PathBuf> {
    if path.is_absolute() {
        return None;
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => clean.push(part),
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    (!clean.as_os_str().is_empty()).then_some(clean)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SwarmProjectBaseline {
    schema: String,
    workflow_id: String,
    run_id: String,
    dispatch_id: String,
    git_state: GitStateSnapshot,
    manifest: BTreeMap<String, String>,
    manifest_sha256: String,
    working_tree_fingerprint: String,
    captured_at_ms: u64,
}

fn capture_swarm_project_baseline(
    artifact_dir: &Path,
    project_root: &Path,
    dispatch: &SwarmDispatchManifest,
) -> Result<(String, String, SwarmProjectBaseline)> {
    let relative_path = format!("workers/{}/project-baseline.json", dispatch.dispatch_id);
    let project_manifest = file_manifest(project_root)?;
    let project_git_state = git_state_snapshot(project_root)?;
    let project_manifest_sha256 = manifest_sha256(&project_manifest)?;
    let project_fingerprint = working_tree_fingerprint(&project_git_state, &project_manifest)?;
    let event_path = relative_path.clone();
    let event_workflow_id = dispatch.workflow_id.clone();
    let event_run_id = dispatch.run_id.clone();
    let event_dispatch_id = dispatch.dispatch_id.clone();
    let baseline_manifest = project_manifest.clone();
    let baseline_git_state = project_git_state.clone();
    let baseline_manifest_sha256 = project_manifest_sha256.clone();
    let baseline_fingerprint = project_fingerprint.clone();
    kiana_tasks::commit_immutable_artifacts_with_unique_event(
        artifact_dir,
        WorkflowEventKind::SwarmProjectBaselineCaptured,
        "swarm_project_baseline",
        "project_baseline_id",
        dispatch.dispatch_id.clone(),
        move |sequence, captured_at_ms| {
            let baseline = SwarmProjectBaseline {
                schema: "kiana.swarm-project-baseline.v1".to_string(),
                workflow_id: event_workflow_id.clone(),
                run_id: event_run_id.clone(),
                dispatch_id: event_dispatch_id.clone(),
                git_state: baseline_git_state.clone(),
                manifest: baseline_manifest.clone(),
                manifest_sha256: baseline_manifest_sha256.clone(),
                working_tree_fingerprint: baseline_fingerprint.clone(),
                captured_at_ms,
            };
            let contents = serde_json::to_vec_pretty(&baseline)?;
            let project_baseline_sha256 = format!("sha256:{}", sha256_bytes(&contents));
            Ok(WorkflowArtifactBatch {
                artifacts: vec![WorkflowArtifactInput {
                    relative_path: event_path.clone(),
                    contents,
                }],
                event_data: json!({
                    "project_baseline_id": event_dispatch_id,
                    "workflow_id": event_workflow_id,
                    "run_id": event_run_id,
                    "dispatch_id": event_dispatch_id,
                    "project_baseline_path": event_path,
                    "project_baseline_sha256": project_baseline_sha256,
                    "project_manifest_sha256": baseline_manifest_sha256,
                    "project_working_tree_fingerprint": baseline_fingerprint,
                    "sequence": sequence,
                    "captured_at_ms": captured_at_ms
                }),
            })
        },
    )?;
    read_swarm_project_baseline(artifact_dir, &dispatch.dispatch_id)
}

fn read_swarm_project_baseline(
    artifact_dir: &Path,
    dispatch_id: &str,
) -> Result<(String, String, SwarmProjectBaseline)> {
    let events = read_workflow_events(artifact_dir)?;
    let event = events
        .iter()
        .find(|event| {
            event.kind == WorkflowEventKind::SwarmProjectBaselineCaptured
                && event.data["dispatch_id"] == dispatch_id
        })
        .ok_or_else(|| anyhow!("missing swarm project baseline event for {dispatch_id}"))?;
    let relative_path = event.data["project_baseline_path"]
        .as_str()
        .ok_or_else(|| anyhow!("swarm project baseline event is missing path"))?
        .to_string();
    let expected_path = format!("workers/{dispatch_id}/project-baseline.json");
    if relative_path != expected_path {
        return Err(anyhow!("swarm project baseline path mismatch"));
    }
    let expected_sha256 = event.data["project_baseline_sha256"]
        .as_str()
        .ok_or_else(|| anyhow!("swarm project baseline event is missing sha256"))?
        .to_string();
    let contents = fs::read(artifact_dir.join(&relative_path))?;
    let actual_sha256 = format!("sha256:{}", sha256_bytes(&contents));
    if actual_sha256 != expected_sha256 {
        return Err(anyhow!("swarm project baseline event hash mismatch"));
    }
    let baseline: SwarmProjectBaseline = serde_json::from_slice(&contents)?;
    if baseline.schema != "kiana.swarm-project-baseline.v1"
        || baseline.dispatch_id != dispatch_id
        || event.data["workflow_id"] != baseline.workflow_id
        || event.data["run_id"] != baseline.run_id
        || baseline.manifest_sha256 != manifest_sha256(&baseline.manifest)?
        || baseline.working_tree_fingerprint
            != working_tree_fingerprint(&baseline.git_state, &baseline.manifest)?
    {
        return Err(anyhow!("swarm project baseline identity mismatch"));
    }
    Ok((relative_path, expected_sha256, baseline))
}

fn swarm_start(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let args = parse_swarm_execution_args(rest, "start")?;
    let resume = resume_workflow_run(cwd(context), Some(&args.workflow_run_id))?;
    if !resume.resume_status.can_continue() {
        return Err(anyhow!(resume.blocker.unwrap_or_else(|| {
            "workflow run is not ready for worker start".to_string()
        })));
    }
    let artifact_dir = workflow_run_dir(context, &args.workflow_run_id)?;
    let dispatch_path = artifact_dir
        .join("workpackets")
        .join(&args.dispatch_id)
        .join("manifest.json");
    let dispatch: SwarmDispatchManifest = read_json_file(&dispatch_path)
        .with_context(|| format!("failed to read dispatch {}", args.dispatch_id))?;
    if dispatch.dispatch_id != args.dispatch_id {
        return Err(anyhow!("dispatch manifest identity mismatch"));
    }

    let runner = swarm_runner_executable()?;
    let runner_mode = if std::env::var_os("KIANA_SWARM_WORKER_EXECUTABLE").is_some() {
        "fixture_executable"
    } else {
        "kiana_cli"
    };
    let runner_fingerprint = format!("sha256:{}", sha256_file(&runner)?);
    let project_root = cwd(context);
    #[cfg(target_os = "linux")]
    let bwrap = bash_sandbox_bwrap_path(&context.app_state).ok_or_else(|| {
        anyhow!(
            "bounded swarm requires bubblewrap (bwrap) on Linux; configure sandbox.bwrapPath or install bwrap"
        )
    })?;
    let isolation_strategy = select_swarm_isolation_strategy(&project_root);
    let mut launches = Vec::with_capacity(dispatch.task_ids.len());
    for task_id in &dispatch.task_ids {
        let isolation_path = format!(".kiana/swarm-worktrees/{}/{task_id}", dispatch.dispatch_id);
        launches.push(SwarmWorkerLaunchInput {
            task_id: task_id.clone(),
            isolation_strategy: isolation_strategy.clone(),
            isolation_path,
            runner_script: build_swarm_runner_script(
                &artifact_dir,
                &dispatch.dispatch_id,
                task_id,
                &runner,
                runner_mode,
            )?,
        });
    }
    let prepared = prepare_swarm_execution(
        &artifact_dir,
        SwarmExecutionRequest {
            dispatch_id: dispatch.dispatch_id.clone(),
            runner_mode: runner_mode.to_string(),
            runner_fingerprint,
            launches,
        },
    )?;
    let (project_baseline_path, project_baseline_sha256, project_baseline) =
        capture_swarm_project_baseline(&artifact_dir, &project_root, &dispatch)?;

    let mut workers = Vec::with_capacity(prepared.launch_paths.len());
    let mut spawned = 0usize;
    let mut reused = 0usize;
    for launch_path in &prepared.launch_paths {
        let launch: SwarmWorkerLaunch = read_json_file(&artifact_dir.join(launch_path))?;
        let isolation_dir = project_root.join(&launch.isolation_path);
        ensure_swarm_isolation(&project_root, &isolation_dir, &launch.isolation_strategy)?;
        let worker_dir = artifact_dir
            .join(launch_path)
            .parent()
            .ok_or_else(|| anyhow!("worker launch path has no parent"))?
            .to_path_buf();
        let state_path = artifact_dir.join(&launch.state_path);
        let baseline_path = worker_dir.join("baseline.json");
        if !baseline_path.is_file() {
            write_json_atomic(
                &baseline_path,
                &serde_json::to_value(file_manifest(&isolation_dir)?)?,
            )?;
        }
        let existing_state = read_optional_json(&state_path)?;
        let existing_pid = existing_state
            .as_ref()
            .and_then(|state| state.get("pid"))
            .and_then(Value::as_u64)
            .map(|value| value as u32);
        let existing_status = existing_state
            .as_ref()
            .and_then(|state| state.get("status"))
            .and_then(Value::as_str);
        let existing_identity = existing_state
            .as_ref()
            .map(worker_process_identity_from_state)
            .transpose()?
            .flatten();
        let exit_exists = worker_dir.join("exit_code").is_file();
        let (pid, process_identity) = if let Some(pid) = existing_pid {
            if !exit_exists && !existing_status.is_some_and(is_terminal_worker_status) {
                let identity = existing_identity.as_ref().ok_or_else(|| {
                    anyhow!(
                        "process_identity_missing: worker {} cannot be safely reused",
                        launch.worker_id
                    )
                })?;
                let identity_status = check_worker_process_identity(identity);
                if !matches!(
                    identity_status,
                    ProcessIdentityStatus::Verified | ProcessIdentityStatus::Exited
                ) {
                    return Err(anyhow!(
                        "{}: worker {} cannot be safely reused",
                        process_identity_reason(&identity_status),
                        launch.worker_id
                    ));
                }
            }
            reused += 1;
            (pid, existing_identity)
        } else if exit_exists {
            return Err(anyhow!(
                "worker {} has exit artifact but no persisted pid",
                launch.worker_id
            ));
        } else {
            let started = start_swarm_worker_process(
                &artifact_dir,
                &project_root,
                &isolation_dir,
                &worker_dir,
                &state_path,
                &launch,
                &dispatch.workflow_id,
                #[cfg(target_os = "linux")]
                &bwrap,
            )?;
            spawned += 1;
            (started.pid, Some(started.process_identity))
        };
        let identity_status = process_identity
            .as_ref()
            .map(check_worker_process_identity)
            .unwrap_or(ProcessIdentityStatus::Missing);
        let worker_status = if existing_status.is_some_and(is_terminal_worker_status) {
            existing_status.unwrap_or("lost")
        } else if exit_exists || matches!(identity_status, ProcessIdentityStatus::Exited) {
            "completed"
        } else if matches!(identity_status, ProcessIdentityStatus::Verified) {
            "running"
        } else {
            "lost"
        };
        workers.push(json!({
            "worker_id": launch.worker_id,
            "task_id": launch.task_id,
            "status": worker_status,
            "pid": pid,
            "process_identity_status": process_identity_status_label(&identity_status),
            "process_identity_reason": process_identity_reason_value(&identity_status),
            "isolation_strategy": launch.isolation_strategy,
            "isolation_path": launch.isolation_path,
            "state_path": launch.state_path
        }));
    }

    let result = json!({
        "schema": "kiana.swarm-start-result.v1",
        "dispatch_id": dispatch.dispatch_id,
        "status": if workers.iter().any(|worker| worker["status"] == "running") { "running" } else { "terminal" },
        "spawned": spawned,
        "reused": reused,
        "workers": workers,
        "manifest_path": prepared.manifest_path,
        "project_baseline_path": project_baseline_path,
        "project_baseline_sha256": project_baseline_sha256,
        "project_working_tree_fingerprint": project_baseline.working_tree_fingerprint,
        "event_seq": prepared.event_seq,
        "prepared_event_reused": prepared.reused_event,
        "next_action": "run_swarm_monitor"
    });
    if args.json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&result)?));
    }
    Ok(CommandResult::text(format!(
        "Bounded Swarm workers 已启动\ndispatch_id: {}\nspawned: {}\nreused: {}\nnext_action: run_swarm_monitor",
        result["dispatch_id"].as_str().unwrap_or_default(),
        spawned,
        reused
    )))
}

#[cfg(target_os = "linux")]
fn prepare_swarm_worker_writable_files(worker_dir: &Path) -> Result<Vec<PathBuf>> {
    let canonical_worker_dir = fs::canonicalize(worker_dir)?;
    let mut paths = Vec::new();
    for (name, initial) in [
        ("pid", ""),
        ("started_at_ms", ""),
        ("stdout.log", ""),
        ("stderr.log", ""),
        ("exit_code", ""),
        ("finished_at_ms", ""),
        ("telemetry.json", "{}\n"),
        ("worker-result.json", "{}\n"),
    ] {
        let path = worker_dir.join(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(anyhow!(
                        "worker writable output symlink is forbidden: {}",
                        path.display()
                    ));
                }
                if !metadata.is_file() {
                    return Err(anyhow!(
                        "worker writable output must be a regular file: {}",
                        path.display()
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?;
                file.write_all(initial.as_bytes())?;
            }
            Err(error) => return Err(error.into()),
        }
        let canonical_path = fs::canonicalize(&path)?;
        let expected_path = canonical_worker_dir.join(name);
        if canonical_path != expected_path {
            return Err(anyhow!(
                "worker writable output escaped worker directory: {}",
                path.display()
            ));
        }
        paths.push(canonical_path);
    }
    Ok(paths)
}

#[cfg(target_os = "linux")]
fn background_sandboxed_swarm_process(plan: &StrictBwrapPlan) -> ProcessCommand {
    let mut command = ProcessCommand::new("setsid");
    command.arg(&plan.program).args(&plan.args);
    command
}

fn swarm_cancel(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let args = parse_swarm_cancel_args(rest)?;
    let artifact_dir = ready_swarm_artifact_dir(context, &args.workflow_run_id, "cancel")?;
    let manifest: SwarmExecutionManifest = read_json_file(
        &artifact_dir
            .join("workers")
            .join(&args.dispatch_id)
            .join("manifest.json"),
    )?;
    let mut selected = Vec::new();
    for launch_path in &manifest.launch_paths {
        let launch: SwarmWorkerLaunch = read_json_file(&artifact_dir.join(launch_path))?;
        if args
            .task_id
            .as_deref()
            .is_some_and(|task| task != launch.task_id)
        {
            continue;
        }
        let state_path = artifact_dir.join(&launch.state_path);
        let state = read_optional_json(&state_path)?
            .ok_or_else(|| anyhow!("worker {} has no state", launch.worker_id))?;
        let pid = state["pid"].as_u64().map(|value| value as u32);
        let terminal = state["status"]
            .as_str()
            .is_some_and(is_terminal_worker_status);
        let process_identity = if terminal {
            None
        } else {
            Some(require_verified_worker_process_identity(
                &state,
                &launch.worker_id,
                "cancelled",
            )?)
        };
        selected.push((launch, state_path, state, pid, process_identity, terminal));
    }
    if selected.is_empty() {
        return Err(anyhow!("no workers matched cancel request"));
    }

    let mut workers = Vec::with_capacity(selected.len());
    let mut cancelled = 0usize;
    let mut reused = 0usize;
    for (launch, state_path, mut state, pid, process_identity, terminal) in selected {
        if terminal {
            reused += 1;
        } else {
            terminate_worker_process(process_identity.as_ref().ok_or_else(|| {
                anyhow!("process_identity_missing: worker {}", launch.worker_id)
            })?)?;
            let finished_at_ms = now_ms();
            state["status"] = json!("cancelled");
            state["termination_reason"] = json!("cancelled");
            state["finished_at_ms"] = json!(finished_at_ms);
            state["updated_at_ms"] = json!(finished_at_ms);
            write_json_atomic(&state_path, &state)?;
            fs::write(
                state_path
                    .parent()
                    .ok_or_else(|| anyhow!("worker state path has no parent"))?
                    .join("cancelled_at_ms"),
                format!("{finished_at_ms}\n"),
            )?;
            let event_worker_id = launch.worker_id.clone();
            let event_dispatch_id = launch.dispatch_id.clone();
            let event_task_id = launch.task_id.clone();
            kiana_tasks::append_workflow_event_idempotent_with_unique_data_value(
                &artifact_dir,
                WorkflowEventKind::WorkerCancelled,
                "swarm_worker",
                "worker_cancel_id",
                launch.worker_id.clone(),
                move |sequence, at_ms| {
                    json!({
                        "worker_cancel_id": event_worker_id,
                        "worker_id": event_worker_id,
                        "dispatch_id": event_dispatch_id,
                        "task_id": event_task_id,
                        "sequence": sequence,
                        "cancelled_at_ms": at_ms
                    })
                },
            )?;
            cancelled += 1;
        }
        workers.push(json!({
            "worker_id": launch.worker_id,
            "task_id": launch.task_id,
            "status": state["status"],
            "pid": pid,
            "state_path": launch.state_path
        }));
    }
    let result = json!({
        "schema": "kiana.swarm-cancel-result.v1",
        "dispatch_id": manifest.dispatch_id,
        "cancelled": cancelled,
        "reused": reused,
        "workers": workers,
        "next_action": "run_swarm_monitor"
    });
    if args.json_output {
        Ok(CommandResult::text(serde_json::to_string_pretty(&result)?))
    } else {
        Ok(CommandResult::text(format!(
            "Bounded Swarm 已取消\ndispatch_id: {}\ncancelled: {}\nreused: {}",
            manifest.dispatch_id, cancelled, reused
        )))
    }
}

fn swarm_monitor(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let args = parse_swarm_execution_args(rest, "monitor")?;
    let artifact_dir = ready_swarm_artifact_dir(context, &args.workflow_run_id, "monitor")?;
    let manifest: SwarmExecutionManifest = read_json_file(
        &artifact_dir
            .join("workers")
            .join(&args.dispatch_id)
            .join("manifest.json"),
    )?;
    let process_identity_backend = process_identity_backend_capability();
    let project_root = cwd(context);
    #[cfg(target_os = "linux")]
    let bwrap = bash_sandbox_bwrap_path(&context.app_state).ok_or_else(|| {
        anyhow!(
            "bounded swarm requires bubblewrap (bwrap) on Linux; configure sandbox.bwrapPath or install bwrap"
        )
    })?;
    let current_project_manifest = file_manifest(&project_root)?;
    let current_project_git_state = git_state_snapshot(&project_root)?;
    let current_project_fingerprint =
        working_tree_fingerprint(&current_project_git_state, &current_project_manifest)?;
    let (
        project_baseline_path,
        project_baseline_sha256,
        project_start_fingerprint,
        project_baseline_error,
    ) = match read_swarm_project_baseline(&artifact_dir, &args.dispatch_id) {
        Ok((path, sha256, baseline)) => (path, sha256, baseline.working_tree_fingerprint, None),
        Err(error) => (
            format!("workers/{}/project-baseline.json", args.dispatch_id),
            "invalid".to_string(),
            "invalid".to_string(),
            Some(error.to_string()),
        ),
    };
    let project_violation_reason = if project_baseline_error.is_some() {
        Some("project_baseline_evidence_invalid")
    } else if current_project_fingerprint != project_start_fingerprint {
        Some("main_tree_changed_during_worker_run")
    } else {
        None
    };
    let mut workers = Vec::new();
    let mut created = 0usize;
    let mut reused = 0usize;
    let mut running = 0usize;
    for launch_path in &manifest.launch_paths {
        let launch: SwarmWorkerLaunch = read_json_file(&artifact_dir.join(launch_path))?;
        let worker_dir = artifact_dir
            .join(launch_path)
            .parent()
            .unwrap()
            .to_path_buf();
        let state_path = artifact_dir.join(&launch.state_path);
        let mut state = read_optional_json(&state_path)?
            .ok_or_else(|| anyhow!("worker {} has no state", launch.worker_id))?;
        let active_attempt = state["attempt"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(launch.attempt);
        let active_launch = retry_swarm_worker_launch(&launch, active_attempt);
        let pid = state["pid"].as_u64().map(|value| value as u32);
        let process_identity = worker_process_identity_from_state(&state)?;
        let process_identity_status = process_identity
            .as_ref()
            .map(check_worker_process_identity)
            .unwrap_or(ProcessIdentityStatus::Missing);
        let started_at_ms = state["started_at_ms"]
            .as_u64()
            .unwrap_or(launch.created_at_ms);
        let output_bytes =
            file_size(&worker_dir.join("stdout.log")) + file_size(&worker_dir.join("stderr.log"));
        let isolation_dir = project_root.join(&launch.isolation_path);
        let baseline: BTreeMap<String, String> = read_json_file(&worker_dir.join("baseline.json"))?;
        let current = file_manifest(&isolation_dir)?;
        let baseline_manifest_sha256 = manifest_sha256(&baseline)?;
        let isolation_manifest_sha256 = manifest_sha256(&current)?;
        let changed_files = manifest_diff(&baseline, &current);
        let packet: SwarmWorkPacket = read_json_file(&artifact_dir.join(&launch.workpacket_path))?;
        let scope_deviations = changed_files
            .iter()
            .filter(|path| !path_allowed_by_packet(path, &packet))
            .cloned()
            .collect::<Vec<_>>();
        let telemetry = read_worker_telemetry(&worker_dir.join("telemetry.json"))?;
        let commands_run = telemetry.commands_run.clone();
        let exit_code = read_optional_i32(&worker_dir.join("exit_code"))?;
        // The runner writes finished_at_ms immediately before the exit marker.
        // Treat an exit marker without that completion metadata as an
        // incomplete observation so it cannot produce a moving result packet.
        let finished_at_file = read_optional_u64(&worker_dir.join("finished_at_ms"))?;
        let previous_status = state["status"].as_str().unwrap_or("running").to_string();
        let alive = matches!(process_identity_status, ProcessIdentityStatus::Verified);
        let elapsed_ms = now_ms().saturating_sub(started_at_ms);
        let mut reason = state["termination_reason"].as_str().map(str::to_string);
        let status = if previous_status == "cancelled" {
            reason = Some("cancelled".to_string());
            "cancelled".to_string()
        } else if !matches!(
            process_identity_status,
            ProcessIdentityStatus::Verified | ProcessIdentityStatus::Exited
        ) {
            reason = Some(process_identity_reason(&process_identity_status));
            "lost".to_string()
        } else if let Some(project_violation_reason) = project_violation_reason {
            if let Some(identity) = process_identity.as_ref().filter(|_| alive) {
                terminate_worker_process(identity)?;
            }
            reason = Some(project_violation_reason.to_string());
            "scope_violation".to_string()
        } else if !scope_deviations.is_empty() {
            if let Some(identity) = process_identity.as_ref().filter(|_| alive) {
                terminate_worker_process(identity)?;
            }
            reason = Some("scope_violation".to_string());
            "scope_violation".to_string()
        } else if output_bytes > launch.budget.max_output_bytes
            || telemetry.command_count > launch.budget.max_commands
        {
            if let Some(identity) = process_identity.as_ref().filter(|_| alive) {
                terminate_worker_process(identity)?;
            }
            reason = Some("budget_exhausted".to_string());
            "budget_exhausted".to_string()
        } else if elapsed_ms > launch.budget.timeout_seconds.saturating_mul(1000) && alive {
            terminate_worker_process(process_identity.as_ref().ok_or_else(|| {
                anyhow!("process_identity_missing: worker {}", launch.worker_id)
            })?)?;
            reason = Some("timeout".to_string());
            "timeout".to_string()
        } else if let Some(code) = exit_code {
            if finished_at_file.is_some() && code == 0 {
                reason = Some("completed".to_string());
                "completed".to_string()
            } else if finished_at_file.is_some() {
                reason = Some("worker_failed".to_string());
                "failed".to_string()
            } else if alive {
                "running".to_string()
            } else {
                reason = Some("completion_marker_missing".to_string());
                "lost".to_string()
            }
        } else if alive {
            "running".to_string()
        } else {
            reason = Some(process_identity_reason(&process_identity_status));
            "lost".to_string()
        };

        if status == "running" {
            running += 1;
            let health = swarm_worker_health_report(SwarmWorkerHealthInput {
                status: &status,
                termination_reason: None,
                pid,
                attempt: active_launch.attempt,
                max_attempts: launch.budget.max_attempts,
                output_bytes,
                max_output_bytes: launch.budget.max_output_bytes,
                commands_run: telemetry.command_count as usize,
                max_commands: launch.budget.max_commands,
                elapsed_ms,
                timeout_seconds: launch.budget.timeout_seconds,
                process_identity_status: &process_identity_status,
                changed_files: changed_files.len(),
                scope_deviations: scope_deviations.len(),
                project_violation_reason,
                retrying: false,
            });
            workers.push(json!({
                "worker_id": launch.worker_id,
                "task_id": launch.task_id,
                "status": status,
                "pid": pid,
                "attempt": active_launch.attempt,
                "health": health,
                "result_path": Value::Null
            }));
            continue;
        }

        let finished_at_ms = finished_at_file
            .or_else(|| state["finished_at_ms"].as_u64())
            .unwrap_or_else(now_ms);
        let retry_reason = reason.as_deref();
        if should_retry_swarm_worker(
            &status,
            retry_reason,
            &active_launch,
            &changed_files,
            &scope_deviations,
            project_violation_reason,
        ) {
            archive_swarm_retry_attempt(
                &worker_dir,
                active_launch.attempt,
                &status,
                retry_reason,
                exit_code,
                output_bytes,
                finished_at_ms,
                &process_identity_status,
            )?;
            let retry_launch = retry_swarm_worker_launch(&launch, active_launch.attempt + 1);
            let started = start_swarm_worker_process(
                &artifact_dir,
                &project_root,
                &isolation_dir,
                &worker_dir,
                &state_path,
                &retry_launch,
                &manifest.workflow_id,
                #[cfg(target_os = "linux")]
                &bwrap,
            )?;
            running += 1;
            let health = swarm_worker_health_report(SwarmWorkerHealthInput {
                status: "running",
                termination_reason: Some("retrying_after_worker_failed"),
                pid: Some(started.pid),
                attempt: retry_launch.attempt,
                max_attempts: retry_launch.budget.max_attempts,
                output_bytes: 0,
                max_output_bytes: retry_launch.budget.max_output_bytes,
                commands_run: 0,
                max_commands: retry_launch.budget.max_commands,
                elapsed_ms: 0,
                timeout_seconds: retry_launch.budget.timeout_seconds,
                process_identity_status: &ProcessIdentityStatus::Verified,
                changed_files: 0,
                scope_deviations: 0,
                project_violation_reason: None,
                retrying: true,
            });
            workers.push(json!({
                "worker_id": retry_launch.worker_id,
                "task_id": retry_launch.task_id,
                "status": "running",
                "pid": started.pid,
                "attempt": retry_launch.attempt,
                "retrying": true,
                "previous_status": status,
                "previous_termination_reason": reason,
                "health": health,
                "result_path": Value::Null
            }));
            continue;
        }
        let result_path = format!(
            "workers/{}/{}/result.json",
            launch.dispatch_id, launch.task_id
        );
        state["status"] = json!(status);
        state["termination_reason"] = json!(reason);
        state["process_identity_status"] =
            json!(process_identity_status_label(&process_identity_status));
        state["process_identity_reason"] = process_identity_reason_value(&process_identity_status);
        state["finished_at_ms"] = json!(finished_at_ms);
        state["exit_code"] = json!(exit_code);
        state["output_bytes"] = json!(output_bytes);
        state["result_path"] = json!(result_path);
        state["updated_at_ms"] = json!(finished_at_ms);
        write_json_atomic(&state_path, &state)?;
        let notes = if telemetry.provided {
            Vec::<String>::new()
        } else {
            vec!["runner telemetry was not provided".to_string()]
        };
        let packet_value = json!({
            "schema": "kiana.swarm-result-packet.v1",
            "workflow_id": manifest.workflow_id,
            "run_id": manifest.run_id,
            "worker_id": launch.worker_id,
            "dispatch_id": launch.dispatch_id,
            "task_id": launch.task_id,
            "status": status,
            "termination_reason": reason,
            "exit_code": exit_code,
            "changed_files": changed_files,
            "commands_run": commands_run,
            "telemetry": telemetry.to_value(),
            "acceptance_evidence": [],
            "scope_deviations": scope_deviations,
            "baseline_manifest_sha256": baseline_manifest_sha256,
            "isolation_manifest_sha256": isolation_manifest_sha256,
            "project_baseline_path": project_baseline_path,
            "project_baseline_sha256": project_baseline_sha256,
            "project_start_fingerprint": project_start_fingerprint,
            "project_current_fingerprint": current_project_fingerprint,
            "project_baseline_error": project_baseline_error,
            "stdout_path": format!("workers/{}/{}/stdout.log", launch.dispatch_id, launch.task_id),
            "stderr_path": format!("workers/{}/{}/stderr.log", launch.dispatch_id, launch.task_id),
            "started_at_ms": started_at_ms,
            "finished_at_ms": finished_at_ms,
            "output_bytes": output_bytes,
            "notes": notes,
            "next_action": "swarm_integration_review"
        });
        let event_worker_id = launch.worker_id.clone();
        let event_dispatch_id = launch.dispatch_id.clone();
        let event_task_id = launch.task_id.clone();
        let event_result_path = result_path.clone();
        let packet_bytes = serde_json::to_vec_pretty(&packet_value)?;
        let result_packet_sha256 = format!("sha256:{}", sha256_bytes(&packet_bytes));
        let event_result_packet_sha256 = result_packet_sha256.clone();
        let event_isolation_manifest_sha256 = isolation_manifest_sha256.clone();
        let event_project_baseline_path = project_baseline_path.clone();
        let event_project_baseline_sha256 = project_baseline_sha256.clone();
        let event_project_start_fingerprint = project_start_fingerprint.clone();
        let event_project_current_fingerprint = current_project_fingerprint.clone();
        let commit = kiana_tasks::commit_immutable_artifacts_with_unique_event(
            &artifact_dir,
            WorkflowEventKind::ResultPacketCreated,
            "swarm_result",
            "result_packet_id",
            launch.worker_id.clone(),
            move |sequence, created_at_ms| {
                Ok(WorkflowArtifactBatch {
                    artifacts: vec![WorkflowArtifactInput {
                        relative_path: event_result_path.clone(),
                        contents: packet_bytes.clone(),
                    }],
                    event_data: json!({
                        "result_packet_id": event_worker_id,
                        "worker_id": event_worker_id,
                        "dispatch_id": event_dispatch_id,
                        "task_id": event_task_id,
                        "result_path": event_result_path,
                        "result_packet_sha256": event_result_packet_sha256,
                        "isolation_manifest_sha256": event_isolation_manifest_sha256,
                        "project_baseline_path": event_project_baseline_path,
                        "project_baseline_sha256": event_project_baseline_sha256,
                        "project_start_fingerprint": event_project_start_fingerprint,
                        "project_current_fingerprint": event_project_current_fingerprint,
                        "sequence": sequence,
                        "created_at_ms": created_at_ms
                    }),
                })
            },
        )?;
        if commit.reused_event {
            reused += 1;
        } else {
            created += 1;
        }
        let health = swarm_worker_health_report(SwarmWorkerHealthInput {
            status: &status,
            termination_reason: reason.as_deref(),
            pid,
            attempt: active_launch.attempt,
            max_attempts: launch.budget.max_attempts,
            output_bytes,
            max_output_bytes: launch.budget.max_output_bytes,
            commands_run: telemetry.command_count as usize,
            max_commands: launch.budget.max_commands,
            elapsed_ms,
            timeout_seconds: launch.budget.timeout_seconds,
            process_identity_status: &process_identity_status,
            changed_files: changed_files.len(),
            scope_deviations: scope_deviations.len(),
            project_violation_reason,
            retrying: false,
        });
        workers.push(json!({
            "worker_id": launch.worker_id,
            "task_id": launch.task_id,
            "status": status,
            "pid": pid,
            "attempt": active_launch.attempt,
            "health": health,
            "result_path": result_path,
            "reused_result": commit.reused_event
        }));
    }
    let result = json!({
        "schema": "kiana.swarm-monitor-result.v1",
        "dispatch_id": manifest.dispatch_id,
        "status": if running > 0 { "running" } else { "terminal" },
        "running": running,
        "result_packets_created": created,
        "result_packets_reused": reused,
        "process_identity_backend": process_identity_backend,
        "workers": workers,
        "next_action": if running > 0 { "run_swarm_monitor" } else { "swarm_integration_review" }
    });
    if args.json_output {
        Ok(CommandResult::text(serde_json::to_string_pretty(&result)?))
    } else {
        Ok(CommandResult::text(format!(
            "Bounded Swarm monitor\ndispatch_id: {}\nstatus: {}\ncreated: {}\nreused: {}",
            manifest.dispatch_id,
            result["status"].as_str().unwrap_or_default(),
            created,
            reused
        )))
    }
}

fn swarm_status(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let args = parse_swarm_execution_args(rest, "status")?;
    let resume = resume_workflow_run(cwd(context), Some(&args.workflow_run_id))?;
    if !resume.resume_status.can_continue() {
        return Err(anyhow!(resume.blocker.unwrap_or_else(|| {
            "workflow run is not ready for worker status".to_string()
        })));
    }
    let artifact_dir = workflow_run_dir(context, &args.workflow_run_id)?;
    let manifest_path = artifact_dir
        .join("workers")
        .join(&args.dispatch_id)
        .join("manifest.json");
    let manifest: SwarmExecutionManifest = read_json_file(&manifest_path)?;
    if manifest.dispatch_id != args.dispatch_id {
        return Err(anyhow!("execution manifest identity mismatch"));
    }
    let process_identity_backend = process_identity_backend_capability();
    let mut workers = Vec::with_capacity(manifest.launch_paths.len());
    let mut running = 0usize;
    let mut terminal = 0usize;
    for launch_path in &manifest.launch_paths {
        let launch: SwarmWorkerLaunch = read_json_file(&artifact_dir.join(launch_path))?;
        let worker_dir = artifact_dir
            .join(launch_path)
            .parent()
            .ok_or_else(|| anyhow!("worker launch path has no parent"))?
            .to_path_buf();
        let state = read_optional_json(&artifact_dir.join(&launch.state_path))?;
        let pid = state
            .as_ref()
            .and_then(|value| value.get("pid"))
            .and_then(Value::as_u64)
            .map(|value| value as u32);
        let exit_code = read_optional_i32(&worker_dir.join("exit_code"))?;
        let persisted_status = state
            .as_ref()
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str);
        let process_identity = state
            .as_ref()
            .map(worker_process_identity_from_state)
            .transpose()?
            .flatten();
        let process_identity_status = process_identity
            .as_ref()
            .map(check_worker_process_identity)
            .unwrap_or(ProcessIdentityStatus::Missing);
        let status = if persisted_status.is_some_and(is_terminal_worker_status) {
            persisted_status.unwrap()
        } else {
            match (exit_code, pid) {
                (Some(0), _) => "completed",
                (Some(_), _) => "failed",
                (None, Some(_))
                    if matches!(process_identity_status, ProcessIdentityStatus::Verified) =>
                {
                    "running"
                }
                (None, Some(_)) => "lost",
                (None, None) => "prepared",
            }
        };
        if status == "running" {
            running += 1;
        }
        if is_terminal_worker_status(status) {
            terminal += 1;
        }
        let attempt = state
            .as_ref()
            .and_then(|value| value.get("attempt"))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(launch.attempt);
        let started_at_ms = state
            .as_ref()
            .and_then(|value| value.get("started_at_ms"))
            .and_then(Value::as_u64)
            .unwrap_or(launch.created_at_ms);
        let output_bytes =
            file_size(&worker_dir.join("stdout.log")) + file_size(&worker_dir.join("stderr.log"));
        let telemetry = read_worker_telemetry(&worker_dir.join("telemetry.json"))?;
        let health = swarm_worker_health_report(SwarmWorkerHealthInput {
            status,
            termination_reason: state
                .as_ref()
                .and_then(|value| value.get("termination_reason"))
                .and_then(Value::as_str),
            pid,
            attempt,
            max_attempts: launch.budget.max_attempts,
            output_bytes,
            max_output_bytes: launch.budget.max_output_bytes,
            commands_run: telemetry.command_count as usize,
            max_commands: launch.budget.max_commands,
            elapsed_ms: now_ms().saturating_sub(started_at_ms),
            timeout_seconds: launch.budget.timeout_seconds,
            process_identity_status: &process_identity_status,
            changed_files: 0,
            scope_deviations: 0,
            project_violation_reason: None,
            retrying: false,
        });
        workers.push(json!({
            "worker_id": launch.worker_id,
            "task_id": launch.task_id,
            "status": status,
            "pid": pid,
            "exit_code": exit_code,
            "attempt": attempt,
            "health": health,
            "process_identity_status": process_identity_status_label(&process_identity_status),
            "process_identity_reason": process_identity_reason_value(&process_identity_status),
            "isolation_strategy": launch.isolation_strategy,
            "isolation_path": launch.isolation_path,
            "state_path": launch.state_path,
            "stdout_path": format!("workers/{}/{}/stdout.log", launch.dispatch_id, launch.task_id),
            "stderr_path": format!("workers/{}/{}/stderr.log", launch.dispatch_id, launch.task_id)
        }));
    }
    let result = json!({
        "schema": "kiana.swarm-status.v1",
        "dispatch_id": manifest.dispatch_id,
        "status": if running > 0 { "running" } else if terminal == workers.len() { "terminal" } else { "prepared" },
        "running": running,
        "terminal": terminal,
        "process_identity_backend": process_identity_backend,
        "workers": workers,
        "next_action": if running > 0 { "run_swarm_monitor" } else { "swarm_integration_review" }
    });
    if args.json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&result)?));
    }
    Ok(CommandResult::text(format!(
        "Bounded Swarm 状态\ndispatch_id: {}\nstatus: {}\nrunning: {}\nterminal: {}",
        result["dispatch_id"].as_str().unwrap_or_default(),
        result["status"].as_str().unwrap_or_default(),
        running,
        terminal
    )))
}

fn swarm_dispatch(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let args = parse_swarm_dispatch_args(rest)?;
    let resume = resume_workflow_run(cwd(context), Some(&args.workflow_run_id))?;
    if !resume.resume_status.can_continue() {
        return Err(anyhow!(resume.blocker.unwrap_or_else(|| {
            "workflow run is not ready for dispatch".to_string()
        })));
    }

    let explicit_task_list = args.task_list_id.as_deref();
    let selected_task_list = task_list_id(context, explicit_task_list);
    let tasks = load_tasks(context, explicit_task_list)?;
    let board = build_project_board_at_root(cwd(context), selected_task_list, &tasks)?;
    let plan = build_swarm_plan(&board, args.max_workers)?;
    let artifact_dir = workflow_run_dir(context, &args.workflow_run_id)?;
    let result = persist_swarm_dispatch(&artifact_dir, &plan, args.budget)?;
    if args.json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&result)?));
    }

    Ok(CommandResult::text(format!(
        "Bounded Swarm dispatch 已持久化\ndispatch_id: {}\nstatus: {}\nexecution_mode: {}\nmanifest: {}\nworkpackets: {}\nevent_seq: {}\nreused_event: {}\nnext_action: {}",
        result.dispatch_id,
        result.status,
        result.execution_mode,
        result.manifest_path,
        result.workpacket_paths.len(),
        result.event_seq,
        result.reused_event,
        result.next_action,
    )))
}

fn swarm_plan(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let args = parse_swarm_plan_args(rest)?;
    let explicit_task_list = args.task_list_id.as_deref();
    let selected_task_list = task_list_id(context, explicit_task_list);
    let tasks = load_tasks(context, explicit_task_list)?;
    let board = build_project_board_at_root(cwd(context), selected_task_list, &tasks)?;
    let plan = build_swarm_plan(&board, args.max_workers)?;
    if args.json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&plan)?));
    }
    let mut lines = vec![format!(
        "Bounded Swarm 预派发\n状态：{}\nReady tasks：{}\n计划派发：{}\n跳过：{}\n最大 workers：{}\n下一步：{}",
        plan.status,
        plan.summary.ready_tasks,
        plan.summary.dispatched,
        plan.summary.skipped,
        plan.max_workers,
        plan.next_action,
    )];
    if !plan.assignments.is_empty() {
        lines.push("Assignments:".to_string());
        lines.extend(plan.assignments.iter().map(|assignment| {
            format!(
                "- {} -> {} ({})",
                assignment.task_id, assignment.worker_type, assignment.workpacket.id
            )
        }));
    }
    if !plan.skipped.is_empty() {
        lines.push("Skipped:".to_string());
        lines.extend(plan.skipped.iter().map(|item| {
            let conflicts = if item.conflicts_with.is_empty() {
                "none".to_string()
            } else {
                item.conflicts_with.join(",")
            };
            format!(
                "- {}: {} (conflicts_with={})",
                item.task_id, item.reason, conflicts
            )
        }));
    }
    if !plan.path_locks.is_empty() {
        lines.push("Path locks:".to_string());
        lines.extend(plan.path_locks.iter().map(|lock| {
            if lock.path == lock.resolved_path {
                format!("- {}: {} ({})", lock.task_id, lock.path, lock.mode)
            } else {
                format!(
                    "- {}: {} -> {} ({})",
                    lock.task_id, lock.path, lock.resolved_path, lock.mode
                )
            }
        }));
    }
    lines.push(
        "usage: kiana tasks swarm plan --json --max-workers <2..32> [task_list_id]".to_string(),
    );
    Ok(CommandResult::text(lines.join("\n")))
}

#[derive(Debug)]
struct SwarmPlanArgs {
    json_output: bool,
    max_workers: usize,
    task_list_id: Option<String>,
}

fn parse_swarm_plan_args(rest: &str) -> Result<SwarmPlanArgs> {
    let tokens = rest.split_whitespace().collect::<Vec<_>>();
    let mut json_output = false;
    let mut max_workers = None;
    let mut task_list_id = None;
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index] {
            "--json" | "-j" => json_output = true,
            "--max-workers" => {
                if max_workers.is_some() {
                    return Err(anyhow!(
                        "duplicate option --max-workers\n\n{}",
                        swarm_usage()
                    ));
                }
                index += 1;
                max_workers = Some(parse_max_workers(
                    tokens.get(index).copied(),
                    "--max-workers",
                )?);
            }
            token if token.starts_with("--max-workers=") => {
                if max_workers.is_some() {
                    return Err(anyhow!(
                        "duplicate option --max-workers\n\n{}",
                        swarm_usage()
                    ));
                }
                let value = token
                    .split_once('=')
                    .map(|(_, value)| value)
                    .filter(|value| !value.is_empty());
                max_workers = Some(parse_max_workers(value, "--max-workers")?);
            }
            token if token.starts_with('-') => {
                return Err(anyhow!(
                    "unknown swarm plan option '{}'

{}",
                    token,
                    swarm_usage()
                ));
            }
            token if task_list_id.is_none() => task_list_id = Some(token.to_string()),
            _ => return Err(anyhow!(swarm_usage())),
        }
        index += 1;
    }
    Ok(SwarmPlanArgs {
        json_output,
        max_workers: max_workers.unwrap_or(2),
        task_list_id,
    })
}

fn parse_max_workers(value: Option<&str>, option: &str) -> Result<usize> {
    let value = value
        .filter(|value| !value.trim().is_empty() && !value.starts_with('-'))
        .ok_or_else(|| anyhow!("missing value for {option}\n\n{}", swarm_usage()))?;
    let parsed = value
        .parse::<usize>()
        .map_err(|_| anyhow!("{option} must be an integer between 2 and 32"))?;
    if !(2..=32).contains(&parsed) {
        return Err(anyhow!("{option} must be between 2 and 32"));
    }
    Ok(parsed)
}

#[derive(Debug)]
struct SwarmDispatchArgs {
    json_output: bool,
    workflow_run_id: String,
    max_workers: usize,
    budget: SwarmWorkerBudget,
    task_list_id: Option<String>,
}

#[derive(Debug)]
struct SwarmExecutionArgs {
    json_output: bool,
    workflow_run_id: String,
    dispatch_id: String,
}

#[derive(Debug)]
struct SwarmCancelArgs {
    json_output: bool,
    workflow_run_id: String,
    dispatch_id: String,
    task_id: Option<String>,
}

fn parse_swarm_cancel_args(rest: &str) -> Result<SwarmCancelArgs> {
    let tokens = rest.split_whitespace().collect::<Vec<_>>();
    let mut json_output = false;
    let mut workflow_run_id = None;
    let mut dispatch_id = None;
    let mut task_id = None;
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        match token {
            "--json" | "-j" => json_output = true,
            "--workflow" | "--dispatch" | "--task" => {
                index += 1;
                let value = required_option_value(tokens.get(index).copied(), token)?;
                match token {
                    "--workflow" => {
                        ensure_option_absent(&workflow_run_id, token)?;
                        workflow_run_id = Some(value);
                    }
                    "--dispatch" => {
                        ensure_option_absent(&dispatch_id, token)?;
                        dispatch_id = Some(value);
                    }
                    _ => {
                        ensure_option_absent(&task_id, token)?;
                        task_id = Some(value);
                    }
                }
            }
            token if token.starts_with("--workflow=") => {
                ensure_option_absent(&workflow_run_id, "--workflow")?;
                workflow_run_id = Some(required_inline_value(token, "--workflow")?);
            }
            token if token.starts_with("--dispatch=") => {
                ensure_option_absent(&dispatch_id, "--dispatch")?;
                dispatch_id = Some(required_inline_value(token, "--dispatch")?);
            }
            token if token.starts_with("--task=") => {
                ensure_option_absent(&task_id, "--task")?;
                task_id = Some(required_inline_value(token, "--task")?);
            }
            token if token.starts_with('-') => {
                return Err(anyhow!(
                    "unknown swarm cancel option '{token}'\n\n{}",
                    swarm_usage()
                ));
            }
            _ => return Err(anyhow!(swarm_usage())),
        }
        index += 1;
    }
    Ok(SwarmCancelArgs {
        json_output,
        workflow_run_id: workflow_run_id
            .ok_or_else(|| anyhow!("missing option --workflow\n\n{}", swarm_usage()))?,
        dispatch_id: dispatch_id
            .ok_or_else(|| anyhow!("missing option --dispatch\n\n{}", swarm_usage()))?,
        task_id,
    })
}

fn parse_swarm_execution_args(rest: &str, action: &str) -> Result<SwarmExecutionArgs> {
    let tokens = rest.split_whitespace().collect::<Vec<_>>();
    let mut json_output = false;
    let mut workflow_run_id = None;
    let mut dispatch_id = None;
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        match token {
            "--json" | "-j" => json_output = true,
            "--workflow" => {
                ensure_option_absent(&workflow_run_id, "--workflow")?;
                index += 1;
                workflow_run_id = Some(required_option_value(
                    tokens.get(index).copied(),
                    "--workflow",
                )?);
            }
            "--dispatch" => {
                ensure_option_absent(&dispatch_id, "--dispatch")?;
                index += 1;
                dispatch_id = Some(required_option_value(
                    tokens.get(index).copied(),
                    "--dispatch",
                )?);
            }
            token if token.starts_with("--workflow=") => {
                ensure_option_absent(&workflow_run_id, "--workflow")?;
                workflow_run_id = Some(required_inline_value(token, "--workflow")?);
            }
            token if token.starts_with("--dispatch=") => {
                ensure_option_absent(&dispatch_id, "--dispatch")?;
                dispatch_id = Some(required_inline_value(token, "--dispatch")?);
            }
            token if token.starts_with('-') => {
                return Err(anyhow!(
                    "unknown swarm {action} option '{token}'\n\n{}",
                    swarm_usage()
                ));
            }
            _ => return Err(anyhow!(swarm_usage())),
        }
        index += 1;
    }
    Ok(SwarmExecutionArgs {
        json_output,
        workflow_run_id: workflow_run_id
            .ok_or_else(|| anyhow!("missing option --workflow\n\n{}", swarm_usage()))?,
        dispatch_id: dispatch_id
            .ok_or_else(|| anyhow!("missing option --dispatch\n\n{}", swarm_usage()))?,
    })
}

fn parse_swarm_dispatch_args(rest: &str) -> Result<SwarmDispatchArgs> {
    let tokens = rest.split_whitespace().collect::<Vec<_>>();
    let mut json_output = false;
    let mut workflow_run_id = None;
    let mut max_workers = None;
    let mut max_attempts = None;
    let mut max_commands = None;
    let mut timeout_seconds = None;
    let mut max_output_bytes = None;
    let mut task_list_id = None;
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        match token {
            "--json" | "-j" => json_output = true,
            "--workflow" => {
                ensure_option_absent(&workflow_run_id, "--workflow")?;
                index += 1;
                workflow_run_id = Some(required_option_value(
                    tokens.get(index).copied(),
                    "--workflow",
                )?);
            }
            "--max-workers" => {
                ensure_option_absent(&max_workers, "--max-workers")?;
                index += 1;
                max_workers = Some(parse_max_workers(
                    tokens.get(index).copied(),
                    "--max-workers",
                )?);
            }
            "--max-attempts" => {
                ensure_option_absent(&max_attempts, "--max-attempts")?;
                index += 1;
                max_attempts =
                    Some(
                        parse_bounded_u64(tokens.get(index).copied(), "--max-attempts", 1, 3)?
                            as u32,
                    );
            }
            "--max-commands" => {
                ensure_option_absent(&max_commands, "--max-commands")?;
                index += 1;
                max_commands =
                    Some(
                        parse_bounded_u64(tokens.get(index).copied(), "--max-commands", 1, 100)?
                            as u32,
                    );
            }
            "--timeout-seconds" => {
                ensure_option_absent(&timeout_seconds, "--timeout-seconds")?;
                index += 1;
                timeout_seconds = Some(parse_bounded_u64(
                    tokens.get(index).copied(),
                    "--timeout-seconds",
                    1,
                    86_400,
                )?);
            }
            "--max-output-bytes" => {
                ensure_option_absent(&max_output_bytes, "--max-output-bytes")?;
                index += 1;
                max_output_bytes = Some(parse_bounded_u64(
                    tokens.get(index).copied(),
                    "--max-output-bytes",
                    1_024,
                    100 * 1024 * 1024,
                )?);
            }
            token if token.starts_with("--workflow=") => {
                ensure_option_absent(&workflow_run_id, "--workflow")?;
                workflow_run_id = Some(required_inline_value(token, "--workflow")?);
            }
            token if token.starts_with("--max-workers=") => {
                ensure_option_absent(&max_workers, "--max-workers")?;
                max_workers = Some(parse_max_workers(
                    Some(required_inline_value(token, "--max-workers")?.as_str()),
                    "--max-workers",
                )?);
            }
            token if token.starts_with("--max-attempts=") => {
                ensure_option_absent(&max_attempts, "--max-attempts")?;
                let value = required_inline_value(token, "--max-attempts")?;
                max_attempts =
                    Some(parse_bounded_u64(Some(&value), "--max-attempts", 1, 3)? as u32);
            }
            token if token.starts_with("--max-commands=") => {
                ensure_option_absent(&max_commands, "--max-commands")?;
                let value = required_inline_value(token, "--max-commands")?;
                max_commands =
                    Some(parse_bounded_u64(Some(&value), "--max-commands", 1, 100)? as u32);
            }
            token if token.starts_with("--timeout-seconds=") => {
                ensure_option_absent(&timeout_seconds, "--timeout-seconds")?;
                let value = required_inline_value(token, "--timeout-seconds")?;
                timeout_seconds = Some(parse_bounded_u64(
                    Some(&value),
                    "--timeout-seconds",
                    1,
                    86_400,
                )?);
            }
            token if token.starts_with("--max-output-bytes=") => {
                ensure_option_absent(&max_output_bytes, "--max-output-bytes")?;
                let value = required_inline_value(token, "--max-output-bytes")?;
                max_output_bytes = Some(parse_bounded_u64(
                    Some(&value),
                    "--max-output-bytes",
                    1_024,
                    100 * 1024 * 1024,
                )?);
            }
            token if token.starts_with('-') => {
                return Err(anyhow!(
                    "unknown swarm dispatch option '{}'\n\n{}",
                    token,
                    swarm_usage()
                ));
            }
            token if task_list_id.is_none() => task_list_id = Some(token.to_string()),
            _ => return Err(anyhow!(swarm_usage())),
        }
        index += 1;
    }

    let workflow_run_id =
        workflow_run_id.ok_or_else(|| anyhow!("missing option --workflow\n\n{}", swarm_usage()))?;
    let defaults = SwarmWorkerBudget::default();
    Ok(SwarmDispatchArgs {
        json_output,
        workflow_run_id,
        max_workers: max_workers.unwrap_or(2),
        budget: SwarmWorkerBudget {
            max_attempts: max_attempts.unwrap_or(defaults.max_attempts),
            max_commands: max_commands.unwrap_or(defaults.max_commands),
            timeout_seconds: timeout_seconds.unwrap_or(defaults.timeout_seconds),
            max_output_bytes: max_output_bytes.unwrap_or(defaults.max_output_bytes),
        },
        task_list_id,
    })
}

fn ensure_option_absent<T>(value: &Option<T>, option: &str) -> Result<()> {
    if value.is_some() {
        return Err(anyhow!("duplicate option {option}\n\n{}", swarm_usage()));
    }
    Ok(())
}

fn required_option_value(value: Option<&str>, option: &str) -> Result<String> {
    value
        .filter(|value| !value.trim().is_empty() && !value.starts_with('-'))
        .map(str::to_string)
        .ok_or_else(|| anyhow!("missing value for {option}\n\n{}", swarm_usage()))
}

fn required_inline_value(token: &str, option: &str) -> Result<String> {
    token
        .split_once('=')
        .map(|(_, value)| value)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("missing value for {option}\n\n{}", swarm_usage()))
}

fn parse_bounded_u64(value: Option<&str>, option: &str, min: u64, max: u64) -> Result<u64> {
    let value = value
        .filter(|value| !value.trim().is_empty() && !value.starts_with('-'))
        .ok_or_else(|| anyhow!("missing value for {option}\n\n{}", swarm_usage()))?;
    let parsed = value
        .parse::<u64>()
        .map_err(|_| anyhow!("{option} must be an integer between {min} and {max}"))?;
    if !(min..=max).contains(&parsed) {
        return Err(anyhow!("{option} must be between {min} and {max}"));
    }
    Ok(parsed)
}

fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let contents = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&contents)
        .with_context(|| format!("failed to parse JSON from {}", path.display()))
}

fn read_optional_json(path: &Path) -> Result<Option<Value>> {
    match fs::read(path) {
        Ok(contents) => Ok(Some(serde_json::from_slice(&contents).with_context(
            || format!("failed to parse JSON from {}", path.display()),
        )?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn read_optional_i32(path: &Path) -> Result<Option<i32>> {
    match fs::read_to_string(path) {
        Ok(contents) if contents.trim().is_empty() => Ok(None),
        Ok(contents) => {
            Ok(Some(contents.trim().parse::<i32>().with_context(|| {
                format!("invalid integer in {}", path.display())
            })?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn read_optional_u64(path: &Path) -> Result<Option<u64>> {
    match fs::read_to_string(path) {
        Ok(contents) if contents.trim().is_empty() => Ok(None),
        Ok(contents) => {
            Ok(Some(contents.trim().parse::<u64>().with_context(|| {
                format!("invalid integer in {}", path.display())
            })?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn ready_swarm_artifact_dir(
    context: &CommandContext,
    workflow_run_id: &str,
    action: &str,
) -> Result<PathBuf> {
    let resume = resume_workflow_run(cwd(context), Some(workflow_run_id))?;
    if !resume.resume_status.can_continue() {
        return Err(anyhow!(resume.blocker.unwrap_or_else(|| {
            format!("workflow run is not ready for worker {action}")
        })));
    }
    workflow_run_dir(context, workflow_run_id)
}

fn file_size(path: &Path) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn is_terminal_worker_status(status: &str) -> bool {
    matches!(
        status,
        "cancelled"
            | "completed"
            | "failed"
            | "timeout"
            | "budget_exhausted"
            | "scope_violation"
            | "lost"
    )
}

struct SwarmWorkerHealthInput<'a> {
    status: &'a str,
    termination_reason: Option<&'a str>,
    pid: Option<u32>,
    attempt: u32,
    max_attempts: u32,
    output_bytes: u64,
    max_output_bytes: u64,
    commands_run: usize,
    max_commands: u32,
    elapsed_ms: u64,
    timeout_seconds: u64,
    process_identity_status: &'a ProcessIdentityStatus,
    changed_files: usize,
    scope_deviations: usize,
    project_violation_reason: Option<&'a str>,
    retrying: bool,
}

fn swarm_worker_health_report(input: SwarmWorkerHealthInput<'_>) -> Value {
    let command_count = u32::try_from(input.commands_run).unwrap_or(u32::MAX);
    let timeout_ms = input.timeout_seconds.saturating_mul(1000);
    let identity_state = process_identity_status_label(input.process_identity_status);
    let health_state = if input.retrying {
        "retrying"
    } else if input.status == "completed" {
        "completed"
    } else if input.status == "running" && identity_state == "verified" {
        "healthy"
    } else if input.status == "prepared" {
        "pending"
    } else {
        "attention_required"
    };
    let health_reason = if input.retrying {
        Some("retrying_after_worker_failed".to_string())
    } else if let Some(reason) = input.project_violation_reason {
        Some(reason.to_string())
    } else if input.scope_deviations > 0 {
        Some("scope_deviation".to_string())
    } else if !matches!(
        input.process_identity_status,
        ProcessIdentityStatus::Verified
    ) {
        Some(process_identity_reason(input.process_identity_status))
    } else {
        input.termination_reason.map(str::to_string)
    };
    let next_action = if input.retrying || input.status == "running" {
        "run_swarm_monitor"
    } else if input.status == "completed" {
        "swarm_integration_review"
    } else {
        "inspect_worker_result"
    };
    json!({
        "schema": "kiana.swarm-worker-health.v1",
        "state": health_state,
        "reason": health_reason,
        "next_action": next_action,
        "process": {
            "pid": input.pid,
            "identity_status": identity_state,
            "identity_reason": process_identity_reason_value(input.process_identity_status)
        },
        "attempt": {
            "current": input.attempt,
            "max": input.max_attempts,
            "remaining": input.max_attempts.saturating_sub(input.attempt),
            "retrying": input.retrying
        },
        "budget": {
            "output_bytes": input.output_bytes,
            "max_output_bytes": input.max_output_bytes,
            "output_bytes_remaining": input.max_output_bytes.saturating_sub(input.output_bytes),
            "commands_run": command_count,
            "max_commands": input.max_commands,
            "commands_remaining": input.max_commands.saturating_sub(command_count),
            "elapsed_ms": input.elapsed_ms,
            "timeout_ms": timeout_ms,
            "timeout_ms_remaining": timeout_ms.saturating_sub(input.elapsed_ms)
        },
        "scope": {
            "changed_files": input.changed_files,
            "scope_deviations": input.scope_deviations,
            "project_violation_reason": input.project_violation_reason
        }
    })
}

fn should_retry_swarm_worker(
    status: &str,
    reason: Option<&str>,
    launch: &SwarmWorkerLaunch,
    changed_files: &[String],
    scope_deviations: &[String],
    project_violation_reason: Option<&str>,
) -> bool {
    status == "failed"
        && reason == Some("worker_failed")
        && launch.attempt < launch.budget.max_attempts
        && changed_files.is_empty()
        && scope_deviations.is_empty()
        && project_violation_reason.is_none()
}

fn retry_swarm_worker_launch(launch: &SwarmWorkerLaunch, attempt: u32) -> SwarmWorkerLaunch {
    let mut retry = launch.clone();
    retry.attempt = attempt;
    retry
}

fn archive_swarm_retry_attempt(
    worker_dir: &Path,
    attempt: u32,
    status: &str,
    reason: Option<&str>,
    exit_code: Option<i32>,
    output_bytes: u64,
    finished_at_ms: u64,
    process_identity_status: &ProcessIdentityStatus,
) -> Result<()> {
    let attempt_dir = worker_dir.join("attempts").join(attempt.to_string());
    fs::create_dir_all(&attempt_dir)?;
    for name in [
        "pid",
        "started_at_ms",
        "stdout.log",
        "stderr.log",
        "exit_code",
        "finished_at_ms",
        "telemetry.json",
        "worker-result.json",
        "state.json",
    ] {
        let source = worker_dir.join(name);
        if source.is_file() {
            fs::copy(&source, attempt_dir.join(name)).with_context(|| {
                format!(
                    "failed to archive swarm retry attempt file {}",
                    source.display()
                )
            })?;
        }
    }
    write_json_atomic(
        &attempt_dir.join("attempt-summary.json"),
        &json!({
            "schema": "kiana.swarm-retry-attempt.v1",
            "attempt": attempt,
            "status": status,
            "termination_reason": reason,
            "exit_code": exit_code,
            "output_bytes": output_bytes,
            "finished_at_ms": finished_at_ms,
            "process_identity_status": process_identity_status_label(process_identity_status),
            "process_identity_reason": process_identity_reason_value(process_identity_status)
        }),
    )?;
    reset_swarm_worker_outputs(worker_dir)?;
    Ok(())
}

fn reset_swarm_worker_outputs(worker_dir: &Path) -> Result<()> {
    for (name, contents) in [
        ("pid", ""),
        ("started_at_ms", ""),
        ("stdout.log", ""),
        ("stderr.log", ""),
        ("exit_code", ""),
        ("finished_at_ms", ""),
        ("telemetry.json", "{}\n"),
        ("worker-result.json", "{}\n"),
    ] {
        let path = worker_dir.join(name);
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(anyhow!(
                    "worker writable output must be a regular file: {}",
                    path.display()
                ));
            }
        }
        fs::write(&path, contents)
            .with_context(|| format!("failed to reset swarm worker output {}", path.display()))?;
    }
    Ok(())
}

fn worker_process_identity_from_state(state: &Value) -> Result<Option<WorkerProcessIdentity>> {
    let Some(identity) = state.get("process_identity") else {
        return Ok(None);
    };
    if identity.is_null() {
        return Ok(None);
    }
    serde_json::from_value(identity.clone())
        .context("worker process identity is invalid")
        .map(Some)
}

fn require_verified_worker_process_identity(
    state: &Value,
    worker_id: &str,
    action: &str,
) -> Result<WorkerProcessIdentity> {
    let identity = worker_process_identity_from_state(state)?.ok_or_else(|| {
        anyhow!("process_identity_missing: worker {worker_id} cannot be safely {action}")
    })?;
    if identity.process_group_id != identity.pid {
        return Err(anyhow!(
            "process_identity_mismatch:process_group_id_not_worker_leader: worker {worker_id} cannot be safely {action}"
        ));
    }
    let status = check_worker_process_identity(&identity);
    if !matches!(status, ProcessIdentityStatus::Verified) {
        return Err(anyhow!(
            "{}: worker {worker_id} cannot be safely {action}",
            process_identity_reason(&status)
        ));
    }
    Ok(identity)
}

fn capture_stable_worker_process_identity(pid: u32) -> Result<WorkerProcessIdentity> {
    let mut last_status = ProcessIdentityStatus::Missing;
    for _ in 0..100 {
        match capture_worker_process_identity(pid) {
            Ok(identity) => {
                if identity.process_group_id != pid {
                    last_status = ProcessIdentityStatus::Mismatch(vec![
                        "process_group_id_not_worker_leader".to_string(),
                    ]);
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
                match check_worker_process_identity(&identity) {
                    ProcessIdentityStatus::Verified => return Ok(identity),
                    status => last_status = status,
                }
            }
            Err(status) => last_status = status,
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    Err(anyhow!(process_identity_reason(&last_status)))
}

fn process_identity_status_label(status: &ProcessIdentityStatus) -> &'static str {
    match status {
        ProcessIdentityStatus::Verified => "verified",
        ProcessIdentityStatus::Exited => "exited",
        ProcessIdentityStatus::Missing => "missing",
        ProcessIdentityStatus::Mismatch(_) => "mismatch",
        ProcessIdentityStatus::Unavailable(_) => "unavailable",
    }
}

fn process_identity_reason(status: &ProcessIdentityStatus) -> String {
    match status {
        ProcessIdentityStatus::Verified => "process_identity_verified".to_string(),
        ProcessIdentityStatus::Exited => "process_identity_exited".to_string(),
        ProcessIdentityStatus::Missing => "process_identity_missing".to_string(),
        ProcessIdentityStatus::Mismatch(reasons) => {
            format!("process_identity_mismatch:{}", reasons.join(","))
        }
        ProcessIdentityStatus::Unavailable(reason) => {
            format!("process_identity_unavailable:{reason}")
        }
    }
}

fn process_identity_reason_value(status: &ProcessIdentityStatus) -> Value {
    if matches!(status, ProcessIdentityStatus::Verified) {
        Value::Null
    } else {
        json!(process_identity_reason(status))
    }
}

fn file_manifest(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut manifest = BTreeMap::new();
    collect_file_manifest(root, root, &mut manifest)?;
    Ok(manifest)
}

fn collect_file_manifest(
    root: &Path,
    current: &Path,
    manifest: &mut BTreeMap<String, String>,
) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if matches!(
            name_text.as_ref(),
            ".git" | ".kiana" | "target" | "node_modules" | ".venv" | "dist" | "build"
        ) {
            continue;
        }
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_file_manifest(root, &path, manifest)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| anyhow!("manifest path escaped isolation root"))?
                .to_string_lossy()
                .replace('\\', "/");
            manifest.insert(relative, integration_path_descriptor(&path)?);
        } else if file_type.is_symlink() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| anyhow!("manifest symlink escaped isolation root"))?
                .to_string_lossy()
                .replace('\\', "/");
            manifest.insert(relative, integration_path_descriptor(&path)?);
        }
    }
    Ok(())
}

fn file_mode_fingerprint(path: &Path) -> Result<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return Ok(format!(
            "{:04o}",
            fs::symlink_metadata(path)?.permissions().mode() & 0o7777
        ));
    }
    #[cfg(not(unix))]
    {
        Ok(if fs::symlink_metadata(path)?.permissions().readonly() {
            "readonly".to_string()
        } else {
            "writable".to_string()
        })
    }
}

fn manifest_diff(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut paths = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .filter(|path| before.get(path) != after.get(path))
        .collect()
}

fn path_allowed_by_packet(path: &str, packet: &SwarmWorkPacket) -> bool {
    packet.path_locks.iter().any(|lock| {
        path == lock.resolved_path
            || path
                .strip_prefix(&lock.resolved_path)
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

#[derive(Clone, Debug)]
struct SwarmWorkerTelemetry {
    provided: bool,
    command_count: u32,
    commands_run: Vec<String>,
    invalid_command_entries: u32,
    raw_sha256: Option<String>,
    notes: Vec<String>,
}

impl SwarmWorkerTelemetry {
    fn missing() -> Self {
        Self {
            provided: false,
            command_count: 0,
            commands_run: Vec::new(),
            invalid_command_entries: 0,
            raw_sha256: None,
            notes: vec!["runner telemetry was not provided".to_string()],
        }
    }

    fn to_value(&self) -> Value {
        json!({
            "schema": "kiana.swarm-worker-telemetry.v1",
            "provided": self.provided,
            "command_count": self.command_count,
            "commands_run": self.commands_run,
            "invalid_command_entries": self.invalid_command_entries,
            "raw_sha256": self.raw_sha256,
            "notes": self.notes
        })
    }
}

fn read_worker_telemetry(path: &Path) -> Result<SwarmWorkerTelemetry> {
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SwarmWorkerTelemetry::missing());
        }
        Err(error) => return Err(error.into()),
    };
    let value: Value = serde_json::from_slice(&contents)
        .with_context(|| format!("failed to parse JSON from {}", path.display()))?;
    let mut commands_run = Vec::new();
    let mut invalid_command_entries = 0u32;
    if let Some(values) = value.get("commands_run").and_then(Value::as_array) {
        for command in values {
            if let Some(command) = command.as_str() {
                commands_run.push(command.to_string());
            } else {
                invalid_command_entries = invalid_command_entries.saturating_add(1);
            }
        }
    }
    let command_count = u32::try_from(commands_run.len())
        .unwrap_or(u32::MAX)
        .saturating_add(invalid_command_entries);
    let notes = if invalid_command_entries > 0 {
        vec!["runner telemetry commands_run contained non-string entries".to_string()]
    } else {
        Vec::new()
    };
    Ok(SwarmWorkerTelemetry {
        provided: true,
        command_count,
        commands_run,
        invalid_command_entries,
        raw_sha256: Some(format!("sha256:{}", sha256_bytes(&contents))),
        notes,
    })
}

struct StartedSwarmWorker {
    pid: u32,
    process_identity: WorkerProcessIdentity,
}

#[allow(clippy::too_many_arguments)]
fn start_swarm_worker_process(
    artifact_dir: &Path,
    project_root: &Path,
    isolation_dir: &Path,
    worker_dir: &Path,
    state_path: &Path,
    launch: &SwarmWorkerLaunch,
    workflow_id: &str,
    #[cfg(target_os = "linux")] bwrap: &Path,
) -> Result<StartedSwarmWorker> {
    let runner_path = artifact_dir.join(&launch.runner_path);
    #[cfg(target_os = "linux")]
    let mut command = {
        let writable_files = prepare_swarm_worker_writable_files(worker_dir)?;
        let mut writable_roots = vec![isolation_dir.to_path_buf()];
        writable_roots.extend(writable_files);
        let plan = strict_bwrap_plan(
            bwrap,
            isolation_dir,
            std::slice::from_ref(&project_root.to_path_buf()),
            &writable_roots,
            std::ffi::OsString::from("bash"),
            vec![runner_path.as_os_str().to_os_string()],
        )?;
        background_sandboxed_swarm_process(&plan)
    };
    #[cfg(not(target_os = "linux"))]
    let mut command = background_swarm_process(&runner_path);
    let mut child = command
        .current_dir(isolation_dir)
        .env("KIANA_SWARM_WORKER_ID", &launch.worker_id)
        .env("KIANA_SWARM_WORKFLOW_ID", workflow_id)
        .env("KIANA_SWARM_DISPATCH_ID", &launch.dispatch_id)
        .env("KIANA_SWARM_TASK_ID", &launch.task_id)
        .env("KIANA_SWARM_ATTEMPT", launch.attempt.to_string())
        .env(
            "KIANA_SWARM_WORKPACKET_PATH",
            artifact_dir.join(&launch.workpacket_path),
        )
        .env(
            "KIANA_SWARM_RESULT_PATH",
            worker_dir.join("worker-result.json"),
        )
        .env(
            "KIANA_SWARM_TELEMETRY_PATH",
            worker_dir.join("telemetry.json"),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to start worker {}", launch.worker_id))?;
    let pid = child.id();
    let process_identity = match capture_stable_worker_process_identity(pid) {
        Ok(identity) => identity,
        Err(error) => {
            let _ = terminate_unbound_worker_process(pid);
            let _ = child.wait();
            return Err(error).with_context(|| {
                format!(
                    "failed to bind worker process identity {}",
                    launch.worker_id
                )
            });
        }
    };
    let process_identity_sha256 = identity_digest(&process_identity);
    let started_at_ms = now_ms();
    write_json_atomic(
        state_path,
        &json!({
            "schema": "kiana.swarm-worker-state.v2",
            "worker_id": launch.worker_id,
            "dispatch_id": launch.dispatch_id,
            "task_id": launch.task_id,
            "status": "running",
            "pid": pid,
            "process_identity": process_identity,
            "process_identity_status": "verified",
            "process_identity_reason": Value::Null,
            "attempt": launch.attempt,
            "started_at_ms": started_at_ms,
            "finished_at_ms": Value::Null,
            "exit_code": Value::Null,
            "termination_reason": Value::Null,
            "output_bytes": 0,
            "result_path": Value::Null,
            "updated_at_ms": started_at_ms
        }),
    )?;
    let worker_start_id = format!("{}-attempt-{}", launch.worker_id, launch.attempt);
    let event_worker_start_id = worker_start_id.clone();
    let event_worker_id = launch.worker_id.clone();
    let event_dispatch_id = launch.dispatch_id.clone();
    let event_task_id = launch.task_id.clone();
    let event_attempt = launch.attempt;
    let event_process_identity_sha256 = process_identity_sha256.clone();
    let _ = kiana_tasks::append_workflow_event_idempotent_with_unique_data_value(
        artifact_dir,
        WorkflowEventKind::WorkerStarted,
        "swarm_worker",
        "worker_start_id",
        worker_start_id,
        move |sequence, at_ms| {
            json!({
                "worker_start_id": event_worker_start_id,
                "worker_id": event_worker_id,
                "dispatch_id": event_dispatch_id,
                "task_id": event_task_id,
                "attempt": event_attempt,
                "process_identity_sha256": event_process_identity_sha256,
                "sequence": sequence,
                "started_at_ms": at_ms
            })
        },
    )?;
    Ok(StartedSwarmWorker {
        pid,
        process_identity,
    })
}

#[cfg(unix)]
fn terminate_unbound_worker_process(pid: u32) -> Result<()> {
    match capture_worker_process_identity(pid) {
        Ok(identity) => terminate_worker_process(&identity),
        Err(status) => Err(anyhow!(
            "process_identity_missing: worker process {pid} cannot be safely terminated: {}",
            process_identity_reason(&status)
        )),
    }
}

#[cfg(not(unix))]
fn terminate_unbound_worker_process(_pid: u32) -> Result<()> {
    Err(anyhow!(
        "process_identity_unavailable: worker process cannot be safely terminated on this platform"
    ))
}

#[cfg(unix)]
fn terminate_worker_process(identity: &WorkerProcessIdentity) -> Result<()> {
    if identity.process_group_id != identity.pid {
        return Err(anyhow!(
            "process_identity_mismatch:process_group_id_not_worker_leader"
        ));
    }
    match check_worker_process_identity(identity) {
        ProcessIdentityStatus::Verified => {}
        ProcessIdentityStatus::Exited => return Ok(()),
        status => return Err(anyhow!(process_identity_reason(&status))),
    }

    let group = format!("-{}", identity.process_group_id);
    let term_status = ProcessCommand::new("kill")
        .args(["-TERM", "--", &group])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !term_status.success() {
        return match check_worker_process_identity(identity) {
            ProcessIdentityStatus::Exited => Ok(()),
            status => Err(anyhow!(
                "worker TERM failed: {}",
                process_identity_reason(&status)
            )),
        };
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    match check_worker_process_identity(identity) {
        ProcessIdentityStatus::Exited => Ok(()),
        ProcessIdentityStatus::Verified => {
            let kill_status = ProcessCommand::new("kill")
                .args(["-KILL", "--", &group])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            if kill_status.success() {
                Ok(())
            } else {
                Err(anyhow!(
                    "failed to terminate worker process {}",
                    identity.pid
                ))
            }
        }
        status => Err(anyhow!(
            "worker identity changed after TERM: {}",
            process_identity_reason(&status)
        )),
    }
}

#[cfg(not(unix))]
fn terminate_worker_process(_identity: &WorkerProcessIdentity) -> Result<()> {
    Err(anyhow!(
        "process_identity_unavailable: safe worker termination is not supported on this platform"
    ))
}

fn write_json_atomic(path: &Path, value: &Value) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("JSON path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("JSON path has invalid file name: {}", path.display()))?;
    let temp = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.flush()?;
    file.sync_data()?;
    fs::rename(&temp, path)?;
    Ok(())
}

fn write_json_durable_atomic(path: &Path, value: &Value) -> Result<()> {
    write_json_atomic(path, value)?;
    #[cfg(unix)]
    {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow!("JSON path has no parent: {}", path.display()))?;
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn swarm_runner_executable() -> Result<PathBuf> {
    let candidate = std::env::var_os("KIANA_SWARM_WORKER_EXECUTABLE")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_exe()?);
    let canonical = fs::canonicalize(&candidate)
        .with_context(|| format!("worker executable does not exist: {}", candidate.display()))?;
    let metadata = fs::metadata(&canonical)?;
    if !metadata.is_file() {
        return Err(anyhow!(
            "worker executable is not a regular file: {}",
            canonical.display()
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(anyhow!(
                "worker executable is not executable: {}",
                canonical.display()
            ));
        }
    }
    Ok(canonical)
}

fn sha256_file(path: &Path) -> Result<String> {
    let contents = fs::read(path)?;
    let mut digest = Sha256::new();
    digest.update(contents);
    Ok(format!("{:x}", digest.finalize()))
}

fn select_swarm_isolation_strategy(project_root: &Path) -> String {
    if git_repository_is_clean(project_root) {
        "git_worktree".to_string()
    } else {
        "snapshot_copy".to_string()
    }
}

fn git_repository_is_clean(project_root: &Path) -> bool {
    let inside = ProcessCommand::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(project_root)
        .output();
    if !inside.is_ok_and(|output| output.status.success()) {
        return false;
    }
    let head = ProcessCommand::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .current_dir(project_root)
        .output();
    if !head.is_ok_and(|output| output.status.success()) {
        return false;
    }
    let status = match ProcessCommand::new("git")
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .current_dir(project_root)
        .output()
    {
        Ok(output) if output.status.success() => output.stdout,
        _ => return false,
    };
    status
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .all(|record| {
            let path = record.get(3..).unwrap_or_default();
            path == b".kiana" || path.starts_with(b".kiana/")
        })
}

fn ensure_swarm_isolation(project_root: &Path, destination: &Path, strategy: &str) -> Result<()> {
    if destination.is_dir() {
        return Ok(());
    }
    let parent = destination
        .parent()
        .ok_or_else(|| anyhow!("isolation path has no parent"))?;
    fs::create_dir_all(parent)?;
    match strategy {
        "git_worktree" => {
            let status = ProcessCommand::new("git")
                .args(["worktree", "add", "--detach"])
                .arg(destination)
                .arg("HEAD")
                .current_dir(project_root)
                .status()?;
            if !status.success() {
                return Err(anyhow!(
                    "failed to create git worktree isolation at {}",
                    destination.display()
                ));
            }
        }
        "snapshot_copy" => {
            fs::create_dir_all(destination)?;
            copy_swarm_snapshot(project_root, destination, true)?;
        }
        other => return Err(anyhow!("unsupported isolation strategy {other}")),
    }
    Ok(())
}

fn copy_swarm_snapshot(source: &Path, destination: &Path, root_level: bool) -> Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if should_skip_swarm_snapshot_entry(&name_text, root_level) {
            continue;
        }
        let source_path = entry.path();
        let destination_path = destination.join(&name);
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            fs::create_dir_all(&destination_path)?;
            copy_swarm_snapshot(&source_path, &destination_path, false)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path)?;
        } else if file_type.is_symlink() {
            copy_swarm_symlink(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn should_skip_swarm_snapshot_entry(name: &str, root_level: bool) -> bool {
    matches!(
        name,
        ".git" | ".kiana" | "target" | "node_modules" | ".venv" | "dist" | "build"
    ) || (root_level && name == ".claude")
}

#[cfg(unix)]
fn copy_swarm_symlink(source: &Path, destination: &Path) -> Result<()> {
    std::os::unix::fs::symlink(fs::read_link(source)?, destination)?;
    Ok(())
}

#[cfg(not(unix))]
fn copy_swarm_symlink(_source: &Path, _destination: &Path) -> Result<()> {
    Err(anyhow!(
        "snapshot symlink copy is not supported on this platform"
    ))
}

fn build_swarm_runner_script(
    artifact_dir: &Path,
    dispatch_id: &str,
    task_id: &str,
    executable: &Path,
    runner_mode: &str,
) -> Result<String> {
    let worker_dir = artifact_dir.join("workers").join(dispatch_id).join(task_id);
    let prompt = worker_dir.join("prompt.md");
    let command = if runner_mode == "fixture_executable" {
        shell_quote_path(executable)
    } else {
        format!(
            "{} -p \"$(cat {})\"",
            shell_quote_path(executable),
            shell_quote_path(&prompt)
        )
    };
    Ok(format!(
        "#!/usr/bin/env bash\nset +e\numask 077\nprintf '%s\\n' \"$$\" > {pid}\ndate +%s%3N > {started}\n{command} >> {stdout} 2>> {stderr}\ncode=$?\n# Write completion metadata before the exit marker; the marker is the final\n# signal that a monitor may consume as a complete worker result.\ndate +%s%3N > {finished}\nprintf '%s\\n' \"$code\" > {exit}\nexit \"$code\"\n",
        pid = shell_quote_path(&worker_dir.join("pid")),
        started = shell_quote_path(&worker_dir.join("started_at_ms")),
        stdout = shell_quote_path(&worker_dir.join("stdout.log")),
        stderr = shell_quote_path(&worker_dir.join("stderr.log")),
        exit = shell_quote_path(&worker_dir.join("exit_code")),
        finished = shell_quote_path(&worker_dir.join("finished_at_ms")),
    ))
}

fn shell_quote_path(path: &Path) -> String {
    shell_quote_text(&path.to_string_lossy())
}

fn shell_quote_text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(all(unix, not(target_os = "linux")))]
fn background_swarm_process(runner_path: &Path) -> ProcessCommand {
    let mut command = ProcessCommand::new("setsid");
    command.arg("bash").arg(runner_path);
    command
}

#[cfg(not(unix))]
fn background_swarm_process(runner_path: &Path) -> ProcessCommand {
    let mut command = ProcessCommand::new("bash");
    command.arg(runner_path);
    command
}

fn list_tasks(context: &CommandContext, task_list: Option<&str>) -> Result<CommandResult> {
    let tasks = load_tasks(context, task_list)?;
    let task_list_id = task_list_id(context, task_list);
    if tasks.is_empty() {
        return Ok(CommandResult::text(format!(
            "No tasks.\ntask_list_id: {}\ntasks_dir: {}\nTasks created by TaskCreate are stored in this task list.",
            task_list_id,
            task_list_dir(context, task_list).display()
        )));
    }

    Ok(CommandResult::text(format_task_list(
        &tasks,
        &task_list_id,
        None,
    )))
}

fn list_tasks_with_status(
    context: &CommandContext,
    status: &str,
    task_list: Option<&str>,
) -> Result<CommandResult> {
    let mut tasks = load_tasks(context, task_list)?;
    tasks.retain(|task| task_status(task).is_some_and(|value| value == status));
    Ok(CommandResult::text(format_task_list(
        &tasks,
        &task_list_id(context, task_list),
        Some(status),
    )))
}

fn tasks_json(context: &CommandContext, task_list: Option<&str>) -> Result<CommandResult> {
    Ok(CommandResult::text(serde_json::to_string_pretty(
        &tasks_report(context, task_list)?,
    )?))
}

fn team_plan(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let (json_output, task_list) = parse_plan_args(rest)?;
    let report = team_plan_report(context, task_list)?;
    if json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
    }

    let status = report["role_runtime"]["status"]
        .as_str()
        .unwrap_or("incomplete");
    let missing_roles = report["role_runtime"]["missing_roles"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let missing_artifacts = report["artifact_readiness"]["missing_roles"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    Ok(CommandResult::text(format!(
        "Team plan\nstatus: {}\ntask_list_id: {}\nmissing_roles: {}\nmissing_artifacts: {}\nusage: kiana tasks plan --json",
        status,
        report["task_list"]["id"].as_str().unwrap_or("default"),
        if missing_roles.is_empty() { "-" } else { &missing_roles },
        if missing_artifacts.is_empty() { "-" } else { &missing_artifacts }
    )))
}

fn workflow_command(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let (command, rest) = split_word(rest);
    match command.unwrap_or("help") {
        "template" => workflow_template(rest),
        "init" | "create" => workflow_init(context, rest),
        "list" => workflow_list(context, rest),
        "continue" | "resume" => workflow_continue(context, rest),
        "show" | "get" => workflow_show(context, rest),
        "advance" => workflow_advance(context, rest),
        "complete" => workflow_complete(context, rest),
        "integrity" => workflow_integrity_command(context, rest),
        "help" | "--help" | "-h" => Ok(CommandResult::text(workflow_usage())),
        other => Err(anyhow!(
            "unknown workflow command '{}'\n\n{}",
            other,
            workflow_usage()
        )),
    }
}

fn workflow_integrity_command(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let (command, rest) = split_word(rest);
    match command.unwrap_or("help") {
        "init" => workflow_integrity_init(rest),
        "status" => workflow_integrity_status(rest),
        "verify" => workflow_integrity_verify(context, rest),
        "seal" => workflow_integrity_seal(context, rest),
        "help" | "--help" | "-h" => Ok(CommandResult::text(workflow_integrity_usage())),
        other => Err(anyhow!(
            "unknown workflow integrity command '{}'\n\n{}",
            other,
            workflow_integrity_usage()
        )),
    }
}

fn workflow_integrity_init(rest: &str) -> Result<CommandResult> {
    let json_output = parse_workflow_integrity_json_only(rest, "init")?;
    let status = initialize_local_hmac_key()?;
    format_workflow_integrity_key_status(status, json_output)
}

fn workflow_integrity_status(rest: &str) -> Result<CommandResult> {
    let json_output = parse_workflow_integrity_json_only(rest, "status")?;
    let status = inspect_local_hmac_key()?;
    format_workflow_integrity_key_status(status, json_output)
}

fn workflow_release_last_event(artifact_dir: &Path) -> Result<(u64, String)> {
    let eventlog_path = artifact_dir.join("eventlog.jsonl");
    let contents = fs::read_to_string(&eventlog_path)
        .with_context(|| format!("failed to read {}", eventlog_path.display()))?;
    let line = contents
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| anyhow!("workflow release eventlog is empty"))?;
    let record: Value = serde_json::from_str(line)
        .with_context(|| format!("invalid final event in {}", eventlog_path.display()))?;
    let event = record.get("event").unwrap_or(&record);
    let seq = event["seq"]
        .as_u64()
        .ok_or_else(|| anyhow!("workflow release final event is missing seq"))?;
    let kind = event["kind"]
        .as_str()
        .filter(|kind| !kind.trim().is_empty())
        .ok_or_else(|| anyhow!("workflow release final event is missing kind"))?
        .to_string();
    Ok((seq, kind))
}

fn workflow_integrity_verify(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let (json_output, run_id) = parse_workflow_integrity_run_args(rest, "verify", false)?;
    let run_id = resolve_workflow_integrity_run_id(context, run_id)?;
    let artifact_dir = workflow_run_dir(context, &run_id)?;
    let report = inspect_workflow_integrity(&artifact_dir)?;
    let recovery = inspect_swarm_recovery_integrity(&artifact_dir)?;
    let workflow_state = read_workflow_state(&artifact_dir)?;
    if workflow_state.run_id != run_id || workflow_state.workflow_id != report.workflow_id {
        return Err(anyhow!(
            "workflow release state identity does not match integrity report"
        ));
    }
    let (last_event_seq, last_event_kind) = workflow_release_last_event(&artifact_dir)?;
    if workflow_state.last_event_seq != last_event_seq {
        return Err(anyhow!(
            "workflow release state last_event_seq {} does not match eventlog {}",
            workflow_state.last_event_seq,
            last_event_seq
        ));
    }
    let workflow_status = serde_json::to_value(workflow_state.status)?;
    let release_binding = json!({
        "schema": "kiana.workflow-release-binding.v1",
        "run_id": run_id,
        "workflow_id": report.workflow_id,
        "workflow": {
            "status": workflow_status,
            "current_node": workflow_state.current_node,
            "last_event_seq": workflow_state.last_event_seq,
            "last_event_kind": last_event_kind,
            "state_sha256": sha256_file(&artifact_dir.join("state.json"))?,
            "eventlog_sha256": sha256_file(&artifact_dir.join("eventlog.jsonl"))?,
        },
        "git": git_state_snapshot(&cwd(context))?,
    });
    if json_output {
        let mut value = serde_json::to_value(&report)?;
        value["run_id"] = json!(run_id);
        value["release_binding"] = release_binding.clone();
        value["recovery_integrity"] = serde_json::to_value(&recovery)?;
        return Ok(CommandResult::text(serde_json::to_string_pretty(&value)?));
    }
    let status = serde_json::to_value(&report.status)?
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    Ok(CommandResult::text(format!(
        "Workflow integrity\nrun_id: {}\nworkflow_id: {}\nstatus: {}\nevents: {}\nlegacy_prefix: {}\nverified_events: {}\nartifact_descriptors: {}\nverified_artifacts: {}\norphan_artifacts: {}\nrecovery_active: {}\nrecovery_active_verified: {}\nrecovery_archived: {}\nrecovery_archived_verified: {}\nrelease_workflow_status: {}\nrelease_workflow_node: {}\nrelease_last_event: {}\nrelease_git_head: {}\nkey_id: {}\ngenesis_seal: {}",
        run_id,
        report.workflow_id,
        status,
        report.event_count,
        report.legacy_prefix_count,
        report.verified_event_count,
        report.artifact_descriptor_count,
        report.verified_artifact_count,
        report.orphan_artifact_count,
        recovery.active_journal_count,
        recovery.verified_active_journal_count,
        recovery.archived_journal_count,
        recovery.verified_archived_journal_count,
        release_binding["workflow"]["status"]
            .as_str()
            .unwrap_or("unknown"),
        release_binding["workflow"]["current_node"]
            .as_str()
            .unwrap_or("unknown"),
        release_binding["workflow"]["last_event_kind"]
            .as_str()
            .unwrap_or("unknown"),
        release_binding["git"]["head"].as_str().unwrap_or("none"),
        report.key_id.as_deref().unwrap_or("-"),
        report.genesis_seal_path.as_deref().unwrap_or("-")
    )))
}

fn workflow_integrity_seal(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let (json_output, run_id) = parse_workflow_integrity_run_args(rest, "seal", true)?;
    let run_id = run_id.expect("required workflow integrity run_id");
    let seal = seal_legacy_workflow(workflow_run_dir(context, &run_id)?)?;
    let result = json!({
        "schema": "kiana.workflow-integrity-seal-result.v1",
        "run_id": run_id,
        "workflow_id": seal.workflow_id,
        "genesis_seal_path": "integrity/genesis-seal.json",
        "seal": seal,
    });
    if json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&result)?));
    }
    Ok(CommandResult::text(format!(
        "Workflow integrity seal\nrun_id: {}\nworkflow_id: {}\nlegacy_events: {}\nkey_id: {}\npath: integrity/genesis-seal.json",
        result["run_id"].as_str().unwrap_or("-"),
        result["workflow_id"].as_str().unwrap_or("-"),
        result["seal"]["legacy_event_count"].as_u64().unwrap_or(0),
        result["seal"]["key_id"].as_str().unwrap_or("-")
    )))
}

fn format_workflow_integrity_key_status(
    status: kiana_tasks::IntegrityKeyStatus,
    json_output: bool,
) -> Result<CommandResult> {
    if json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&status)?));
    }
    Ok(CommandResult::text(format!(
        "Workflow integrity key\nconfigured: {}\nbackend: {}\nalgorithm: {}\nkey_id: {}\npath: {}\npermission_state: {}",
        status.configured,
        status.backend,
        status.algorithm,
        status.key_id.as_deref().unwrap_or("-"),
        status.path,
        status.permission_state
    )))
}

fn parse_workflow_integrity_json_only(rest: &str, action: &str) -> Result<bool> {
    let mut json_output = false;
    for part in rest.split_whitespace() {
        match part {
            "--json" | "-j" => json_output = true,
            value => {
                return Err(anyhow!(
                    "unknown workflow integrity {} option '{}'\n\n{}",
                    action,
                    value,
                    workflow_integrity_usage()
                ));
            }
        }
    }
    Ok(json_output)
}

fn parse_workflow_integrity_run_args(
    rest: &str,
    action: &str,
    required: bool,
) -> Result<(bool, Option<String>)> {
    let mut json_output = false;
    let mut run_id = None;
    for part in rest.split_whitespace() {
        match part {
            "--json" | "-j" => json_output = true,
            value if value.starts_with('-') => {
                return Err(anyhow!(
                    "unknown workflow integrity {} option '{}'\n\n{}",
                    action,
                    value,
                    workflow_integrity_usage()
                ));
            }
            value if run_id.is_none() => run_id = Some(value.to_string()),
            _ => return Err(anyhow!("{}", workflow_integrity_usage())),
        }
    }
    if required && run_id.is_none() {
        return Err(anyhow!(
            "workflow integrity {} requires a run_id\n\n{}",
            action,
            workflow_integrity_usage()
        ));
    }
    Ok((json_output, run_id))
}

fn resolve_workflow_integrity_run_id(
    context: &CommandContext,
    run_id: Option<String>,
) -> Result<String> {
    if let Some(run_id) = run_id {
        return Ok(run_id);
    }
    list_workflow_runs(cwd(context))?
        .into_iter()
        .next()
        .map(|run| run.run_id)
        .ok_or_else(|| anyhow!("workflow_not_found: no workflow runs exist"))
}

fn workflow_template(rest: &str) -> Result<CommandResult> {
    let json_output = parse_json_only_flag(rest)?;
    let template = default_workflow_template();
    if json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(
            &template,
        )?));
    }

    Ok(CommandResult::text(format!(
        "Workflow template\nid: {}\nversion: {}\nnodes: {}\nedges: {}\nusage: kiana tasks workflow template --json",
        template.id,
        template.version,
        template.nodes.len(),
        template.edges.len()
    )))
}

fn workflow_init(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let args = parse_workflow_init_args(rest)?;
    let run = initialize_workflow_run(
        cwd(context),
        WorkflowInit {
            request: args.request,
            input_kind: args.input_kind,
            profile: args.profile,
            approval_required: args.approval_required,
        },
    )?;
    if args.json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&run)?));
    }

    Ok(CommandResult::text(format!(
        "Workflow run created\nrun_id: {}\nworkflow_id: {}\ncurrent_node: {}\nartifact_dir: {}\nusage: kiana tasks workflow show {}",
        run.run_id,
        run.workflow_id,
        run.current_node,
        run.artifact_dir.display(),
        run.run_id
    )))
}

fn workflow_show(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let (json_output, run_id) = parse_workflow_show_args(rest)?;
    let dir = workflow_run_dir(context, &run_id)?;
    let state_path = dir.join("state.json");
    let contents = std::fs::read_to_string(&state_path)
        .with_context(|| format!("failed to read {}", state_path.display()))?;
    if json_output {
        return Ok(CommandResult::text(contents));
    }
    let state: Value = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse {}", state_path.display()))?;
    Ok(CommandResult::text(format!(
        "Workflow run\nrun_id: {}\nstatus: {}\ncurrent_node: {}\nartifact_dir: {}",
        state["run_id"].as_str().unwrap_or(&run_id),
        state["status"].as_str().unwrap_or("unknown"),
        state["current_node"].as_str().unwrap_or("unknown"),
        dir.display()
    )))
}

fn workflow_list(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let json_output = parse_json_only_flag(rest)?;
    let runs = list_workflow_runs(cwd(context))?;
    if json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&json!({
            "schema": "kiana.workflow-list.v1",
            "count": runs.len(),
            "runs": runs,
        }))?));
    }

    if runs.is_empty() {
        return Ok(CommandResult::text(
            "No workflow runs.\nusage: kiana tasks workflow init <request>",
        ));
    }

    let lines = runs
        .iter()
        .map(|run| {
            format!(
                "- {} current_node={} updated_at_ms={} request={}",
                run.run_id, run.current_node, run.updated_at_ms, run.request
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(CommandResult::text(format!(
        "Workflow runs\ncount: {}\n{}\nusage: kiana tasks workflow continue [run_id]",
        runs.len(),
        lines
    )))
}

fn workflow_continue(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let (json_output, run_id) = parse_workflow_continue_args(rest)?;
    let report = resume_workflow_run(cwd(context), run_id.as_deref())?;
    if json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
    }

    let status = report.resume_status.as_str();
    Ok(CommandResult::text(format!(
        "Workflow resume\nrun_id: {}\nstatus: {}\ncurrent_node: {}\neventlog_consistent: {}\nblocker: {}\nrecommended_action: {}",
        report.run.run_id,
        status,
        report.run.current_node,
        report.eventlog_consistent,
        report.blocker.as_deref().unwrap_or("-"),
        report.recommended_action
    )))
}

struct WorkflowAdvanceArgs {
    json_output: bool,
    decision: String,
    evidence: Vec<String>,
    note: Option<String>,
    run_id: String,
}

struct WorkflowCompleteArgs {
    json_output: bool,
    verification_id: String,
    run_id: String,
}

fn workflow_advance(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let args = parse_workflow_advance_args(rest)?;
    let resume = resume_workflow_run(cwd(context), Some(&args.run_id))?;
    if !resume.resume_status.can_continue() {
        return Err(anyhow!(resume.blocker.unwrap_or_else(|| {
            "workflow is not ready for advancement".to_string()
        })));
    }
    if resume.run.status != WorkflowStatus::Running {
        return Err(anyhow!(
            "workflow must be running to advance; current status is {:?}",
            resume.run.status
        ));
    }
    let artifact_dir = resume.run.artifact_dir.clone();
    let dag: WorkflowDagTemplate = read_json_file(&artifact_dir.join("workflow_dag.json"))?;
    let current_node = resume.run.current_node.clone();
    let allowed = dag
        .edges
        .iter()
        .filter(|edge| edge.from == current_node)
        .collect::<Vec<_>>();
    let matching = allowed
        .iter()
        .filter(|edge| edge.decision == args.decision)
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        let allowed_decisions = allowed
            .iter()
            .map(|edge| edge.decision.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ");
        return Err(anyhow!(
            "decision '{}' is not a unique edge from {}; allowed decisions: {}",
            args.decision,
            current_node,
            if allowed_decisions.is_empty() {
                "<none>"
            } else {
                &allowed_decisions
            }
        ));
    }
    let evidence = args
        .evidence
        .iter()
        .map(|path| workflow_transition_evidence(&artifact_dir, path))
        .collect::<Result<Vec<_>>>()?;
    let edge = matching[0];
    let result = append_workflow_transition(
        &artifact_dir,
        WorkflowTransitionInput {
            from: edge.from.clone(),
            to: edge.to.clone(),
            decision: edge.decision.clone(),
            evidence: evidence.clone(),
            note: args.note,
        },
    )?;
    let report = json!({
        "schema": "kiana.workflow-transition.v1",
        "run_id": resume.run.run_id,
        "workflow_id": resume.run.workflow_id,
        "transition_id": result.transition_id,
        "from": edge.from,
        "to": edge.to,
        "decision": edge.decision,
        "evidence": evidence,
        "first_event_seq": result.gate_event.seq,
        "last_event_seq": result.node_entered_event.seq,
        "current_node": result.state.current_node,
        "status": result.state.status,
        "idempotent": false,
    });
    if args.json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
    }
    Ok(CommandResult::text(format!(
        "Workflow advanced\nrun_id: {}\ntransition_id: {}\nfrom: {}\nto: {}\ndecision: {}\nevidence: {}\nlast_event_seq: {}",
        report["run_id"].as_str().unwrap_or_default(),
        report["transition_id"].as_str().unwrap_or_default(),
        report["from"].as_str().unwrap_or_default(),
        report["to"].as_str().unwrap_or_default(),
        report["decision"].as_str().unwrap_or_default(),
        report["evidence"].as_array().map(Vec::len).unwrap_or(0),
        report["last_event_seq"].as_u64().unwrap_or(0),
    )))
}

fn workflow_complete(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let args = parse_workflow_complete_args(rest)?;
    validate_workflow_artifact_id("verification", &args.verification_id)?;
    let artifact_dir = workflow_run_dir(context, &args.run_id)?;
    let state = read_workflow_state(&artifact_dir)?;
    let existing = read_workflow_events(&artifact_dir)?
        .into_iter()
        .find(|event| event.kind == WorkflowEventKind::WorkflowCompleted);
    if let Some(event) = existing {
        if event
            .data
            .get("completion_verification_id")
            .and_then(Value::as_str)
            != Some(args.verification_id.as_str())
        {
            return Err(anyhow!(
                "workflow is already completed with a different verification packet"
            ));
        }
        let report = workflow_completion_report(&state, &event, true)?;
        return format_workflow_completion_report(report, args.json_output);
    }
    if state.status != WorkflowStatus::Running || state.current_node != "completed" {
        return Err(anyhow!(
            "workflow complete requires the terminal node completed; current status is {:?} at {}",
            state.status,
            state.current_node
        ));
    }

    let packet_path = artifact_dir
        .join("verification")
        .join(format!("{}.json", args.verification_id));
    let packet_bytes = read_regular_file_nofollow(&packet_path).map_err(|error| {
        anyhow!(
            "verification packet {} is unavailable or unsafe: {error}",
            args.verification_id
        )
    })?;
    let packet: VerificationPacket = serde_json::from_slice(&packet_bytes).with_context(|| {
        format!(
            "failed to parse verification packet {}",
            args.verification_id
        )
    })?;
    if packet.workflow_id != state.workflow_id || packet.run_id != state.run_id {
        return Err(anyhow!(
            "verification packet belongs to a different workflow run"
        ));
    }
    let evidence_events = read_evidence_events(&artifact_dir)?;
    validate_verification_packet_integrity(&packet, &evidence_events)?;
    validate_verification_packet_completion(&artifact_dir, &packet_path, &packet)?;
    if packet.final_status != VerificationStatus::Pass {
        return Err(anyhow!(
            "verification packet final status must be pass, got {:?}",
            packet.final_status
        ));
    }
    let verification_sha256 = format!("{:x}", Sha256::digest(&packet_bytes));
    let verification_id = packet.verification_id.clone();
    let event = append_workflow_event_idempotent_with_unique_data_value(
        &artifact_dir,
        WorkflowEventKind::WorkflowCompleted,
        "completed",
        "completion_verification_id",
        verification_id.clone(),
        |_, _| {
            json!({
                "completion_verification_id": verification_id,
                "verification_id": packet.verification_id,
                "verification_path": packet_path,
                "verification_sha256": verification_sha256,
                "check_count": packet.checks.len(),
                "pass_count": packet.pass_count,
                "fail_count": packet.fail_count,
                "blocked_count": packet.blocked_count,
                "skipped_count": packet.skipped_count,
                "unknown_count": packet.unknown_count,
            })
        },
    )?;
    let completed_state = read_workflow_state(&artifact_dir)?;
    let report = workflow_completion_report(&completed_state, &event, false)?;
    format_workflow_completion_report(report, args.json_output)
}

fn workflow_completion_report(
    state: &kiana_tasks::WorkflowState,
    event: &WorkflowEvent,
    idempotent: bool,
) -> Result<Value> {
    Ok(json!({
        "schema": "kiana.workflow-completion.v1",
        "run_id": state.run_id,
        "workflow_id": state.workflow_id,
        "verification_id": event.data.get("completion_verification_id").and_then(Value::as_str).ok_or_else(|| anyhow!("completed workflow event is missing verification id"))?,
        "verification_sha256": event.data.get("verification_sha256").and_then(Value::as_str).ok_or_else(|| anyhow!("completed workflow event is missing verification hash"))?,
        "event_seq": event.seq,
        "current_node": state.current_node,
        "status": state.status,
        "idempotent": idempotent,
    }))
}

fn format_workflow_completion_report(report: Value, json_output: bool) -> Result<CommandResult> {
    if json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
    }
    Ok(CommandResult::text(format!(
        "Workflow completed\nrun_id: {}\nverification_id: {}\nverification_sha256: {}\nevent_seq: {}",
        report["run_id"].as_str().unwrap_or_default(),
        report["verification_id"].as_str().unwrap_or_default(),
        report["verification_sha256"].as_str().unwrap_or_default(),
        report["event_seq"].as_u64().unwrap_or(0),
    )))
}

fn parse_workflow_advance_args(rest: &str) -> Result<WorkflowAdvanceArgs> {
    let mut json_output = false;
    let mut decision = None;
    let mut evidence = Vec::new();
    let mut note = None;
    let mut run_id = None;
    let mut parts = rest.split_whitespace();
    while let Some(part) = parts.next() {
        match part {
            "--json" | "-j" => json_output = true,
            "--decision" => set_once(
                &mut decision,
                required_option_value(parts.next(), "--decision")?,
                "--decision",
            )?,
            "--evidence" => evidence.push(required_option_value(parts.next(), "--evidence")?),
            "--note" => set_once(
                &mut note,
                required_option_value(parts.next(), "--note")?,
                "--note",
            )?,
            value if value.starts_with('-') => {
                return Err(anyhow!("unknown workflow advance option '{}'", value));
            }
            value if run_id.is_none() => run_id = Some(value.to_string()),
            value => return Err(anyhow!("unexpected workflow advance argument '{}'", value)),
        }
    }
    let decision = decision.ok_or_else(|| anyhow!("workflow advance requires --decision"))?;
    if evidence.is_empty() {
        return Err(anyhow!(
            "workflow advance requires at least one evidence file"
        ));
    }
    let run_id = run_id.ok_or_else(|| anyhow!("workflow advance requires a run_id"))?;
    Ok(WorkflowAdvanceArgs {
        json_output,
        decision,
        evidence,
        note,
        run_id,
    })
}

fn parse_workflow_complete_args(rest: &str) -> Result<WorkflowCompleteArgs> {
    let mut json_output = false;
    let mut verification_id = None;
    let mut run_id = None;
    let mut parts = rest.split_whitespace();
    while let Some(part) = parts.next() {
        match part {
            "--json" | "-j" => json_output = true,
            "--verification" => set_once(
                &mut verification_id,
                required_option_value(parts.next(), "--verification")?,
                "--verification",
            )?,
            value if value.starts_with('-') => {
                return Err(anyhow!("unknown workflow complete option '{}'", value));
            }
            value if run_id.is_none() => run_id = Some(value.to_string()),
            value => return Err(anyhow!("unexpected workflow complete argument '{}'", value)),
        }
    }
    Ok(WorkflowCompleteArgs {
        json_output,
        verification_id: verification_id
            .ok_or_else(|| anyhow!("workflow complete requires --verification"))?,
        run_id: run_id.ok_or_else(|| anyhow!("workflow complete requires a run_id"))?,
    })
}

fn set_once(target: &mut Option<String>, value: String, option: &str) -> Result<()> {
    if target.is_some() {
        return Err(anyhow!("duplicate option {option}"));
    }
    *target = Some(value);
    Ok(())
}

fn workflow_transition_evidence(
    artifact_dir: &Path,
    raw_path: &str,
) -> Result<WorkflowTransitionEvidence> {
    const MAX_EVIDENCE_BYTES: u64 = 1024 * 1024;
    let relative = Path::new(raw_path);
    if raw_path.trim().is_empty()
        || relative.is_absolute()
        || raw_path.as_bytes().get(1) == Some(&b':')
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(anyhow!(
            "evidence path is outside workflow artifact directory: {}",
            raw_path
        ));
    }
    let artifact_root = fs::canonicalize(artifact_dir)?;
    let path = artifact_dir.join(relative);
    let metadata = fs::symlink_metadata(&path)
        .with_context(|| format!("evidence file was not found: {raw_path}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(anyhow!("evidence path must be a regular file: {raw_path}"));
    }
    let canonical = fs::canonicalize(&path)?;
    if !canonical.starts_with(&artifact_root) {
        return Err(anyhow!(
            "evidence path is outside workflow artifact directory: {}",
            raw_path
        ));
    }
    if metadata.len() == 0 || metadata.len() > MAX_EVIDENCE_BYTES {
        return Err(anyhow!(
            "evidence file size must be between 1 and {} bytes: {}",
            MAX_EVIDENCE_BYTES,
            raw_path
        ));
    }
    let contents = fs::read(&canonical)?;
    if workflow_evidence_is_placeholder(&contents) {
        return Err(anyhow!("evidence file is a placeholder: {raw_path}"));
    }
    let path = relative
        .components()
        .filter_map(|component| match component {
            Component::CurDir => None,
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    Ok(WorkflowTransitionEvidence {
        path,
        bytes: contents.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&contents)),
    })
}

fn workflow_evidence_is_placeholder(contents: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(contents) else {
        return false;
    };
    let lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() || lines.iter().all(|line| line.starts_with('#')) {
        return true;
    }
    let normalized = lines.join(" ").to_ascii_lowercase();
    matches!(normalized.as_str(), "todo" | "tbd" | "placeholder" | "n/a")
        || normalized.contains("TODO: replace")
}

fn validate_workflow_artifact_id(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.contains("..")
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err(anyhow!("{label} id is invalid"));
    }
    Ok(())
}

struct WorkflowInitArgs {
    json_output: bool,
    input_kind: WorkflowInputKind,
    profile: WorkflowProfile,
    approval_required: bool,
    request: String,
}

fn parse_workflow_init_args(rest: &str) -> Result<WorkflowInitArgs> {
    let mut json_output = false;
    let mut input_kind = WorkflowInputKind::Task;
    let mut profile = WorkflowProfile::Standard;
    let mut approval_required = false;
    let mut request_parts = Vec::new();
    let mut parts = rest.split_whitespace();

    while let Some(part) = parts.next() {
        match part {
            "--json" | "-j" => json_output = true,
            "--approval-required" => approval_required = true,
            "--type" | "--kind" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("{} requires a value", part))?;
                input_kind = parse_workflow_input_kind(value)?;
            }
            "--profile" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--profile requires a value"))?;
                profile = parse_workflow_profile(value)?;
            }
            value if value.starts_with('-') => {
                return Err(anyhow!("unknown workflow init option '{}'", value));
            }
            value => {
                request_parts.push(value.to_string());
                request_parts.extend(parts.map(str::to_string));
                break;
            }
        }
    }

    let request = request_parts.join(" ").trim().to_string();
    if request.is_empty() {
        return Err(anyhow!(
            "workflow init requires a request\n\n{}",
            workflow_usage()
        ));
    }
    Ok(WorkflowInitArgs {
        json_output,
        input_kind,
        profile,
        approval_required,
        request,
    })
}

fn parse_workflow_show_args(rest: &str) -> Result<(bool, String)> {
    let mut json_output = false;
    let mut run_id = None;
    for part in rest.split_whitespace() {
        match part {
            "--json" | "-j" => json_output = true,
            value if run_id.is_none() => run_id = Some(value.to_string()),
            _ => return Err(anyhow!("{}", workflow_usage())),
        }
    }
    let run_id = run_id.ok_or_else(|| anyhow!("workflow show requires a run_id"))?;
    Ok((json_output, run_id))
}

fn parse_workflow_continue_args(rest: &str) -> Result<(bool, Option<String>)> {
    let mut json_output = false;
    let mut run_id = None;
    for part in rest.split_whitespace() {
        match part {
            "--json" | "-j" => json_output = true,
            value if value.starts_with('-') => {
                return Err(anyhow!("unknown workflow continue option '{}'", value));
            }
            value if run_id.is_none() => run_id = Some(value.to_string()),
            _ => return Err(anyhow!("{}", workflow_usage())),
        }
    }
    Ok((json_output, run_id))
}

fn parse_json_only_flag(rest: &str) -> Result<bool> {
    let mut json_output = false;
    for part in rest.split_whitespace() {
        match part {
            "--json" | "-j" => json_output = true,
            _ => return Err(anyhow!("{}", workflow_usage())),
        }
    }
    Ok(json_output)
}

fn parse_workflow_input_kind(value: &str) -> Result<WorkflowInputKind> {
    match normalize_key(value).as_str() {
        "feature" => Ok(WorkflowInputKind::Feature),
        "bug" | "error" => Ok(WorkflowInputKind::Bug),
        "refactor" => Ok(WorkflowInputKind::Refactor),
        "research" => Ok(WorkflowInputKind::Research),
        "prd" => Ok(WorkflowInputKind::Prd),
        "issue" => Ok(WorkflowInputKind::Issue),
        "qa" => Ok(WorkflowInputKind::Qa),
        "review" => Ok(WorkflowInputKind::Review),
        "eda" | "hardware" | "pcb" => Ok(WorkflowInputKind::Eda),
        "ship" | "deploy" | "release" => Ok(WorkflowInputKind::Ship),
        "task" => Ok(WorkflowInputKind::Task),
        _ => Err(anyhow!("unknown workflow type '{}'", value)),
    }
}

fn parse_workflow_profile(value: &str) -> Result<WorkflowProfile> {
    match normalize_key(value).as_str() {
        "quick" => Ok(WorkflowProfile::Quick),
        "standard" | "default" => Ok(WorkflowProfile::Standard),
        "gated" | "gate" => Ok(WorkflowProfile::Gated),
        _ => Err(anyhow!("unknown workflow profile '{}'", value)),
    }
}

fn workflow_run_dir(context: &CommandContext, run_id: &str) -> Result<PathBuf> {
    if run_id.contains('/') || run_id.contains('\\') || run_id.contains("..") {
        return Err(anyhow!("run_id is invalid"));
    }
    Ok(cwd(context).join(".kiana").join("workflows").join(run_id))
}

pub fn tasks_report(context: &CommandContext, task_list: Option<&str>) -> Result<Value> {
    let tasks = load_tasks(context, task_list)?;
    let task_list_id = task_list_id(context, task_list);
    let tasks_dir = task_list_dir(context, task_list);
    let mut status_counts = BTreeMap::new();
    for task in &tasks {
        let status = task_status(task).unwrap_or("unknown").to_string();
        *status_counts.entry(status).or_insert(0usize) += 1;
    }
    Ok(json!({
        "schema": "kiana.tasks.v1",
        "task_list_id": task_list_id,
        "tasks_dir": tasks_dir.display().to_string(),
        "count": tasks.len(),
        "status_counts": status_counts,
        "tasks": tasks,
    }))
}

pub fn team_plan_report(context: &CommandContext, task_list: Option<&str>) -> Result<Value> {
    let tasks_report = tasks_report(context, task_list)?;
    let task_list_id = tasks_report["task_list_id"]
        .as_str()
        .unwrap_or("default")
        .to_string();
    let tasks = tasks_report["tasks"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let role_profiles = [
        ("pm", "PM", ["pm", "product-manager", "product-owner"]),
        (
            "architect",
            "Architect",
            ["architect", "architecture", "tech-lead"],
        ),
        ("engineer", "Engineer", ["engineer", "developer", "coder"]),
        ("qa", "QA", ["qa", "tester", "quality-assurance"]),
        (
            "data-analyst",
            "Data Analyst",
            ["data-analyst", "analyst", "data-scientist"],
        ),
    ];
    let required_artifact_roles = ["prd", "design", "tasks", "source", "test"];
    let mut role_rows = Vec::new();
    let mut missing_roles = Vec::new();
    let mut role_task_count = 0usize;
    for (role_id, display_name, aliases) in role_profiles {
        let mut task_ids = Vec::new();
        let mut open_task_count = 0usize;
        let mut completed_task_count = 0usize;
        for task in &tasks {
            if task_owner_role(task).as_deref() != Some(role_id) {
                continue;
            }
            if let Some(id) = task_id(task) {
                task_ids.push(id.to_string());
            }
            role_task_count += 1;
            if task_status(task).is_some_and(|status| status == "completed") {
                completed_task_count += 1;
            } else if task_status(task).map_or(true, is_open_status) {
                open_task_count += 1;
            }
        }
        let present = !task_ids.is_empty();
        if !present {
            missing_roles.push(role_id.to_string());
        }
        role_rows.push(json!({
            "id": role_id,
            "name": display_name,
            "aliases": aliases,
            "present": present,
            "task_count": task_ids.len(),
            "open_task_count": open_task_count,
            "completed_task_count": completed_task_count,
            "task_ids": task_ids,
        }));
    }

    let mut artifact_rows = Vec::new();
    let mut missing_artifact_roles = Vec::new();
    for artifact_role in required_artifact_roles {
        let mut task_ids = Vec::new();
        for task in &tasks {
            if task_artifact_role(task).as_deref() == Some(artifact_role) {
                if let Some(id) = task_id(task) {
                    task_ids.push(id.to_string());
                }
            }
        }
        let present = !task_ids.is_empty();
        if !present {
            missing_artifact_roles.push(artifact_role.to_string());
        }
        artifact_rows.push(json!({
            "role": artifact_role,
            "present": present,
            "count": task_ids.len(),
            "task_ids": task_ids,
        }));
    }

    let blocked_task_count = tasks
        .iter()
        .filter(|task| {
            task_string_array(task, "blockedBy", "blocked_by")
                .is_some_and(|items| !items.is_empty())
        })
        .count();
    let dependency_edge_count = tasks
        .iter()
        .map(|task| {
            task_string_array(task, "blocks", "blocks")
                .map(|items| items.len())
                .unwrap_or_default()
                + task_string_array(task, "blockedBy", "blocked_by")
                    .map(|items| items.len())
                    .unwrap_or_default()
        })
        .sum::<usize>();
    let artifacts_ready = missing_artifact_roles.is_empty();
    let roles_ready = missing_roles.is_empty();
    let ready_for_fake_runtime = roles_ready && artifacts_ready && role_task_count > 0;
    Ok(json!({
        "schema": "kiana.team-plan.v1",
        "workspace": cwd(context).display().to_string(),
        "team": team_metadata(context, &task_list_id),
        "task_list": {
            "id": tasks_report["task_list_id"].clone(),
            "tasks_dir": tasks_report["tasks_dir"].clone(),
            "count": tasks_report["count"].clone(),
            "status_counts": tasks_report["status_counts"].clone(),
        },
        "role_runtime": {
            "status": if ready_for_fake_runtime { "ready" } else { "incomplete" },
            "ready_for_fake_runtime": ready_for_fake_runtime,
            "required_roles": role_profiles.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
            "missing_roles": missing_roles,
            "role_task_count": role_task_count,
            "blocked_task_count": blocked_task_count,
            "dependency_edge_count": dependency_edge_count,
        },
        "roles": role_rows,
        "artifact_readiness": {
            "status": if artifacts_ready { "ready" } else { "incomplete" },
            "required_roles": artifact_rows,
            "missing_roles": missing_artifact_roles,
        },
        "tasks": tasks_report,
    }))
}

fn show_task(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let (task_id, task_list) = parse_show_args(rest)?;
    let tasks = load_tasks(context, task_list)?;
    let Some(task) = tasks.iter().find(|task| task_id_matches(task, &task_id)) else {
        return Err(anyhow!(
            "task '{}' was not found in task list '{}'",
            task_id,
            task_list_id(context, task_list)
        ));
    };
    Ok(CommandResult::text(serde_json::to_string_pretty(task)?))
}

pub(crate) fn load_tasks(context: &CommandContext, task_list: Option<&str>) -> Result<Vec<Value>> {
    let mut tasks = read_disk_tasks(&task_list_dir(context, task_list))?;
    if task_list.is_none() {
        let state_tasks = context
            .app_state
            .get("tasks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for task in state_tasks {
            if !tasks
                .iter()
                .any(|existing| task_id(existing) == task_id(&task))
            {
                tasks.push(task);
            }
        }
    }

    tasks.sort_by(compare_tasks);
    Ok(tasks)
}

#[derive(Debug)]
pub(crate) struct TaskEvidenceLease {
    lock_path: PathBuf,
    task_list_dir: PathBuf,
}

impl Drop for TaskEvidenceLease {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

pub(crate) fn acquire_task_evidence_lease(
    context: &CommandContext,
    task_list: Option<&str>,
    requested_task_id: &str,
) -> Result<TaskEvidenceLease> {
    let dir = task_list_dir(context, task_list);
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let lock_path = dir.join(".evidence-update.lock");
    let mut lock = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                anyhow!(
                    "task_evidence_busy: another evidence update holds {}",
                    lock_path.display()
                )
            } else {
                anyhow!("failed to acquire {}: {error}", lock_path.display())
            }
        })?;
    writeln!(lock, "pid={} task={requested_task_id}", std::process::id())?;
    lock.flush()?;
    let lease = TaskEvidenceLease {
        lock_path,
        task_list_dir: dir,
    };
    validate_task_evidence_target(context, task_list, requested_task_id)?;
    Ok(lease)
}

pub(crate) fn append_task_evidence_reference(
    context: &CommandContext,
    task_list: Option<&str>,
    requested_task_id: &str,
    evidence_reference: &str,
    lease: &TaskEvidenceLease,
) -> Result<PathBuf> {
    let expected_dir = task_list_dir(context, task_list);
    if lease.task_list_dir != expected_dir || !lease.lock_path.is_file() {
        return Err(anyhow!(
            "task_evidence_lease_invalid: evidence update lease does not cover {}",
            expected_dir.display()
        ));
    }
    let (path, mut task) = resolve_task_evidence_target(context, task_list, requested_task_id)?;
    validate_task_evidence_shape(requested_task_id, &task)?;
    let object = task
        .as_object_mut()
        .expect("validated task evidence target must be an object");
    let metadata = object
        .entry("metadata".to_string())
        .or_insert_with(|| json!({}));
    let metadata = metadata
        .as_object_mut()
        .expect("validated task metadata must be an object");
    let evidence = metadata
        .entry("evidence".to_string())
        .or_insert_with(|| json!([]));
    let evidence = evidence
        .as_array_mut()
        .expect("validated task evidence must be an array");
    if !evidence
        .iter()
        .any(|value| value.as_str() == Some(evidence_reference))
    {
        evidence.push(json!(evidence_reference));
    }

    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = path.with_extension(format!("json.{}.{unique}.tmp", std::process::id()));
    let mut bytes = serde_json::to_vec_pretty(&task)?;
    bytes.push(b'\n');
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .with_context(|| format!("failed to create {}", temporary.display()))?;
    file.write_all(&bytes)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temporary, &path).with_context(|| {
        format!(
            "failed to replace task file {} from {}",
            path.display(),
            temporary.display()
        )
    })?;
    Ok(path)
}

pub(crate) fn validate_task_evidence_target(
    context: &CommandContext,
    task_list: Option<&str>,
    requested_task_id: &str,
) -> Result<()> {
    let (_, task) = resolve_task_evidence_target(context, task_list, requested_task_id)?;
    validate_task_evidence_shape(requested_task_id, &task)
}

fn resolve_task_evidence_target(
    context: &CommandContext,
    task_list: Option<&str>,
    requested_task_id: &str,
) -> Result<(PathBuf, Value)> {
    let dir = task_list_dir(context, task_list);
    let mut matches = Vec::new();
    if dir.is_dir() {
        for entry in
            std::fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let value: Value = serde_json::from_str(&contents)
                .with_context(|| format!("failed to parse {}", path.display()))?;
            if task_id_matches(&value, requested_task_id) {
                matches.push((path, value));
            }
        }
    }
    if matches.is_empty() {
        return Err(anyhow!(
            "task_not_found: task '{}' was not found in task list '{}'",
            requested_task_id,
            task_list_id(context, task_list)
        ));
    }
    if matches.len() > 1 {
        return Err(anyhow!(
            "task_identity_conflict: task '{}' matched multiple files in {}",
            requested_task_id,
            dir.display()
        ));
    }

    Ok(matches.pop().expect("one task match"))
}

fn validate_task_evidence_shape(requested_task_id: &str, task: &Value) -> Result<()> {
    let object = task
        .as_object()
        .ok_or_else(|| anyhow!("task '{}' is not a JSON object", requested_task_id))?;
    let Some(metadata) = object.get("metadata") else {
        return Ok(());
    };
    let metadata = metadata.as_object().ok_or_else(|| {
        anyhow!(
            "task '{}' metadata must be a JSON object",
            requested_task_id
        )
    })?;
    let Some(evidence) = metadata.get("evidence") else {
        return Ok(());
    };
    let evidence = evidence.as_array().ok_or_else(|| {
        anyhow!(
            "task '{}' metadata.evidence must be an array",
            requested_task_id
        )
    })?;
    if evidence.iter().any(|value| !value.is_string()) {
        return Err(anyhow!(
            "task '{}' metadata.evidence must contain only strings",
            requested_task_id
        ));
    }
    Ok(())
}

fn read_disk_tasks(dir: &Path) -> Result<Vec<Value>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut tasks = Vec::new();
    for entry in
        std::fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let mut value: Value = serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        if value.get("subject").is_none() {
            if let Some(title) = value.get("title").cloned() {
                value["subject"] = title;
            }
        }
        tasks.push(value);
    }
    Ok(tasks)
}

fn task_list_dir(context: &CommandContext, task_list: Option<&str>) -> PathBuf {
    tasks_root(context).join(sanitize_path_component(&task_list_id(context, task_list)))
}

fn tasks_root(context: &CommandContext) -> PathBuf {
    if let Some(path) = context
        .app_state
        .get("tasks_root")
        .or_else(|| context.app_state.get("tasksRoot"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        return resolve_path(context, path);
    }
    if let Some(path) = std::env::var_os("KIANA_TASKS_ROOT") {
        let path = PathBuf::from(path);
        return if path.is_absolute() {
            path
        } else {
            cwd(context).join(path)
        };
    }
    cwd(context).join(".kiana").join("tasks")
}

pub(crate) fn task_list_id(context: &CommandContext, explicit: Option<&str>) -> String {
    let task_list_id = explicit
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            context
                .app_state
                .get("task_list_id")
                .or_else(|| context.app_state.get("taskListId"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            context
                .app_state
                .get("team_context")
                .or_else(|| context.app_state.get("teamContext"))
                .and_then(Value::as_object)
                .and_then(|team| {
                    team.get("team_name")
                        .or_else(|| team.get("teamName"))
                        .and_then(Value::as_str)
                })
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            context
                .app_state
                .get("session_id")
                .or_else(|| context.app_state.get("sessionId"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "default".to_string());
    sanitize_path_component(&task_list_id)
}

fn cwd(context: &CommandContext) -> PathBuf {
    context
        .app_state
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn resolve_path(context: &CommandContext, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        cwd(context).join(path)
    }
}

fn format_task_list(tasks: &[Value], task_list_id: &str, status: Option<&str>) -> String {
    let mut lines = vec![match status {
        Some(status) => format!(
            "{} task(s) in '{}' with status '{}':",
            tasks.len(),
            task_list_id,
            status
        ),
        None => format!("{} task(s) in '{}':", tasks.len(), task_list_id),
    }];

    for task in tasks.iter().take(20) {
        let id = task_id(task).unwrap_or("<unknown>");
        let title = task_title(task);
        let status = task_status(task).unwrap_or("unknown");
        let owner = task
            .get("owner")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("-");
        let blocked_by = task
            .get("blockedBy")
            .or_else(|| task.get("blocked_by"))
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        lines.push(format!(
            "- {} [{}] owner={} blocked_by={} {}",
            id, status, owner, blocked_by, title
        ));
    }
    if tasks.len() > 20 {
        lines.push(format!("... {} more", tasks.len() - 20));
    }
    lines.push("usage: kiana tasks show <id> | json | <status>".to_string());
    lines.join("\n")
}

fn task_id(task: &Value) -> Option<&str> {
    task.get("id")
        .or_else(|| task.get("task_id"))
        .or_else(|| task.get("taskId"))
        .and_then(Value::as_str)
}

fn task_id_matches(task: &Value, expected_id: &str) -> bool {
    task_id(task).is_some_and(|id| id == expected_id)
}

fn task_title(task: &Value) -> &str {
    task.get("subject")
        .or_else(|| task.get("title"))
        .or_else(|| task.get("description"))
        .and_then(Value::as_str)
        .unwrap_or("<untitled>")
}

fn task_status(task: &Value) -> Option<&str> {
    task.get("status").and_then(Value::as_str)
}

fn task_owner_role(task: &Value) -> Option<String> {
    let owner = task.get("owner").and_then(Value::as_str).or_else(|| {
        task.get("metadata")
            .and_then(Value::as_object)
            .and_then(|metadata| metadata.get("owner").and_then(Value::as_str))
    })?;
    canonical_role_id(owner)
}

fn canonical_role_id(value: &str) -> Option<String> {
    match normalize_key(value).as_str() {
        "pm" | "product-manager" | "product-owner" => Some("pm".to_string()),
        "architect" | "architecture" | "tech-lead" => Some("architect".to_string()),
        "engineer" | "developer" | "coder" => Some("engineer".to_string()),
        "qa" | "tester" | "quality-assurance" => Some("qa".to_string()),
        "data-analyst" | "analyst" | "data-scientist" => Some("data-analyst".to_string()),
        _ => None,
    }
}

fn task_artifact_role(task: &Value) -> Option<String> {
    let value = task
        .get("artifact_role")
        .or_else(|| task.get("artifactRole"))
        .and_then(Value::as_str)
        .or_else(|| {
            task.get("metadata")
                .and_then(Value::as_object)
                .and_then(|metadata| {
                    metadata
                        .get("artifact_role")
                        .or_else(|| metadata.get("artifactRole"))
                        .and_then(Value::as_str)
                })
        })?;
    canonical_artifact_role(value)
}

fn canonical_artifact_role(value: &str) -> Option<String> {
    match normalize_key(value).as_str() {
        "prd" | "requirements" | "product-requirements" => Some("prd".to_string()),
        "design" | "architecture" | "spec" => Some("design".to_string()),
        "task" | "tasks" | "plan" => Some("tasks".to_string()),
        "source" | "src" | "code" => Some("source".to_string()),
        "test" | "tests" | "qa" => Some("test".to_string()),
        _ => None,
    }
}

fn normalize_key(value: &str) -> String {
    let mut normalized = String::new();
    let mut last_dash = false;
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            normalized.push('-');
            last_dash = true;
        }
    }
    normalized.trim_matches('-').to_string()
}

fn is_open_status(status: &str) -> bool {
    !matches!(status, "completed" | "cancelled" | "killed")
}

fn task_string_array(task: &Value, camel_key: &str, snake_key: &str) -> Option<Vec<String>> {
    task.get(camel_key)
        .or_else(|| task.get(snake_key))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
}

fn team_metadata(context: &CommandContext, task_list_id: &str) -> Value {
    let team_name = context
        .app_state
        .get("team_context")
        .or_else(|| context.app_state.get("teamContext"))
        .and_then(Value::as_object)
        .and_then(|team| {
            team.get("team_name")
                .or_else(|| team.get("teamName"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let source = if team_name.is_some() {
        "team_context"
    } else if context.app_state.contains_key("task_list_id")
        || context.app_state.contains_key("taskListId")
    {
        "task_list_id"
    } else if context.app_state.contains_key("session_id")
        || context.app_state.contains_key("sessionId")
    {
        "session_id"
    } else {
        "default_task_list"
    };
    json!({
        "name": team_name.unwrap_or(task_list_id),
        "source": source,
    })
}

fn compare_tasks(a: &Value, b: &Value) -> std::cmp::Ordering {
    let a_id = task_id(a).unwrap_or_default();
    let b_id = task_id(b).unwrap_or_default();
    let a_num = a_id.parse::<i64>().ok();
    let b_num = b_id.parse::<i64>().ok();
    a_num.cmp(&b_num).then_with(|| a_id.cmp(b_id))
}

fn optional_task_list_arg(rest: &str) -> Result<Option<&str>> {
    let mut parts = rest.trim().split_whitespace();
    let task_list = parts.next();
    if parts.next().is_some() {
        return Err(anyhow!("{}", usage()));
    }
    Ok(task_list)
}

fn parse_show_args(rest: &str) -> Result<(String, Option<&str>)> {
    let mut parts = rest.split_whitespace();
    let task_id = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("tasks show requires a task id"))?;
    if task_id.contains('/') || task_id.contains('\\') || task_id.contains("..") {
        return Err(anyhow!("task id is invalid"));
    }
    let task_list = parts.next();
    if parts.next().is_some() {
        return Err(anyhow!("{}", usage()));
    }
    Ok((task_id.to_string(), task_list))
}

fn parse_plan_args(rest: &str) -> Result<(bool, Option<&str>)> {
    let mut json_output = false;
    let mut task_list = None;
    for part in rest.split_whitespace() {
        match part {
            "--json" | "-j" => json_output = true,
            value if task_list.is_none() => task_list = Some(value),
            _ => return Err(anyhow!("{}", usage())),
        }
    }
    Ok((json_output, task_list))
}

fn split_word(value: &str) -> (Option<&str>, &str) {
    let value = value.trim_start();
    if value.is_empty() {
        return (None, "");
    }
    let Some((word, rest)) = value.split_once(char::is_whitespace) else {
        return (Some(value), "");
    };
    (Some(word), rest.trim_start())
}

fn looks_like_status(value: &str) -> bool {
    matches!(
        value,
        "pending" | "in_progress" | "running" | "completed" | "failed" | "cancelled" | "killed"
    )
}

fn sanitize_path_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

fn usage() -> &'static str {
    "Usage:\n  kiana tasks [list|status] [task_list_id]\n  kiana tasks json [task_list_id]\n  kiana tasks plan [--json] [task_list_id]\n  kiana tasks swarm plan [--json] [--max-workers <2..32>] [task_list_id]\n  kiana tasks swarm dispatch --workflow <run_id> [--json] [budget options] [task_list_id]\n  kiana tasks swarm start|status|monitor --workflow <run_id> --dispatch <dispatch_id> [--json]\n  kiana tasks swarm cancel --workflow <run_id> --dispatch <dispatch_id> [--task <task_id>] [--json]\n  kiana tasks workflow <template|init|show> ...\n  kiana tasks show <task_id> [task_list_id]\n  kiana tasks path [task_list_id]\n  kiana tasks <pending|in_progress|running|completed|failed|cancelled|killed> [task_list_id]"
}

fn swarm_usage() -> &'static str {
    "Usage:\n  kiana tasks swarm plan [--json] [--max-workers <2..32>] [task_list_id]\n  kiana tasks swarm dispatch --workflow <run_id> [--json] [--max-workers <2..32>] [--max-attempts <1..3>] [--max-commands <1..100>] [--timeout-seconds <1..86400>] [--max-output-bytes <1024..104857600>] [task_list_id]\n  kiana tasks swarm start --workflow <run_id> --dispatch <dispatch_id> [--json]\n  kiana tasks swarm status --workflow <run_id> --dispatch <dispatch_id> [--json]\n  kiana tasks swarm monitor --workflow <run_id> --dispatch <dispatch_id> [--json]\n  kiana tasks swarm cancel --workflow <run_id> --dispatch <dispatch_id> [--task <task_id>] [--json]\n  kiana tasks swarm integrate <plan|apply|status> --workflow <run_id> --dispatch <dispatch_id> [--json]\n  kiana tasks swarm cleanup --workflow <run_id> --dispatch <dispatch_id> [--json]"
}

fn workflow_usage() -> &'static str {
    "Usage:\n  kiana tasks workflow template [--json]\n  kiana tasks workflow init [--json] [--type <feature|bug|refactor|research|prd|issue|qa|review|ship|task>] [--profile <quick|standard|gated>] [--approval-required] <request>\n  kiana tasks workflow list [--json]\n  kiana tasks workflow continue [--json] [run_id]\n  kiana tasks workflow show [--json] <run_id>\n  kiana tasks workflow advance --decision <decision> --evidence <workflow-relative-path>... [--note <text>] [--json] <run_id>\n  kiana tasks workflow complete --verification <verification_id> [--json] <run_id>\n  kiana tasks workflow integrity <init|status|verify|seal> ..."
}

fn workflow_integrity_usage() -> &'static str {
    "Usage:\n  kiana tasks workflow integrity init [--json]\n  kiana tasks workflow integrity status [--json]\n  kiana tasks workflow integrity verify [--json] [run_id]\n  kiana tasks workflow integrity seal [--json] <run_id>"
}

#[cfg(test)]
mod tests {
    use super::{
        acquire_task_evidence_lease, append_task_evidence_reference, git_state_snapshot,
        integration_path_descriptor, restore_verified_integration_checkpoint,
        swarm_integration_platform_capability, TasksCommand, VerifiedCheckpointContent,
        VerifiedCheckpointEntry, VerifiedIntegrationCheckpoint,
    };
    #[cfg(target_os = "linux")]
    use super::{
        archive_completed_integration_recovery_journal, checkpoint_metadata_at_path,
        classify_integration_recovery_journal_revision,
        commit_integration_recovery_journal_revision, create_integration_recovery_journal,
        current_working_tree_fingerprint, inspect_swarm_recovery_integrity,
        integration_recovery_journal_binding_sha256, integration_recovery_journal_path,
        integration_recovery_journal_record_sha256, latest_recovery_revision_anchor_from_events,
        linux_create_named_file, linux_create_temp_file, linux_descriptor_at,
        linux_install_temp_entry, linux_open_parent_beneath, linux_open_project_root,
        linux_remove_expected_entry, linux_renameat2, linux_sync_directory,
        restore_verified_integration_checkpoint_with_journal, sign_integration_recovery_journal,
        verify_integration_recovery_journal_mac, IntegrationRecoveryEntryStatus,
        IntegrationRecoveryJournal, RecoveryRevisionAnchor, RecoveryRevisionState,
        INTEGRATION_RECOVERY_JOURNAL_GENESIS_SHA256, LINUX_RENAME_EXCHANGE, LINUX_RENAME_NOREPLACE,
    };
    use crate::{Command, CommandContext};
    use kiana_tasks::{append_workflow_event, WorkflowEventKind};
    #[cfg(target_os = "linux")]
    use kiana_tasks::{
        initialize_local_hmac_key_at, initialize_workflow_run, load_local_hmac_key_at,
        WorkflowEvent, WorkflowInit, WorkflowInputKind, WorkflowProfile,
    };
    use serde_json::{json, Value};
    use std::collections::HashMap;
    #[cfg(target_os = "linux")]
    use std::ffi::CString;
    use std::ffi::OsString;
    #[cfg(target_os = "linux")]
    use std::os::fd::AsRawFd;
    #[cfg(target_os = "linux")]
    use std::os::unix::ffi::OsStrExt;
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        MutexGuard, OnceLock,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn git_apply_uses_the_same_verified_patch_bytes_after_path_replacement() {
        let root = temp_root();
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "kiana-tests@example.invalid"]);
        git(&["config", "user.name", "Kiana Tests"]);
        std::fs::write(root.join("value.txt"), "base\n").unwrap();
        git(&["add", "value.txt"]);
        git(&["commit", "-qm", "base"]);

        let good_patch = b"diff --git a/value.txt b/value.txt\n--- a/value.txt\n+++ b/value.txt\n@@ -1 +1 @@\n-base\n+good\n";
        let evil_patch = b"diff --git a/value.txt b/value.txt\n--- a/value.txt\n+++ b/value.txt\n@@ -1 +1 @@\n-base\n+evil\n";
        let patch_path = root.join("worker.patch");
        std::fs::write(&patch_path, good_patch).unwrap();
        let verified = super::read_regular_file_nofollow(&patch_path).unwrap();
        std::fs::write(&patch_path, evil_patch).unwrap();

        super::git_apply_check_bytes(&root, &verified).unwrap();
        super::git_apply_patch_bytes(&root, &verified).unwrap();

        assert_eq!(
            std::fs::read_to_string(root.join("value.txt")).unwrap(),
            "good\n"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn git_state_snapshot_hashes_untracked_file_contents() {
        let root = temp_root();
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "kiana-tests@example.invalid"]);
        git(&["config", "user.name", "Kiana Tests"]);
        std::fs::write(root.join("tracked.txt"), "base\n").unwrap();
        git(&["add", "tracked.txt"]);
        git(&["commit", "-qm", "base"]);

        let untracked = root.join("untracked.txt");
        std::fs::write(&untracked, "alpha\n").unwrap();
        let first = git_state_snapshot(&root).unwrap();

        std::fs::write(&untracked, "bravo\n").unwrap();
        let second = git_state_snapshot(&root).unwrap();

        assert_ne!(first.status_sha256, second.status_sha256);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn swarm_integration_apply_lease_rejects_concurrent_holder_and_releases_on_drop() {
        let integration_dir = temp_root();
        std::fs::create_dir_all(&integration_dir).unwrap();

        let first = super::acquire_swarm_integration_apply_lease(&integration_dir).unwrap();
        let error = super::acquire_swarm_integration_apply_lease(&integration_dir).unwrap_err();
        assert!(error.to_string().contains("already active"), "{error:#}");

        drop(first);
        super::acquire_swarm_integration_apply_lease(&integration_dir).unwrap();
        let _ = std::fs::remove_dir_all(integration_dir);
    }

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    static TEST_WORKFLOW_INTEGRITY_KEY: OnceLock<PathBuf> = OnceLock::new();

    struct TestWorkflowIntegrityEnv {
        previous_key_file: Option<OsString>,
        _lock: MutexGuard<'static, ()>,
    }

    impl TestWorkflowIntegrityEnv {
        fn without_key() -> Self {
            let lock = crate::test_env_lock()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let previous_key_file = std::env::var_os("KIANA_WORKFLOW_INTEGRITY_KEY_FILE");
            std::env::remove_var("KIANA_WORKFLOW_INTEGRITY_KEY_FILE");
            Self {
                previous_key_file,
                _lock: lock,
            }
        }

        #[cfg(target_os = "linux")]
        fn with_key() -> Self {
            let lock = crate::test_env_lock()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let previous_key_file = std::env::var_os("KIANA_WORKFLOW_INTEGRITY_KEY_FILE");
            let path = TEST_WORKFLOW_INTEGRITY_KEY.get_or_init(|| {
                let path = temp_root().join("trust/workflow-integrity-key.json");
                initialize_local_hmac_key_at(&path).unwrap();
                path
            });
            std::env::set_var("KIANA_WORKFLOW_INTEGRITY_KEY_FILE", path);
            Self {
                previous_key_file,
                _lock: lock,
            }
        }
    }

    impl Drop for TestWorkflowIntegrityEnv {
        fn drop(&mut self) {
            match self.previous_key_file.take() {
                Some(value) => std::env::set_var("KIANA_WORKFLOW_INTEGRITY_KEY_FILE", value),
                None => std::env::remove_var("KIANA_WORKFLOW_INTEGRITY_KEY_FILE"),
            }
        }
    }

    fn temp_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "kiana-tasks-command-{}-{unique}-{counter}",
            std::process::id(),
        ))
    }

    #[cfg(target_os = "linux")]
    fn sample_recovery_journal() -> IntegrationRecoveryJournal {
        IntegrationRecoveryJournal {
            schema: super::INTEGRATION_RECOVERY_JOURNAL_SCHEMA.to_string(),
            revision: 0,
            integration_id: "integration-test".to_string(),
            dispatch_id: "dispatch-test".to_string(),
            plan_sha256: "sha256:plan".to_string(),
            checkpoint_path: "checkpoint/manifest.json".to_string(),
            checkpoint_sha256: "sha256:checkpoint".to_string(),
            expected_pre_restore_fingerprint: "sha256:before".to_string(),
            expected_post_restore_fingerprint: "sha256:after".to_string(),
            source_manifest_sha256: "sha256:manifest".to_string(),
            source_git_repository: false,
            source_git_head: String::new(),
            source_git_index_diff_sha256: "sha256:index".to_string(),
            status: "pending".to_string(),
            next_entry_index: 0,
            entries: vec![super::IntegrationRecoveryJournalEntry {
                path: "src/lib.rs".to_string(),
                source_descriptor: "file:source".to_string(),
                restored_descriptor: "file:restored".to_string(),
                operation: super::IntegrationRecoveryOperation::InstallFile,
                auxiliary_name: ".kiana-recovery-journal-test-0".to_string(),
                status: IntegrationRecoveryEntryStatus::Pending,
            }],
            last_error: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            integrity: None,
        }
    }

    #[cfg(target_os = "linux")]
    fn authenticated_recovery_fixture(
        status: &str,
    ) -> (
        TestWorkflowIntegrityEnv,
        PathBuf,
        PathBuf,
        PathBuf,
        IntegrationRecoveryJournal,
    ) {
        let env = TestWorkflowIntegrityEnv::with_key();
        let root = temp_root();
        std::fs::create_dir_all(&root).unwrap();
        let run = initialize_workflow_run(
            &root,
            WorkflowInit {
                request: "inspect authenticated recovery journal".to_string(),
                input_kind: WorkflowInputKind::Review,
                profile: WorkflowProfile::Gated,
                approval_required: false,
            },
        )
        .unwrap();
        let integration_dir = run.artifact_dir.join("integrations/integration-test");
        let journal_path = integration_recovery_journal_path(&integration_dir);
        std::fs::create_dir_all(journal_path.parent().unwrap()).unwrap();
        let mut journal = sample_recovery_journal();
        journal.status = status.to_string();
        commit_integration_recovery_journal_revision(
            &run.artifact_dir,
            &journal_path,
            &mut journal,
        )
        .unwrap();
        (env, root, run.artifact_dir, journal_path, journal)
    }

    fn context(args: &str, app_state: HashMap<String, Value>) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state,
        }
    }

    #[test]
    fn swarm_integration_platform_capability_is_explicit() {
        let capability = swarm_integration_platform_capability();
        assert_eq!(
            capability["schema"],
            "kiana.swarm-integration-platform-capability.v1"
        );
        assert_eq!(capability["platform"], std::env::consts::OS);
        #[cfg(target_os = "linux")]
        {
            assert_eq!(capability["secure_recovery"], "supported");
            assert_eq!(
                capability["recovery_backend"],
                "linux_openat2_renameat2_journal_v1"
            );
            assert_eq!(
                capability["supports_crash_resumable_multi_path_rollback"],
                true
            );
        }
        #[cfg(not(target_os = "linux"))]
        assert_eq!(capability["secure_recovery"], "unsupported_platform");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_journal_v2_serialization_reserves_revision_and_integrity() {
        let journal = sample_recovery_journal();
        let value = serde_json::to_value(journal).unwrap();

        assert_eq!(
            value["schema"],
            "kiana.swarm-integration-recovery-journal.v2"
        );
        assert_eq!(value["revision"], 0);
        assert!(value.get("integrity").is_some());
        assert!(value["integrity"].is_null());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_journal_revision_mac_covers_mutable_state() {
        let root = temp_root();
        let key_path = root.join("trust/workflow-integrity-key.json");
        initialize_local_hmac_key_at(&key_path).unwrap();
        let key = load_local_hmac_key_at(&key_path).unwrap();
        let mut journal = sample_recovery_journal();

        sign_integration_recovery_journal(
            &mut journal,
            &key,
            1,
            INTEGRATION_RECOVERY_JOURNAL_GENESIS_SHA256,
        )
        .unwrap();

        assert_eq!(journal.revision, 1);
        assert_eq!(
            journal.integrity.as_ref().unwrap().previous_record_sha256,
            INTEGRATION_RECOVERY_JOURNAL_GENESIS_SHA256
        );
        verify_integration_recovery_journal_mac(&journal, &key).unwrap();
        assert!(integration_recovery_journal_record_sha256(&journal)
            .unwrap()
            .starts_with("sha256:"));

        journal.status = "completed".to_string();
        assert!(verify_integration_recovery_journal_mac(&journal, &key).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_journal_binding_ignores_revision_and_integrity_envelope() {
        let root = temp_root();
        let key_path = root.join("trust/workflow-integrity-key.json");
        initialize_local_hmac_key_at(&key_path).unwrap();
        let key = load_local_hmac_key_at(&key_path).unwrap();
        let mut journal = sample_recovery_journal();

        sign_integration_recovery_journal(
            &mut journal,
            &key,
            1,
            INTEGRATION_RECOVERY_JOURNAL_GENESIS_SHA256,
        )
        .unwrap();
        let binding = integration_recovery_journal_binding_sha256(&journal).unwrap();
        let previous = integration_recovery_journal_record_sha256(&journal).unwrap();

        journal.status = "restoring".to_string();
        journal.updated_at_ms = 2;
        sign_integration_recovery_journal(&mut journal, &key, 2, &previous).unwrap();

        assert_eq!(
            integration_recovery_journal_binding_sha256(&journal).unwrap(),
            binding
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_journal_revision_anchor_detects_tail_replay_and_gap() {
        let root = temp_root();
        let key_path = root.join("trust/workflow-integrity-key.json");
        initialize_local_hmac_key_at(&key_path).unwrap();
        let key = load_local_hmac_key_at(&key_path).unwrap();
        let mut journal = sample_recovery_journal();

        sign_integration_recovery_journal(
            &mut journal,
            &key,
            1,
            INTEGRATION_RECOVERY_JOURNAL_GENESIS_SHA256,
        )
        .unwrap();
        let revision_one_hash = integration_recovery_journal_record_sha256(&journal).unwrap();
        let revision_one = RecoveryRevisionAnchor {
            revision: 1,
            record_sha256: revision_one_hash.clone(),
            previous_record_sha256: INTEGRATION_RECOVERY_JOURNAL_GENESIS_SHA256.to_string(),
        };
        assert_eq!(
            classify_integration_recovery_journal_revision(&journal, Some(&revision_one)).unwrap(),
            RecoveryRevisionState::Anchored
        );

        journal.status = "restoring".to_string();
        journal.updated_at_ms = 2;
        sign_integration_recovery_journal(&mut journal, &key, 2, &revision_one_hash).unwrap();
        assert_eq!(
            classify_integration_recovery_journal_revision(&journal, Some(&revision_one)).unwrap(),
            RecoveryRevisionState::RecoverableUnanchoredTail
        );

        let revision_two = RecoveryRevisionAnchor {
            revision: 2,
            record_sha256: integration_recovery_journal_record_sha256(&journal).unwrap(),
            previous_record_sha256: revision_one_hash,
        };
        let replayed = sample_recovery_journal();
        assert!(
            classify_integration_recovery_journal_revision(&replayed, Some(&revision_two))
                .unwrap_err()
                .to_string()
                .contains("integration_recovery_revision_replay")
        );

        journal.revision = 4;
        assert!(
            classify_integration_recovery_journal_revision(&journal, Some(&revision_one))
                .unwrap_err()
                .to_string()
                .contains("integration_recovery_revision_gap")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_journal_revision_anchor_reads_latest_matching_event() {
        let journal = sample_recovery_journal();
        let events = vec![
            WorkflowEvent {
                schema: "kiana.workflow-event.v2".to_string(),
                seq: 1,
                at_ms: 1,
                kind: WorkflowEventKind::SwarmIntegrationRecoveryRevisionCommitted,
                node_id: "swarm_integration".to_string(),
                data: json!({
                    "integration_id": journal.integration_id,
                    "checkpoint_sha256": journal.checkpoint_sha256,
                    "revision": 1,
                    "record_sha256": "sha256:one",
                    "previous_record_sha256": INTEGRATION_RECOVERY_JOURNAL_GENESIS_SHA256,
                }),
            },
            WorkflowEvent {
                schema: "kiana.workflow-event.v2".to_string(),
                seq: 2,
                at_ms: 2,
                kind: WorkflowEventKind::SwarmIntegrationRecoveryRevisionCommitted,
                node_id: "swarm_integration".to_string(),
                data: json!({
                    "integration_id": journal.integration_id,
                    "checkpoint_sha256": journal.checkpoint_sha256,
                    "revision": 2,
                    "record_sha256": "sha256:two",
                    "previous_record_sha256": "sha256:one",
                }),
            },
        ];

        assert_eq!(
            latest_recovery_revision_anchor_from_events(&events, &journal).unwrap(),
            Some(RecoveryRevisionAnchor {
                revision: 2,
                record_sha256: "sha256:two".to_string(),
                previous_record_sha256: "sha256:one".to_string(),
            })
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_journal_inspector_verifies_anchored_active_journal() {
        let (_env, root, artifact_dir, _, journal) = authenticated_recovery_fixture("pending");

        let report = inspect_swarm_recovery_integrity(&artifact_dir).unwrap();

        assert_eq!(report.status, "verified");
        assert_eq!(report.active_journal_count, 1);
        assert_eq!(report.verified_active_journal_count, 1);
        assert_eq!(report.archived_journal_count, 0);
        assert_eq!(report.mismatch_count, 0);
        assert_eq!(
            report.key_id.as_deref(),
            journal.integrity.as_ref().map(|i| i.key_id.as_str())
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_journal_inspector_rejects_tampered_active_journal() {
        let (_env, root, artifact_dir, journal_path, _) = authenticated_recovery_fixture("pending");
        let mut value: Value =
            serde_json::from_slice(&std::fs::read(&journal_path).unwrap()).unwrap();
        value["status"] = json!("completed");
        std::fs::write(&journal_path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let error = inspect_swarm_recovery_integrity(&artifact_dir)
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("integration_recovery_integrity_mismatch"),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_journal_inspector_rejects_replayed_revision() {
        let (_env, root, artifact_dir, journal_path, mut journal) =
            authenticated_recovery_fixture("pending");
        let revision_one = std::fs::read(&journal_path).unwrap();
        journal.status = "restoring".to_string();
        journal.updated_at_ms = 2;
        commit_integration_recovery_journal_revision(&artifact_dir, &journal_path, &mut journal)
            .unwrap();
        std::fs::write(&journal_path, revision_one).unwrap();

        let error = inspect_swarm_recovery_integrity(&artifact_dir)
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("integration_recovery_revision_replay"),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_journal_inspector_rejects_unanchored_tail_without_mutating_eventlog() {
        let (_env, root, artifact_dir, journal_path, mut journal) =
            authenticated_recovery_fixture("pending");
        let key = load_local_hmac_key_at(
            TEST_WORKFLOW_INTEGRITY_KEY
                .get()
                .expect("test integrity key must be installed"),
        )
        .unwrap();
        let previous = integration_recovery_journal_record_sha256(&journal).unwrap();
        journal.status = "restoring".to_string();
        journal.updated_at_ms = 2;
        sign_integration_recovery_journal(&mut journal, &key, 2, &previous).unwrap();
        std::fs::write(&journal_path, serde_json::to_vec_pretty(&journal).unwrap()).unwrap();
        let eventlog_before = std::fs::read(artifact_dir.join("eventlog.jsonl")).unwrap();

        let error = inspect_swarm_recovery_integrity(&artifact_dir)
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("integration_recovery_revision_unanchored"),
            "{error}"
        );
        assert_eq!(
            std::fs::read(artifact_dir.join("eventlog.jsonl")).unwrap(),
            eventlog_before
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_journal_inspector_rejects_missing_archived_file() {
        let (_env, root, artifact_dir, journal_path, _) =
            authenticated_recovery_fixture("completed");
        let integration_dir = journal_path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        archive_completed_integration_recovery_journal(&artifact_dir, &integration_dir).unwrap();
        let archive_path = std::fs::read_dir(integration_dir.join("recovery/history"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        std::fs::remove_file(&archive_path).unwrap();

        let error = inspect_swarm_recovery_integrity(&artifact_dir)
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("integration_recovery_archive_missing"),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn verified_checkpoint_restore_rejects_symlinked_parent_escape() {
        use std::os::unix::fs::symlink;

        let root = temp_root();
        let outside = temp_root();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(outside.join("api")).unwrap();
        let outside_file = outside.join("api/mod.rs");
        let outside_before = b"pub fn outside() {}\n";
        std::fs::write(&outside_file, outside_before).unwrap();
        symlink(&outside, root.join("src")).unwrap();
        let relative = PathBuf::from("src/api/mod.rs");
        let expected_current = integration_path_descriptor(&root.join(&relative)).unwrap();
        let checkpoint = VerifiedIntegrationCheckpoint {
            expected_fingerprint: "unused".to_string(),
            entries: vec![VerifiedCheckpointEntry {
                relative,
                expected_current,
                content: VerifiedCheckpointContent::File {
                    contents: b"pub fn restored() {}\n".to_vec(),
                    permissions: std::fs::metadata(&outside_file).unwrap().permissions(),
                    metadata: checkpoint_metadata_at_path(&outside_file).unwrap(),
                },
            }],
        };

        let error = restore_verified_integration_checkpoint(&root, &checkpoint)
            .unwrap_err()
            .to_string();

        assert!(error.contains("secure integration recovery"), "{error}");
        assert_eq!(std::fs::read(&outside_file).unwrap(), outside_before);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn secure_restore_preserves_target_changed_before_exchange() {
        let root = temp_root();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let relative = PathBuf::from("src/lib.rs");
        let target = root.join(&relative);
        std::fs::write(&target, b"pub fn original() {}\n").unwrap();
        let expected_current = integration_path_descriptor(&target).unwrap();

        let root_fd = linux_open_project_root(&root).unwrap();
        let (parent_fd, name) = linux_open_parent_beneath(root_fd.as_raw_fd(), &relative).unwrap();
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o7777;
        let temp =
            linux_create_temp_file(parent_fd.as_raw_fd(), b"pub fn restored() {}\n", mode).unwrap();

        let user_edit = b"pub fn user_edit() {}\n";
        std::fs::write(&target, user_edit).unwrap();
        let error = linux_install_temp_entry(
            parent_fd.as_raw_fd(),
            &temp,
            &name,
            &expected_current,
            &relative,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("changed during exchange"), "{error}");
        assert_eq!(std::fs::read(&target).unwrap(), user_edit);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn secure_restore_preserves_target_changed_before_removal() {
        let root = temp_root();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let relative = PathBuf::from("src/lib.rs");
        let target = root.join(&relative);
        std::fs::write(&target, b"pub fn original() {}\n").unwrap();
        let expected_current = integration_path_descriptor(&target).unwrap();

        let root_fd = linux_open_project_root(&root).unwrap();
        let (parent_fd, name) = linux_open_parent_beneath(root_fd.as_raw_fd(), &relative).unwrap();
        let user_edit = b"pub fn user_edit() {}\n";
        std::fs::write(&target, user_edit).unwrap();
        let error =
            linux_remove_expected_entry(parent_fd.as_raw_fd(), &name, &expected_current, &relative)
                .unwrap_err()
                .to_string();

        assert!(error.contains("changed during removal"), "{error}");
        assert_eq!(std::fs::read(&target).unwrap(), user_edit);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_journal_resumes_exchange_and_removal_after_process_crash() {
        let _env = TestWorkflowIntegrityEnv::with_key();
        let root = temp_root();
        let workflow = initialize_workflow_run(
            &root,
            WorkflowInit {
                request: "recovery journal crash test".to_string(),
                input_kind: WorkflowInputKind::Feature,
                profile: WorkflowProfile::Gated,
                approval_required: false,
            },
        )
        .unwrap();
        let artifact_dir = workflow.artifact_dir;
        let integration_dir = artifact_dir.join("integrations/dispatch");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(&integration_dir).unwrap();
        let restored_file = root.join("src/lib.rs");
        let removed_file = root.join("src/generated.rs");
        let restored_contents = b"pub fn original() {}\n";
        let integrated_contents = b"pub fn integrated() {}\n";
        let generated_contents = b"pub fn generated() {}\n";
        std::fs::write(&restored_file, restored_contents).unwrap();
        let restored_permissions = std::fs::metadata(&restored_file).unwrap().permissions();
        let restored_metadata = checkpoint_metadata_at_path(&restored_file).unwrap();
        let baseline_fingerprint = current_working_tree_fingerprint(&root).unwrap();
        std::fs::write(&restored_file, integrated_contents).unwrap();
        std::fs::write(&removed_file, generated_contents).unwrap();
        let checkpoint = VerifiedIntegrationCheckpoint {
            expected_fingerprint: current_working_tree_fingerprint(&root).unwrap(),
            entries: vec![
                VerifiedCheckpointEntry {
                    relative: PathBuf::from("src/lib.rs"),
                    expected_current: integration_path_descriptor(&restored_file).unwrap(),
                    content: VerifiedCheckpointContent::File {
                        contents: restored_contents.to_vec(),
                        permissions: restored_permissions.clone(),
                        metadata: restored_metadata.clone(),
                    },
                },
                VerifiedCheckpointEntry {
                    relative: PathBuf::from("src/generated.rs"),
                    expected_current: integration_path_descriptor(&removed_file).unwrap(),
                    content: VerifiedCheckpointContent::Missing,
                },
            ],
        };
        let plan = json!({
            "integration_id": "integration-test",
            "dispatch_id": "dispatch",
            "plan_sha256": "sha256:plan",
            "baseline": {"working_tree_fingerprint": baseline_fingerprint},
        });
        let state = json!({
            "checkpoint_path": "checkpoint/manifest.json",
            "checkpoint_sha256": "sha256:checkpoint",
        });
        let mut journal =
            create_integration_recovery_journal(&root, &plan, &state, &checkpoint).unwrap();
        let journal_path = integration_recovery_journal_path(&integration_dir);
        commit_integration_recovery_journal_revision(&artifact_dir, &journal_path, &mut journal)
            .unwrap();

        let root_fd = linux_open_project_root(&root).unwrap();
        let (lib_parent, lib_name) =
            linux_open_parent_beneath(root_fd.as_raw_fd(), Path::new("src/lib.rs")).unwrap();
        let lib_aux = CString::new(journal.entries[0].auxiliary_name.as_str()).unwrap();
        linux_create_named_file(
            lib_parent.as_raw_fd(),
            &lib_aux,
            restored_contents,
            restored_permissions.mode() & 0o7777,
            &restored_metadata,
        )
        .unwrap();
        linux_renameat2(
            lib_parent.as_raw_fd(),
            &lib_aux,
            lib_parent.as_raw_fd(),
            &lib_name,
            LINUX_RENAME_EXCHANGE,
        )
        .unwrap();
        linux_sync_directory(lib_parent.as_raw_fd()).unwrap();

        let (generated_parent, generated_name) =
            linux_open_parent_beneath(root_fd.as_raw_fd(), Path::new("src/generated.rs")).unwrap();
        let generated_aux = CString::new(journal.entries[1].auxiliary_name.as_str()).unwrap();
        linux_renameat2(
            generated_parent.as_raw_fd(),
            &generated_name,
            generated_parent.as_raw_fd(),
            &generated_aux,
            LINUX_RENAME_NOREPLACE,
        )
        .unwrap();
        linux_sync_directory(generated_parent.as_raw_fd()).unwrap();

        assert_eq!(std::fs::read(&restored_file).unwrap(), restored_contents);
        assert!(!removed_file.exists());
        assert_ne!(
            linux_descriptor_at(lib_parent.as_raw_fd(), &lib_aux).unwrap(),
            "missing"
        );
        assert_ne!(
            linux_descriptor_at(generated_parent.as_raw_fd(), &generated_aux).unwrap(),
            "missing"
        );

        restore_verified_integration_checkpoint_with_journal(
            &artifact_dir,
            &root,
            &integration_dir,
            &plan,
            &state,
            &checkpoint,
        )
        .unwrap();

        assert_eq!(std::fs::read(&restored_file).unwrap(), restored_contents);
        assert!(!removed_file.exists());
        assert_eq!(
            linux_descriptor_at(lib_parent.as_raw_fd(), &lib_aux).unwrap(),
            "missing"
        );
        assert_eq!(
            linux_descriptor_at(generated_parent.as_raw_fd(), &generated_aux).unwrap(),
            "missing"
        );
        let completed: IntegrationRecoveryJournal =
            serde_json::from_slice(&std::fs::read(&journal_path).unwrap()).unwrap();
        assert_eq!(completed.status, "completed");
        assert_eq!(completed.next_entry_index, 2);
        assert!(completed
            .entries
            .iter()
            .all(|entry| entry.status == IntegrationRecoveryEntryStatus::Completed));
        assert_eq!(
            current_working_tree_fingerprint(&root).unwrap(),
            plan["baseline"]["working_tree_fingerprint"]
        );
        archive_completed_integration_recovery_journal(&artifact_dir, &integration_dir).unwrap();
        assert!(!journal_path.exists());
        let archived = std::fs::read_dir(integration_dir.join("recovery/history"))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(archived.len(), 1);
        let archived_journal: IntegrationRecoveryJournal =
            serde_json::from_slice(&std::fs::read(archived[0].path()).unwrap()).unwrap();
        assert_eq!(archived_journal.status, "completed");
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_journal_blocks_unrelated_user_edit_before_mutation() {
        let _env = TestWorkflowIntegrityEnv::with_key();
        let root = temp_root();
        let workflow = initialize_workflow_run(
            &root,
            WorkflowInit {
                request: "recovery journal drift test".to_string(),
                input_kind: WorkflowInputKind::Feature,
                profile: WorkflowProfile::Gated,
                approval_required: false,
            },
        )
        .unwrap();
        let artifact_dir = workflow.artifact_dir;
        let integration_dir = artifact_dir.join("integrations/dispatch");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(&integration_dir).unwrap();
        let target = root.join("src/lib.rs");
        let unrelated = root.join("src/user.rs");
        let restored_contents = b"pub fn original() {}\n";
        std::fs::write(&target, restored_contents).unwrap();
        std::fs::write(&unrelated, b"pub fn before() {}\n").unwrap();
        let permissions = std::fs::metadata(&target).unwrap().permissions();
        let metadata = checkpoint_metadata_at_path(&target).unwrap();
        let baseline_fingerprint = current_working_tree_fingerprint(&root).unwrap();
        let integrated_contents = b"pub fn integrated() {}\n";
        std::fs::write(&target, integrated_contents).unwrap();
        let checkpoint = VerifiedIntegrationCheckpoint {
            expected_fingerprint: current_working_tree_fingerprint(&root).unwrap(),
            entries: vec![VerifiedCheckpointEntry {
                relative: PathBuf::from("src/lib.rs"),
                expected_current: integration_path_descriptor(&target).unwrap(),
                content: VerifiedCheckpointContent::File {
                    contents: restored_contents.to_vec(),
                    permissions,
                    metadata,
                },
            }],
        };
        let plan = json!({
            "integration_id": "integration-test",
            "dispatch_id": "dispatch",
            "plan_sha256": "sha256:plan",
            "baseline": {"working_tree_fingerprint": baseline_fingerprint},
        });
        let state = json!({
            "checkpoint_path": "checkpoint/manifest.json",
            "checkpoint_sha256": "sha256:checkpoint",
        });
        let mut journal =
            create_integration_recovery_journal(&root, &plan, &state, &checkpoint).unwrap();
        commit_integration_recovery_journal_revision(
            &artifact_dir,
            &integration_recovery_journal_path(&integration_dir),
            &mut journal,
        )
        .unwrap();
        std::fs::write(&unrelated, b"pub fn user_edit() {}\n").unwrap();

        let error = restore_verified_integration_checkpoint_with_journal(
            &artifact_dir,
            &root,
            &integration_dir,
            &plan,
            &state,
            &checkpoint,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("unrelated working tree drift"), "{error}");
        assert_eq!(std::fs::read(&target).unwrap(), integrated_contents);
        assert_eq!(
            std::fs::read(&unrelated).unwrap(),
            b"pub fn user_edit() {}\n"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_journal_restores_file_owner_and_xattrs() {
        let _env = TestWorkflowIntegrityEnv::with_key();
        fn set_xattr(path: &Path, name: &[u8], value: &[u8]) {
            let path = CString::new(path.as_os_str().as_bytes()).unwrap();
            let name = CString::new(name).unwrap();
            let result = unsafe {
                libc::setxattr(
                    path.as_ptr(),
                    name.as_ptr(),
                    value.as_ptr().cast(),
                    value.len(),
                    0,
                )
            };
            assert_eq!(result, 0, "{}", std::io::Error::last_os_error());
        }

        fn get_xattr(path: &Path, name: &[u8]) -> Vec<u8> {
            let path = CString::new(path.as_os_str().as_bytes()).unwrap();
            let name = CString::new(name).unwrap();
            let length =
                unsafe { libc::getxattr(path.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0) };
            assert!(length >= 0, "{}", std::io::Error::last_os_error());
            let mut value = vec![0u8; length as usize];
            let read = unsafe {
                libc::getxattr(
                    path.as_ptr(),
                    name.as_ptr(),
                    value.as_mut_ptr().cast(),
                    value.len(),
                )
            };
            assert_eq!(read, length, "{}", std::io::Error::last_os_error());
            value
        }

        use std::os::unix::fs::MetadataExt;

        let root = temp_root();
        let workflow = initialize_workflow_run(
            &root,
            WorkflowInit {
                request: "recovery metadata test".to_string(),
                input_kind: WorkflowInputKind::Feature,
                profile: WorkflowProfile::Gated,
                approval_required: false,
            },
        )
        .unwrap();
        let artifact_dir = workflow.artifact_dir;
        let integration_dir = artifact_dir.join("integrations/dispatch");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(&integration_dir).unwrap();
        let target = root.join("src/lib.rs");
        let restored_contents = b"pub fn original() {}\n";
        let integrated_contents = b"pub fn integrated() {}\n";
        let xattr_name = b"user.kiana.recovery";
        std::fs::write(&target, restored_contents).unwrap();
        set_xattr(&target, xattr_name, b"original-metadata");
        let restored_permissions = std::fs::metadata(&target).unwrap().permissions();
        let restored_metadata = checkpoint_metadata_at_path(&target).unwrap();
        let baseline_fingerprint = current_working_tree_fingerprint(&root).unwrap();

        std::fs::write(&target, integrated_contents).unwrap();
        set_xattr(&target, xattr_name, b"integrated-metadata");
        let checkpoint = VerifiedIntegrationCheckpoint {
            expected_fingerprint: current_working_tree_fingerprint(&root).unwrap(),
            entries: vec![VerifiedCheckpointEntry {
                relative: PathBuf::from("src/lib.rs"),
                expected_current: integration_path_descriptor(&target).unwrap(),
                content: VerifiedCheckpointContent::File {
                    contents: restored_contents.to_vec(),
                    permissions: restored_permissions,
                    metadata: restored_metadata.clone(),
                },
            }],
        };
        let plan = json!({
            "integration_id": "integration-metadata-test",
            "dispatch_id": "dispatch",
            "plan_sha256": "sha256:plan",
            "baseline": {"working_tree_fingerprint": baseline_fingerprint},
        });
        let state = json!({
            "checkpoint_path": "checkpoint/manifest.json",
            "checkpoint_sha256": "sha256:checkpoint",
        });

        restore_verified_integration_checkpoint_with_journal(
            &artifact_dir,
            &root,
            &integration_dir,
            &plan,
            &state,
            &checkpoint,
        )
        .unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), restored_contents);
        assert_eq!(get_xattr(&target, xattr_name), b"original-metadata");
        let actual_metadata = std::fs::metadata(&target).unwrap();
        assert_eq!(actual_metadata.uid(), restored_metadata.uid);
        assert_eq!(actual_metadata.gid(), restored_metadata.gid);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_journal_restores_symlink_target_owner_and_supported_xattrs() {
        let _env = TestWorkflowIntegrityEnv::with_key();
        fn try_set_symlink_xattr(path: &Path, name: &[u8], value: &[u8]) -> bool {
            let path = CString::new(path.as_os_str().as_bytes()).unwrap();
            let name = CString::new(name).unwrap();
            let result = unsafe {
                libc::lsetxattr(
                    path.as_ptr(),
                    name.as_ptr(),
                    value.as_ptr().cast(),
                    value.len(),
                    0,
                )
            };
            if result == 0 {
                return true;
            }
            let error = std::io::Error::last_os_error();
            if matches!(
                error.raw_os_error(),
                Some(code)
                    if code == libc::ENOTSUP
                        || code == libc::EPERM
                        || code == libc::EACCES
            ) {
                eprintln!(
                    "symlink user xattr verification skipped: filesystem rejected lsetxattr: {error}"
                );
                return false;
            }
            panic!("unexpected symlink lsetxattr failure: {error}");
        }

        fn get_symlink_xattr(path: &Path, name: &[u8]) -> Vec<u8> {
            let path = CString::new(path.as_os_str().as_bytes()).unwrap();
            let name = CString::new(name).unwrap();
            let length =
                unsafe { libc::lgetxattr(path.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0) };
            assert!(length >= 0, "{}", std::io::Error::last_os_error());
            let mut value = vec![0u8; length as usize];
            let read = unsafe {
                libc::lgetxattr(
                    path.as_ptr(),
                    name.as_ptr(),
                    value.as_mut_ptr().cast(),
                    value.len(),
                )
            };
            assert_eq!(read, length, "{}", std::io::Error::last_os_error());
            value
        }

        use std::os::unix::fs::{symlink, MetadataExt};

        let root = temp_root();
        let workflow = initialize_workflow_run(
            &root,
            WorkflowInit {
                request: "recovery symlink metadata test".to_string(),
                input_kind: WorkflowInputKind::Feature,
                profile: WorkflowProfile::Gated,
                approval_required: false,
            },
        )
        .unwrap();
        let artifact_dir = workflow.artifact_dir;
        let integration_dir = artifact_dir.join("integrations/dispatch");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(&integration_dir).unwrap();
        let target = root.join("src/current.rs");
        let restored_target = PathBuf::from("original.rs");
        let integrated_target = PathBuf::from("integrated.rs");
        let xattr_name = b"user.kiana.recovery";

        symlink(&restored_target, &target).unwrap();
        let symlink_xattrs_supported =
            try_set_symlink_xattr(&target, xattr_name, b"original-metadata");
        let restored_metadata = checkpoint_metadata_at_path(&target).unwrap();
        let baseline_fingerprint = current_working_tree_fingerprint(&root).unwrap();

        std::fs::remove_file(&target).unwrap();
        symlink(&integrated_target, &target).unwrap();
        if symlink_xattrs_supported {
            assert!(try_set_symlink_xattr(
                &target,
                xattr_name,
                b"integrated-metadata"
            ));
        }
        let checkpoint = VerifiedIntegrationCheckpoint {
            expected_fingerprint: current_working_tree_fingerprint(&root).unwrap(),
            entries: vec![VerifiedCheckpointEntry {
                relative: PathBuf::from("src/current.rs"),
                expected_current: integration_path_descriptor(&target).unwrap(),
                content: VerifiedCheckpointContent::Symlink {
                    target: restored_target.clone(),
                    metadata: restored_metadata.clone(),
                },
            }],
        };
        let plan = json!({
            "integration_id": "integration-symlink-metadata-test",
            "dispatch_id": "dispatch",
            "plan_sha256": "sha256:plan",
            "baseline": {"working_tree_fingerprint": baseline_fingerprint},
        });
        let state = json!({
            "checkpoint_path": "checkpoint/manifest.json",
            "checkpoint_sha256": "sha256:checkpoint",
        });

        restore_verified_integration_checkpoint_with_journal(
            &artifact_dir,
            &root,
            &integration_dir,
            &plan,
            &state,
            &checkpoint,
        )
        .unwrap();

        assert_eq!(std::fs::read_link(&target).unwrap(), restored_target);
        let actual_metadata = std::fs::symlink_metadata(&target).unwrap();
        assert_eq!(actual_metadata.uid(), restored_metadata.uid);
        assert_eq!(actual_metadata.gid(), restored_metadata.gid);
        if symlink_xattrs_supported {
            assert_eq!(get_symlink_xattr(&target, xattr_name), b"original-metadata");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    fn write_task(root: &std::path::Path, list_id: &str, id: &str, status: &str, title: &str) {
        let dir = root.join(".kiana").join("tasks").join(list_id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{id}.json")),
            serde_json::to_string_pretty(&json!({
                "id": id,
                "title": title,
                "subject": title,
                "description": null,
                "status": status,
                "owner": null,
                "blocks": [],
                "blockedBy": [],
                "created_at": 1,
                "updated_at": 2
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn task_evidence_reference_update_is_exactly_deduplicated() {
        let root = temp_root();
        write_task(&root, "default", "task-1", "completed", "Task one");
        let app_state =
            HashMap::from([("cwd".to_string(), json!(root.to_string_lossy().to_string()))]);
        let context = context("", app_state);
        let reference = "verification:.kiana/workflows/run-1/verification/verify-1.json#verify-1";

        let lease = acquire_task_evidence_lease(&context, Some("default"), "task-1").unwrap();
        append_task_evidence_reference(&context, Some("default"), "task-1", reference, &lease)
            .unwrap();
        append_task_evidence_reference(&context, Some("default"), "task-1", reference, &lease)
            .unwrap();

        let task: Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(".kiana/tasks/default/task-1.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(task["metadata"]["evidence"], json!([reference]));
        assert!(!root.join(".kiana/tasks/default/task-1.json.tmp").exists());
        drop(lease);
        acquire_task_evidence_lease(&context, Some("default"), "task-1").unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn task_evidence_lease_rejects_concurrent_writer() {
        let root = temp_root();
        write_task(&root, "default", "task-1", "completed", "Task one");
        let app_state =
            HashMap::from([("cwd".to_string(), json!(root.to_string_lossy().to_string()))]);
        let context = context("", app_state);

        let lease = acquire_task_evidence_lease(&context, Some("default"), "task-1").unwrap();
        let error = acquire_task_evidence_lease(&context, Some("default"), "task-1")
            .unwrap_err()
            .to_string();
        assert!(error.contains("task_evidence_busy"));
        drop(lease);
        acquire_task_evidence_lease(&context, Some("default"), "task-1").unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn tasks_lists_session_state_tasks() {
        let mut app_state = HashMap::new();
        app_state.insert(
            "tasks".to_string(),
            json!([{ "id": "t1", "title": "Inspect", "status": "pending" }]),
        );

        let result = TasksCommand.execute(context("", app_state)).await.unwrap();

        assert!(result.value.contains("1 task(s) in 'default'"));
        assert!(result.value.contains("t1 [pending]"));
        assert!(result.value.contains("Inspect"));
    }

    #[tokio::test]
    async fn tasks_read_persisted_session_task_list() {
        let root = temp_root();
        write_task(&root, "session-1", "1", "pending", "Persisted task");
        let app_state = HashMap::from([
            ("cwd".to_string(), json!(root.to_string_lossy().to_string())),
            ("session_id".to_string(), json!("session-1")),
        ]);

        let result = TasksCommand
            .execute(context("list", app_state.clone()))
            .await
            .unwrap();
        assert!(result.value.contains("1 task(s) in 'session-1'"));
        assert!(result.value.contains("Persisted task"));

        let shown = TasksCommand
            .execute(context("show 1", app_state))
            .await
            .unwrap();
        assert!(shown.value.contains("\"id\": \"1\""));
        assert!(shown.value.contains("\"subject\": \"Persisted task\""));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn tasks_can_filter_and_select_explicit_task_list() {
        let root = temp_root();
        write_task(&root, "review", "1", "completed", "Done task");
        write_task(&root, "review", "2", "pending", "Open task");
        let app_state =
            HashMap::from([("cwd".to_string(), json!(root.to_string_lossy().to_string()))]);

        let result = TasksCommand
            .execute(context("pending review", app_state.clone()))
            .await
            .unwrap();
        assert!(result
            .value
            .contains("1 task(s) in 'review' with status 'pending'"));
        assert!(result.value.contains("Open task"));
        assert!(!result.value.contains("Done task"));

        let json = TasksCommand
            .execute(context("json review", app_state))
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&json.value).unwrap();
        assert_eq!(parsed["schema"], "kiana.tasks.v1");
        assert_eq!(parsed["task_list_id"], "review");
        assert_eq!(parsed["count"], 2);
        assert_eq!(parsed["status_counts"]["completed"], 1);
        assert_eq!(parsed["status_counts"]["pending"], 1);
        assert!(json.value.contains("\"id\": \"1\""));
        assert!(json.value.contains("\"id\": \"2\""));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn tasks_json_reports_schema_status_counts_and_task_order() {
        let root = temp_root();
        write_task(&root, "session-2", "2", "completed", "Second task");
        write_task(&root, "session-2", "1", "pending", "First task");
        let app_state = HashMap::from([
            ("cwd".to_string(), json!(root.to_string_lossy().to_string())),
            ("session_id".to_string(), json!("session-2")),
        ]);

        let result = TasksCommand
            .execute(context("json", app_state))
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(report["schema"], "kiana.tasks.v1");
        assert_eq!(report["task_list_id"], "session-2");
        assert_eq!(report["count"], 2);
        assert_eq!(report["status_counts"]["pending"], 1);
        assert_eq!(report["status_counts"]["completed"], 1);
        assert_eq!(report["tasks"][0]["id"], "1");
        assert_eq!(report["tasks"][1]["id"], "2");
        assert!(report["tasks_dir"].as_str().unwrap().contains("session-2"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn team_plan_reports_missing_artifacts_and_role_assignments() {
        let app_state = HashMap::from([(
            "tasks".to_string(),
            json!([
                {
                    "id": "1",
                    "title": "Write PRD",
                    "status": "pending",
                    "owner": "pm",
                    "metadata": { "artifact_role": "prd" }
                },
                {
                    "id": "2",
                    "title": "Implement core",
                    "status": "in_progress",
                    "owner": "engineer",
                    "metadata": { "artifact_role": "source" },
                    "blockedBy": ["1"]
                }
            ]),
        )]);

        let result = TasksCommand
            .execute(context("plan --json", app_state))
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(report["schema"], "kiana.team-plan.v1");
        assert_eq!(report["role_runtime"]["status"], "incomplete");
        assert_eq!(report["role_runtime"]["blocked_task_count"], 1);
        assert!(report["role_runtime"]["missing_roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|role| role == "architect"));
        assert!(report["role_runtime"]["missing_roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|role| role == "qa"));
        assert!(report["artifact_readiness"]["missing_roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|role| role == "design"));
        assert!(report["roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|role| role["id"] == "engineer" && role["task_ids"][0] == "2"));
    }

    #[tokio::test]
    async fn team_plan_ready_when_roles_and_artifacts_are_covered() {
        let app_state = HashMap::from([(
            "tasks".to_string(),
            json!([
                { "id": "1", "title": "PRD", "status": "completed", "owner": "product-manager", "metadata": { "artifact_role": "prd" } },
                { "id": "2", "title": "Design", "status": "completed", "owner": "architect", "metadata": { "artifact_role": "design" } },
                { "id": "3", "title": "Plan", "status": "pending", "owner": "qa", "metadata": { "artifact_role": "tasks" } },
                { "id": "4", "title": "Code", "status": "pending", "owner": "engineer", "metadata": { "artifact_role": "source" } },
                { "id": "5", "title": "Analysis tests", "status": "pending", "owner": "data analyst", "metadata": { "artifact_role": "test" } }
            ]),
        )]);

        let result = TasksCommand
            .execute(context("plan --json", app_state))
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(report["role_runtime"]["status"], "ready");
        assert_eq!(report["role_runtime"]["ready_for_fake_runtime"], true);
        assert_eq!(report["artifact_readiness"]["status"], "ready");
        assert!(report["role_runtime"]["missing_roles"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(report["artifact_readiness"]["missing_roles"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn tasks_reject_extra_words_after_task_list_id() {
        let root = temp_root();
        write_task(&root, "review", "1", "pending", "Open task");
        let app_state =
            HashMap::from([("cwd".to_string(), json!(root.to_string_lossy().to_string()))]);

        for args in [
            "list review extra",
            "json review extra",
            "path review extra",
            "pending review extra",
            "show 1 review extra",
        ] {
            let error = TasksCommand
                .execute(context(args, app_state.clone()))
                .await
                .unwrap_err()
                .to_string();

            assert!(
                error.contains("Usage:"),
                "{args} returned unexpected error: {error}"
            );
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn workflow_template_reports_default_dag_as_json() {
        let result = TasksCommand
            .execute(context("workflow template --json", HashMap::new()))
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(report["schema"], "kiana.workflow-dag.v1");
        assert_eq!(report["id"], "kiana-full-workflow");
        assert!(report["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["id"] == "execute"));
        assert!(report["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["from"] == "ship" && edge["to"] == "learn"));
    }

    #[tokio::test]
    async fn workflow_init_creates_run_artifacts_from_tasks_command() {
        let _env = TestWorkflowIntegrityEnv::without_key();
        let root = temp_root();
        let app_state =
            HashMap::from([("cwd".to_string(), json!(root.to_string_lossy().to_string()))]);

        let result = TasksCommand
            .execute(context(
                "workflow init --json --type feature --profile gated Build the workflow runtime",
                app_state,
            ))
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(report["schema"], "kiana.workflow-run.v1");
        assert_eq!(report["input_kind"], "feature");
        assert_eq!(report["profile"], "gated");
        assert_eq!(report["current_node"], "capture");

        let artifact_dir = PathBuf::from(report["artifact_dir"].as_str().unwrap());
        assert!(artifact_dir.join("workflow_dag.json").is_file());
        assert!(artifact_dir.join("eventlog.jsonl").is_file());
        assert!(artifact_dir.join("state.json").is_file());

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn workflow_list_and_continue_report_latest_run_as_json() {
        let _env = TestWorkflowIntegrityEnv::without_key();
        let root = temp_root();
        let app_state =
            HashMap::from([("cwd".to_string(), json!(root.to_string_lossy().to_string()))]);

        TasksCommand
            .execute(context(
                "workflow init --json First workflow",
                app_state.clone(),
            ))
            .await
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = TasksCommand
            .execute(context(
                "workflow init --json Second workflow",
                app_state.clone(),
            ))
            .await
            .unwrap();
        let second: Value = serde_json::from_str(&second.value).unwrap();

        let list = TasksCommand
            .execute(context("workflow list --json", app_state.clone()))
            .await
            .unwrap();
        let list: Value = serde_json::from_str(&list.value).unwrap();
        assert_eq!(list["schema"], "kiana.workflow-list.v1");
        assert_eq!(list["runs"].as_array().unwrap().len(), 2);
        assert_eq!(list["runs"][0]["run_id"], second["run_id"]);

        let resumed = TasksCommand
            .execute(context("workflow continue --json", app_state))
            .await
            .unwrap();
        let resumed: Value = serde_json::from_str(&resumed.value).unwrap();
        assert_eq!(resumed["schema"], "kiana.workflow-resume.v1");
        assert_eq!(resumed["resume_status"], "ready");
        assert_eq!(resumed["run"]["run_id"], second["run_id"]);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn workflow_continue_reports_blocked_state_eventlog_conflict() {
        let _env = TestWorkflowIntegrityEnv::without_key();
        let root = temp_root();
        let app_state =
            HashMap::from([("cwd".to_string(), json!(root.to_string_lossy().to_string()))]);
        let created = TasksCommand
            .execute(context(
                "workflow init --json Conflicted workflow",
                app_state.clone(),
            ))
            .await
            .unwrap();
        let created: Value = serde_json::from_str(&created.value).unwrap();
        let state_path =
            PathBuf::from(created["artifact_dir"].as_str().unwrap()).join("state.json");
        let mut state: Value =
            serde_json::from_str(&std::fs::read_to_string(&state_path).unwrap()).unwrap();
        state["last_event_seq"] = json!(77);
        std::fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

        let resumed = TasksCommand
            .execute(context(
                &format!(
                    "workflow continue --json {}",
                    created["run_id"].as_str().unwrap()
                ),
                app_state,
            ))
            .await
            .unwrap();
        let resumed: Value = serde_json::from_str(&resumed.value).unwrap();
        assert_eq!(resumed["resume_status"], "blocked");
        assert_eq!(resumed["eventlog_consistent"], false);
        assert!(resumed["blocker"]
            .as_str()
            .unwrap()
            .contains("state last_event_seq 77 does not match eventlog 2"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn workflow_continue_reports_repaired_lagging_state() {
        let _env = TestWorkflowIntegrityEnv::without_key();
        let root = temp_root();
        let app_state =
            HashMap::from([("cwd".to_string(), json!(root.to_string_lossy().to_string()))]);
        let created = TasksCommand
            .execute(context(
                "workflow init --json Repair lagging workflow state",
                app_state.clone(),
            ))
            .await
            .unwrap();
        let created: Value = serde_json::from_str(&created.value).unwrap();
        let artifact_dir = PathBuf::from(created["artifact_dir"].as_str().unwrap());
        let state_path = artifact_dir.join("state.json");
        let stale_state = std::fs::read(&state_path).unwrap();
        append_workflow_event(
            &artifact_dir,
            WorkflowEventKind::NodeEntered,
            "product_definition",
            json!({"reason": "simulate EventLog commit before state projection"}),
        )
        .unwrap();
        std::fs::write(&state_path, stale_state).unwrap();

        let resumed = TasksCommand
            .execute(context(
                &format!(
                    "workflow continue --json {}",
                    created["run_id"].as_str().unwrap()
                ),
                app_state,
            ))
            .await
            .unwrap();
        let resumed: Value = serde_json::from_str(&resumed.value).unwrap();

        assert_eq!(resumed["resume_status"], "repaired");
        assert_eq!(resumed["eventlog_consistent"], true);
        assert_eq!(resumed["state_last_event_seq"], 3);
        assert_eq!(resumed["run"]["current_node"], "product_definition");
        assert_eq!(resumed["recommended_action"], "continue:product_definition");

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn workflow_continue_blocks_tampered_lagging_state() {
        let _env = TestWorkflowIntegrityEnv::without_key();
        let root = temp_root();
        let app_state =
            HashMap::from([("cwd".to_string(), json!(root.to_string_lossy().to_string()))]);
        let created = TasksCommand
            .execute(context(
                "workflow init --json Reject tampered lagging state",
                app_state.clone(),
            ))
            .await
            .unwrap();
        let created: Value = serde_json::from_str(&created.value).unwrap();
        let artifact_dir = PathBuf::from(created["artifact_dir"].as_str().unwrap());
        let state_path = artifact_dir.join("state.json");
        let mut stale_state: Value =
            serde_json::from_str(&std::fs::read_to_string(&state_path).unwrap()).unwrap();
        append_workflow_event(
            &artifact_dir,
            WorkflowEventKind::NodeEntered,
            "product_definition",
            json!({"reason": "create a valid EventLog tail"}),
        )
        .unwrap();
        stale_state["current_node"] = json!("ship");
        std::fs::write(
            &state_path,
            serde_json::to_vec_pretty(&stale_state).unwrap(),
        )
        .unwrap();

        let resumed = TasksCommand
            .execute(context(
                &format!(
                    "workflow continue --json {}",
                    created["run_id"].as_str().unwrap()
                ),
                app_state,
            ))
            .await
            .unwrap();
        let resumed: Value = serde_json::from_str(&resumed.value).unwrap();

        assert_eq!(resumed["resume_status"], "blocked");
        assert_eq!(resumed["eventlog_consistent"], false);
        assert!(resumed["blocker"]
            .as_str()
            .unwrap()
            .contains("current_node"));

        let _ = std::fs::remove_dir_all(root);
    }
}

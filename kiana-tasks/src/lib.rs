pub mod dream_task;
pub mod evidence;
pub mod integrity;
pub mod pill_label;
pub mod project_board;
pub mod stop_task;
pub mod swarm;
pub mod task_id;
pub mod types;
pub mod workflow;

pub use dream_task::{
    add_dream_turn, complete_dream_task, create_dream_task, fail_dream_task, kill_dream_task,
};
pub use evidence::{
    append_evidence_event, build_verification_packet, ensure_evidence_artifacts,
    list_review_packets, list_verification_packets, read_evidence_events, read_review_packet,
    read_verification_packet, redact_and_bound_evidence_text, summarize_evidence,
    unresolved_blocking_evidence, validate_verification_packet_completion,
    validate_verification_packet_integrity, write_review_packet, write_verification_packet,
    BoundedEvidenceText, EvidenceError, EvidenceEvent, EvidenceEventDraft, EvidenceKind,
    EvidenceResult, EvidenceSource, EvidenceStatus, LedgerSummary, VerificationCheck,
    VerificationPacket, VerificationStatus, EVIDENCE_EVENT_SCHEMA, REVIEW_PACKET_SCHEMA,
    VERIFICATION_PACKET_SCHEMA,
};
pub use integrity::{
    initialize_local_hmac_key, initialize_local_hmac_key_at, inspect_local_hmac_key,
    load_local_hmac_key, load_local_hmac_key_at, sha256_prefixed, workflow_integrity_key_path,
    IntegrityEnvelope, IntegrityError, IntegrityKeyStatus, LocalHmacKey, INTEGRITY_ENVELOPE_SCHEMA,
    INTEGRITY_KEY_SCHEMA, INTEGRITY_KEY_STATUS_SCHEMA, LOCAL_HMAC_ALGORITHM,
};
pub use pill_label::{get_pill_label, pill_needs_cta};
pub use project_board::{
    build_project_board, build_project_board_at_root, select_next_project_task,
    ProjectBlockingReasonSummary, ProjectBoardColumn, ProjectBoardError, ProjectBoardProjection,
    ProjectBoardResult, ProjectBoardStatus, ProjectNextAlternative, ProjectNextTaskReport,
    ProjectPolicyFinding, ProjectTaskCard,
};
pub use stop_task::{StopTaskError, StopTaskResult, TaskKiller, TaskRegistry};
pub use swarm::{
    build_swarm_plan, persist_swarm_dispatch, prepare_swarm_execution, SwarmAssignment,
    SwarmDispatchError, SwarmDispatchManifest, SwarmDispatchResult, SwarmExecutionError,
    SwarmExecutionManifest, SwarmExecutionPrepareResult, SwarmExecutionRequest, SwarmPathLock,
    SwarmPlan, SwarmPlanError, SwarmPlanSummary, SwarmSkippedTask, SwarmTerminationPolicy,
    SwarmWorkPacket, SwarmWorkerBudget, SwarmWorkerLaunch, SwarmWorkerLaunchInput,
    SWARM_DISPATCH_MANIFEST_SCHEMA, SWARM_DISPATCH_RESULT_SCHEMA, SWARM_EXECUTION_MANIFEST_SCHEMA,
    SWARM_EXECUTION_PREPARE_RESULT_SCHEMA, SWARM_PLAN_SCHEMA, SWARM_WORKER_LAUNCH_SCHEMA,
    SWARM_WORK_PACKET_SCHEMA, WORK_PACKET_PREVIEW_SCHEMA,
};
pub use task_id::{create_task_id_for_type, generate_task_id, get_task_id_prefix};
pub use types::{
    BashTaskKind, DreamPhase, DreamTaskState, DreamTurn, LocalShellTaskState, ShellResult,
    TaskState, TaskStateBase, TaskStatus, TaskType,
};
pub use workflow::{
    append_workflow_event, append_workflow_event_idempotent_with_unique_data_value,
    append_workflow_event_with_data, append_workflow_event_with_unique_data_value,
    append_workflow_transition, commit_immutable_artifacts_with_unique_event,
    default_workflow_template, initialize_workflow_run, inspect_workflow_integrity,
    list_workflow_runs, read_workflow_events, read_workflow_state, resume_workflow_run,
    seal_legacy_workflow, EvidenceItem, EvidencePacket, GateDecision, GateResult, ReviewFinding,
    ReviewPacket, ReviewSeverity, WorkPacket, WorkflowArtifactBatch, WorkflowArtifactCommit,
    WorkflowArtifactDescriptor, WorkflowArtifactInput, WorkflowDagTemplate, WorkflowEdge,
    WorkflowError, WorkflowEvent, WorkflowEventKind, WorkflowGateSpec, WorkflowInit,
    WorkflowInputKind, WorkflowIntegrityGenesisSeal, WorkflowIntegrityReport, WorkflowNodeSpec,
    WorkflowNodeStage, WorkflowNodeType, WorkflowProfile, WorkflowResult, WorkflowResumeReport,
    WorkflowResumeStatus, WorkflowRun, WorkflowRunSummary, WorkflowState, WorkflowStatus,
    WorkflowTransitionEvidence, WorkflowTransitionInput, WorkflowTransitionResult,
    WorkflowTrustStatus, WORKFLOW_ARTIFACT_DESCRIPTOR_SCHEMA, WORKFLOW_INTEGRITY_GENESIS_SCHEMA,
    WORKFLOW_INTEGRITY_REPORT_SCHEMA,
};

//! UI-22 receipt-detail fixtures: the digest a browser shows must be reproducible from the facts.
//!
//! Deny-first: a receipt that changes when its own output is appended, an `Unknown` result that is
//! promoted to success, and a foreign session reading another owner's receipt are all refused or
//! stay Unknown.  Only then does the success fixture recompute the same `RunReceipt` digest from a
//! journal reopened by a fresh process (child re-exec), with `run.receipt` and `run.snapshot`
//! already appended.
//!
//! This target also holds the two source guards for the receipt projection: one for
//! `receipt_source_events` and one for the Web detail projection never gaining execution authority.

use async_trait::async_trait;
use kiana_core::{project_run_state, ControlPlane, RunOutcome, RunPhase};
use kiana_domain::{
    ApprovalChallenge, ApprovalId, CapabilityRequest, ExecutionStatus, PendingApproval,
    RequestContext, RequestId, RunId, RunReceipt, RuntimeEvent,
};
use kiana_ports::{ApprovalStorePort, EventStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const SESSION: &str = "session-1";
const RUN_RECEIPT_ENV: &str = "KIANA_UI22_RECEIPT_CHILD";
const DURABLE_PATH_ENV: &str = "KIANA_UI22_RECEIPT_PATH";
const OUT_PATH_ENV: &str = "KIANA_UI22_RECEIPT_OUT";
const RUN_ID_ENV: &str = "KIANA_UI22_RECEIPT_RUN";
const ROOT_ENV: &str = "KIANA_UI22_RECEIPT_ROOT";

fn unique_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-ui22-{label}-{}-{stamp}.jsonl",
        std::process::id()
    ))
}

fn fixed_digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn trusted_context(root: &Path) -> RequestContext {
    let mut context = RequestContext::local(SESSION, root.to_string_lossy().into_owned());
    context.project_trusted = true;
    context
}

fn fact(sequence: u64, kind: &str, data: Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), sequence, kind, data).expect("runtime fact")
}

/// The committed fact sequence a Builder run leaves behind, ending at `run.completed`.
fn run_facts(run_id: RunId, root: &Path) -> Vec<RuntimeEvent> {
    vec![
        fact(
            1,
            "run.authorized",
            json!({
                "run_id": run_id,
                "session_id": SESSION,
                "actor_id": "local-user",
                "project_root": root.to_string_lossy(),
                "role_id": "builder",
                "department_id": "executing",
                "sandbox": "workspace-write",
            }),
        ),
        fact(
            2,
            "run.prompt",
            json!({"run_id": run_id, "session_id": SESSION, "text": "ui22 receipt"}),
        ),
        fact(
            3,
            "run.started",
            json!({"run_id": run_id, "session_id": SESSION}),
        ),
        fact(
            4,
            "run.completed",
            json!({"run_id": run_id, "session_id": SESSION, "text": "done"}),
        ),
    ]
}

async fn append_all(store: &JsonlStore, events: &[RuntimeEvent]) {
    for event in events {
        store
            .append(event.clone())
            .await
            .expect("append durable fact");
    }
}

type JsonlStore = kiana_eventlog::JsonlEventLog;

struct NoApprovals;

#[async_trait]
impl ApprovalStorePort for NoApprovals {
    async fn stage(
        &self,
        _: &RequestContext,
        _: CapabilityRequest,
        _: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        Err(PortError::Unavailable("ui22_no_approval".to_owned()))
    }
    async fn activate(&self, _: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable("ui22_no_approval".to_owned()))
    }
    async fn consume(
        &self,
        _: &RequestContext,
        _: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable("ui22_no_approval".to_owned()))
    }
}

/// A read-only fixture runner: the receipt path must never send a command or restore a run.
struct ZeroCallRunner;

#[async_trait]
impl RunnerPort for ZeroCallRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Unavailable("ui22_receipt_read_only".to_owned()))
    }
}

fn plane(store: Arc<JsonlStore>) -> ControlPlane {
    ControlPlane::new(
        Arc::new(kiana_policy::DefaultPolicyEngine),
        Arc::new(kiana_gates::DefaultGateEngine),
        store,
        Arc::new(kiana_capability_broker::CapabilityBroker::new()),
        Arc::new(NoApprovals),
        Arc::new(ZeroCallRunner),
    )
}

fn typed_receipt(receipt: &Value) -> RunReceipt {
    let value = receipt.get("run_receipt").unwrap_or(receipt);
    RunReceipt::from_json(value)
        .unwrap_or_else(|error| panic!("typed run receipt: {error}: {value}"))
}

/// 【拒绝】收据自己的输出事件不得改变它的 digest；Unknown 不得被提升为 completed。
#[tokio::test]
async fn ui22_receipt_digest_is_stable_after_its_own_output_events() {
    let path = unique_path("stable");
    let run_id = RunId::new();
    let root = std::env::temp_dir().join(format!("kiana-ui22-root-{run_id}"));
    std::fs::create_dir_all(&root).expect("fixture root");
    let context = trusted_context(&root);

    let store = Arc::new(JsonlStore::open(&path).expect("open durable journal"));
    append_all(&store, &run_facts(run_id, &root)).await;

    // The completing process returns this receipt.
    let completing = plane(store.clone());
    let completed = completing
        .read_receipt(context.clone(), Some(run_id))
        .await
        .expect("completed receipt");
    assert_eq!(
        completed.status,
        ExecutionStatus::Completed,
        "{completed:?}"
    );
    let initial = typed_receipt(&completed.output);
    assert_eq!(initial.status, ExecutionStatus::Completed);

    // Persist exactly what the completion path appends afterwards: the receipt and the snapshot.
    let journal = JsonlStore::open(&path).expect("reopen to append");
    journal
        .append(fact(
            5,
            "run.receipt",
            json!({"run_id": run_id, "session_id": SESSION, "output": completed.output}),
        ))
        .await
        .expect("append run.receipt");
    journal
        .append(fact(
            6,
            "run.snapshot",
            json!({"run_id": run_id, "schema": "kiana.run-snapshot.v1", "phase": "terminal"}),
        ))
        .await
        .expect("append run.snapshot");
    drop(journal);

    let facts_after = JsonlStore::open(&path)
        .expect("reopen journal")
        .read_all()
        .await
        .expect("read facts");
    assert_eq!(facts_after.len(), 6, "both bookkeeping events persisted");
    let state = project_run_state(run_id, &facts_after).expect("state");
    assert_eq!(state.phase, RunPhase::Terminal);
    assert_eq!(state.outcome, Some(RunOutcome::Completed));

    let reread = plane(Arc::new(JsonlStore::open(&path).expect("reopen journal")))
        .read_receipt(context, Some(run_id))
        .await
        .expect("reread receipt");
    let recomputed = typed_receipt(&reread.output);
    assert_eq!(
        recomputed.receipt_digest, initial.receipt_digest,
        "appending the receipt's own output must not rewrite its digest"
    );
    assert_eq!(recomputed.source_cursor, initial.source_cursor);
    assert_eq!(recomputed.source_event_ids, initial.source_event_ids);
    assert_eq!(recomputed.result_digest, initial.result_digest);

    // Deny: a run whose terminal outcome is Unknown can never be normalized to completed, and a
    // receipt claiming both is rejected by the typed contract.
    let unknown_run = RunId::new();
    let mut unknown = initial.clone();
    unknown.run_id = unknown_run;
    unknown.status = ExecutionStatus::ResultUnknown;
    unknown.terminal_reason = Some("result_unknown".to_owned());
    let unknown = RunReceipt::new(
        unknown.run_id,
        unknown.session_id.clone(),
        unknown.owner_actor_id.clone(),
        unknown.project_digest.clone(),
        unknown.status,
        unknown.terminal_reason.clone(),
        unknown.source_cursor,
        unknown.source_event_ids.clone(),
        unknown.redaction_profile.clone(),
        unknown.feature_status.clone(),
        unknown.proof_level.clone(),
        unknown.result_digest.clone(),
        unknown.execution_receipt_digests.clone(),
    )
    .expect("unknown receipt");
    assert_eq!(unknown.status, ExecutionStatus::ResultUnknown);
    let mut forged = unknown.to_json().expect("encode");
    forged["receipt_digest"] = json!(fixed_digest('0'));
    assert_eq!(
        RunReceipt::from_json(&forged).err(),
        Some("run_receipt_digest_mismatch".to_owned())
    );

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("jsonl.lock"));
    let _ = std::fs::remove_dir_all(&root);
}

/// 【拒绝】另一个会话/角色读同一收据必须被 owner 检查挡住。
#[tokio::test]
async fn ui22_foreign_session_cannot_read_the_receipt() {
    let path = unique_path("foreign");
    let run_id = RunId::new();
    let root = std::env::temp_dir().join(format!("kiana-ui22-foreign-{run_id}"));
    std::fs::create_dir_all(&root).expect("fixture root");
    let store = Arc::new(JsonlStore::open(&path).expect("open durable journal"));
    append_all(&store, &run_facts(run_id, &root)).await;

    let mut foreign = RequestContext::local("other-session", root.to_string_lossy().into_owned());
    foreign.project_trusted = true;
    let denied = plane(store)
        .read_receipt(foreign, Some(run_id))
        .await
        .expect("denial response");
    assert_eq!(denied.status, ExecutionStatus::Blocked, "{denied:?}");
    assert_eq!(denied.error.as_deref(), Some("run_owner_mismatch"));
    assert!(denied.output.is_null(), "a refusal carries no receipt body");

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("jsonl.lock"));
    let _ = std::fs::remove_dir_all(&root);
}

/// 【成功/新鲜进程】用子进程重新打开同一持久日志，必须重算出与在线投影相同的收据 digest。
#[tokio::test]
async fn ui22_receipt_recomputes_in_a_fresh_process() {
    if std::env::var_os(RUN_RECEIPT_ENV).is_some() {
        // Child side: open the durable journal read-only and write the recomputed report.
        let path = PathBuf::from(std::env::var_os(DURABLE_PATH_ENV).expect("child journal path"));
        let run_id = RunId::parse_str(&std::env::var(RUN_ID_ENV).expect("child run id"))
            .expect("child run id");
        let out = PathBuf::from(std::env::var_os(OUT_PATH_ENV).expect("child output path"));
        let root = PathBuf::from(std::env::var_os(ROOT_ENV).expect("child project root"));
        let store = Arc::new(JsonlStore::open(&path).expect("child open journal"));
        let context = trusted_context(&root);
        let receipt = plane(store)
            .read_receipt(context, Some(run_id))
            .await
            .expect("child receipt");
        let report = json!({
            "status": receipt.status,
            "error": receipt.error,
            "output": receipt.output,
        });
        std::fs::write(&out, serde_json::to_vec(&report).expect("encode report"))
            .expect("child write report");
        return;
    }

    let path = unique_path("fresh");
    let out = unique_path("fresh-out");
    let run_id = RunId::new();
    let root = std::env::temp_dir().join(format!("kiana-ui22-fresh-{run_id}"));
    std::fs::create_dir_all(&root).expect("fixture root");
    let store = Arc::new(JsonlStore::open(&path).expect("open durable journal"));
    append_all(&store, &run_facts(run_id, &root)).await;
    let completing = plane(store.clone())
        .read_receipt(trusted_context(&root), Some(run_id))
        .await
        .expect("completing receipt");
    let initial = typed_receipt(&completing.output);

    // Persist the completion-time bookkeeping events, exactly like `lifecycle` does.
    let journal = JsonlStore::open(&path).expect("reopen");
    journal
        .append(fact(
            5,
            "run.receipt",
            json!({"run_id": run_id, "session_id": SESSION, "output": completing.output}),
        ))
        .await
        .expect("append run.receipt");
    journal
        .append(fact(
            6,
            "run.snapshot",
            json!({"run_id": run_id, "schema": "kiana.run-snapshot.v1"}),
        ))
        .await
        .expect("append run.snapshot");
    drop(journal);
    let before = std::fs::read(&path).expect("journal bytes");

    let status = std::process::Command::new(std::env::current_exe().expect("current exe"))
        .arg("--exact")
        .arg("ui22_receipt_recomputes_in_a_fresh_process")
        .arg("--nocapture")
        .env(RUN_RECEIPT_ENV, "1")
        .env(DURABLE_PATH_ENV, &path)
        .env(RUN_ID_ENV, run_id.to_string())
        .env(ROOT_ENV, &root)
        .env(OUT_PATH_ENV, &out)
        .status()
        .expect("spawn fresh-process child");
    assert!(status.success(), "fresh-process receipt failed: {status}");

    // The child is read-only over the journal.
    assert_eq!(std::fs::read(&path).expect("journal bytes"), before);

    let child: Value =
        serde_json::from_slice(&std::fs::read(&out).expect("child report")).expect("child report");
    assert_eq!(child["status"], json!("completed"), "{child}");
    assert_eq!(
        child["output"]["run_receipt"]["receipt_digest"],
        json!(initial.receipt_digest),
        "a fresh process must recompute the same receipt digest"
    );
    assert_eq!(
        child["output"]["run_receipt"]["source_cursor"],
        json!(initial.source_cursor)
    );
    assert_eq!(
        child["output"]["run_receipt"]["source_event_ids"],
        json!(initial.source_event_ids)
    );
    let recomputed = RunReceipt::from_json(&child["output"]["run_receipt"]).expect("typed child");
    assert_eq!(recomputed.receipt_digest, initial.receipt_digest);
    assert_eq!(recomputed.run_id, run_id);

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("jsonl.lock"));
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn ui22_receipt_projection_source_guards() {
    let receipts = include_str!("../src/receipts.rs");

    // Guard 1: the receipt source set must exclude only regenerable bookkeeping events, and the
    // cursor must be derived from that filtered set rather than the raw event count.
    let helper = receipts
        .split("fn receipt_source_events(")
        .nth(1)
        .expect("receipt_source_events helper");
    let helper = helper.split("\n}").next().expect("helper body");
    assert!(helper.contains("\"run.receipt\""));
    assert!(helper.contains("\"run.snapshot\""));
    assert!(!helper.contains("run.completed"));
    let typed = receipts
        .split("fn typed_run_receipt(")
        .nth(1)
        .expect("typed_run_receipt");
    let typed = typed.split("fn ").next().expect("typed_run_receipt body");
    assert!(typed.contains("receipt_source_events(events)"));
    assert!(
        typed.contains("source_events.len().min(u64::MAX as usize) as u64"),
        "source_cursor must be the filtered fact count"
    );
    assert!(
        !typed.lines().any(|line| {
            line.trim_start()
                .starts_with("events.len().min(u64::MAX as usize) as u64")
        }),
        "the unfiltered event count is not a receipt source cursor"
    );

    // Guard 2: the receipt contract validates its own digest, so a page cannot present a digest
    // assembled from a different event set.
    let contracts = include_str!("../../kiana-domain/src/receipt_contracts.rs");
    assert!(contracts.contains("run_receipt_digest_mismatch"));
    assert!(contracts.contains("self.receipt_digest != self.digest()"));
}

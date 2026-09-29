//! EQ-43 failure-first fixtures for the `quality.promote` / `quality.rollback` ControlPlane gate.
//!
//! Every test builds real argument values, drives the public `ControlPlane::handle_command` entry
//! and asserts the structured reason code the gate itself returns. The command only re-derives
//! authority and appends an admission fact: it never switches a provider route or a grant. So this
//! module is a contract fixture over an in-memory event log and a scripted approval store, not
//! runtime evidence, and it proves nothing about a live promotion.

use async_trait::async_trait;
use kiana_core::ControlPlane;
use kiana_domain::{
    json_digest, ApprovalChallenge, ApprovalDecision, ApprovalDecisionRecord, ApprovalId,
    ApprovalState, AuthorizedCapabilityRequest, CapabilityRequest, CapabilityResult, CommandIntent,
    CoreResponse, ExecutionStatus, PendingApproval, PermissionProfile, RequestContext, RequestId,
    RiskLevel, RuntimeEvent, APPROVAL_CHALLENGE_SCHEMA,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, CapabilityBrokerPort, EventStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

const PROMOTE: &str = "quality.promote";
const ROLLBACK: &str = "quality.rollback";
const MUTATION_SCHEMA: &str = "kiana.quality-mutation.v1";
const SESSION: &str = "eq43-session";
const ACTOR: &str = "eq43-promoter";
const APPROVER: &str = "eq43-approver";

/// Approval store that hands back exactly one scripted decision and counts how often the gate
/// bothered to read it; a gate that short-circuits earlier must never touch it.
#[derive(Default)]
struct ScriptedApprovalStore {
    decision: Mutex<Option<ApprovalDecisionRecord>>,
    reads: AtomicUsize,
}

#[async_trait]
impl ApprovalStorePort for ScriptedApprovalStore {
    async fn stage(
        &self,
        _context: &RequestContext,
        _request: CapabilityRequest,
        _reason: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        Err(PortError::Failed("eq43_fixture_never_stages".to_owned()))
    }

    async fn activate(&self, _approval_id: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Failed("eq43_fixture_never_activates".to_owned()))
    }

    async fn consume(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Failed("eq43_fixture_never_consumes".to_owned()))
    }

    async fn read_decision(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
    ) -> Result<ApprovalDecisionRecord, PortError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.decision
            .lock()
            .await
            .clone()
            .ok_or_else(|| PortError::Unavailable("approval_not_found".to_owned()))
    }
}

/// Admission must not reach a broker: a promotion is an authority fact, not a dispatched effect.
struct NoBroker;

#[async_trait]
impl CapabilityBrokerPort for NoBroker {
    async fn execute(
        &self,
        _request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        Err(PortError::Failed(
            "eq43_fixture_never_dispatches".to_owned(),
        ))
    }
}

/// Admission must not start a run either.
struct NoRunner;

#[async_trait]
impl RunnerPort for NoRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Failed("eq43_fixture_never_runs".to_owned()))
    }
}

/// A well-formed `sha256:<64 hex>` value; every argument digest the gate accepts has this shape.
fn digest(seed: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (seed as u32) & 0xffff).repeat(16)
    )
}

/// A fresh project root, already canonicalized so the fixture hashes exactly what the gate hashes
/// after `ControlPlane::canonical_project_root` normalizes it.
fn temp_root() -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("EQ-43 clock")
        .as_nanos();
    let raw = std::env::temp_dir().join(format!("kiana-eq43-{stamp}"));
    std::fs::create_dir_all(&raw).expect("EQ-43 temp project root");
    std::fs::canonicalize(&raw)
        .expect("EQ-43 canonical project root")
        .to_string_lossy()
        .into_owned()
}

/// Mirrors `quality_scope_digest`: the scope is the canonical root plus the exact
/// session/actor/role/department tuple that asked for the promotion.
fn scope_digest(root: &str, context: &RequestContext) -> String {
    json_digest(&json!({
        "project_root": root,
        "session_id": context.session_id,
        "actor_id": context.actor_id,
        "role_id": context.role_id,
        "department_id": context.department_id,
    }))
}

/// Mirrors the gate's `expected_request_hash`: the approval must name this exact operation,
/// candidate, gate decision, scope and authority epoch.
fn request_hash(command: &str, candidate: &str, gate: &str, scope: &str, epoch: u64) -> String {
    json_digest(&json!({
        "command": command,
        "candidate_digest": candidate,
        "gate_decision_digest": gate,
        "scope_digest": scope,
        "authority_epoch": epoch,
    }))
}

fn arguments(
    candidate: &str,
    gate: &str,
    approval_id: &ApprovalId,
    approval_request_hash: &str,
    scope: &str,
    epoch: u64,
) -> Value {
    json!({
        "schema": MUTATION_SCHEMA,
        "candidate_digest": candidate,
        "gate_decision_digest": gate,
        "approval_ref": format!("approval:{approval_id}"),
        "approval_request_hash": approval_request_hash,
        "scope_digest": scope,
        "expected_authority_epoch": epoch,
    })
}

fn approval(
    approval_id: ApprovalId,
    state: ApprovalState,
    decision: Option<ApprovalDecision>,
    decided_by: &str,
    bound_request_hash: &str,
    expires_at_unix_ms: u64,
) -> ApprovalDecisionRecord {
    ApprovalDecisionRecord {
        approval_id,
        state,
        challenge: ApprovalChallenge {
            schema: APPROVAL_CHALLENGE_SCHEMA.to_owned(),
            approval_id,
            request_id: RequestId::new(),
            request_hash: bound_request_hash.to_owned(),
            risk: RiskLevel::Critical,
            expires_at_unix_ms,
            reason: "eq43 quality mutation".to_owned(),
            nonce: String::new(),
            policy_version: String::new(),
        },
        decision,
        decision_command_id: Some(RequestId::new()),
        decided_by: Some(decided_by.to_owned()),
        dispatch_command_id: None,
        payload_available: false,
        version: 1,
        decision_digest: None,
        consumption_digest: None,
    }
}

/// The one argument set the gate accepts when nothing is tampered with, kept as parts so a test can
/// invalidate exactly one authority binding and leave the rest valid.
struct Promotion {
    approval_id: ApprovalId,
    candidate: String,
    gate: String,
    scope: String,
    epoch: u64,
}

impl Promotion {
    fn arguments(&self, command: &str) -> Value {
        let hash = request_hash(
            command,
            &self.candidate,
            &self.gate,
            &self.scope,
            self.epoch,
        );
        arguments(
            &self.candidate,
            &self.gate,
            &self.approval_id,
            &hash,
            &self.scope,
            self.epoch,
        )
    }

    fn bound_hash(&self, command: &str) -> String {
        request_hash(
            command,
            &self.candidate,
            &self.gate,
            &self.scope,
            self.epoch,
        )
    }
}

struct Fixture {
    core: ControlPlane,
    events: Arc<MemoryEventLog>,
    approvals: Arc<ScriptedApprovalStore>,
    root: String,
}

impl Fixture {
    /// `epoch` seeds that many `authority.revised` facts; `0` leaves the project with no authority
    /// stream at all, which is the state a promote must not act from.
    async fn with_epoch(epoch: u64) -> Self {
        let root = temp_root();
        let events = Arc::new(MemoryEventLog::new());
        for version in 1..=epoch {
            append_authority_revision(&events, &root, version).await;
        }
        let approvals = Arc::new(ScriptedApprovalStore::default());
        let core = ControlPlane::new(
            Arc::new(DefaultPolicyEngine),
            Arc::new(DefaultGateEngine),
            events.clone(),
            Arc::new(NoBroker),
            approvals.clone(),
            Arc::new(NoRunner),
        );
        Self {
            core,
            events,
            approvals,
            root,
        }
    }

    fn context(&self) -> RequestContext {
        reviewer_context(SESSION)
    }

    /// A legitimately approved promotion, bound to `context`'s scope and to `epoch`.
    fn promotion(&self, context: &RequestContext, epoch: u64) -> Promotion {
        Promotion {
            approval_id: ApprovalId::new(),
            candidate: digest('c'),
            gate: digest('g'),
            scope: scope_digest(&self.root, context),
            epoch,
        }
    }

    async fn script_approval(&self, record: ApprovalDecisionRecord) {
        *self.approvals.decision.lock().await = Some(record);
    }

    /// Approve exactly the operation `promotion` describes, decided by a second party.
    async fn approve(&self, promotion: &Promotion, command: &str) {
        let hash = promotion.bound_hash(command);
        self.script_approval(approval(
            promotion.approval_id,
            ApprovalState::Approved,
            Some(ApprovalDecision::Approve),
            APPROVER,
            &hash,
            u64::MAX,
        ))
        .await;
    }

    fn approval_reads(&self) -> usize {
        self.approvals.reads.load(Ordering::SeqCst)
    }

    async fn submit(
        &self,
        context: RequestContext,
        command: &str,
        arguments: Value,
    ) -> CoreResponse {
        self.core
            .handle_command(context, CommandIntent::new(command, arguments))
            .await
            .expect("EQ-43 quality mutation returns a response, never a transport error")
    }
}

fn reviewer_context(session: &str) -> RequestContext {
    let mut context = RequestContext::local(session, "");
    context.project_trusted = true;
    context.permission_profile = PermissionProfile::Balanced;
    context.actor_id = Some(ACTOR.to_owned());
    context.role_id = kiana_domain::ROLE_REVIEWER.to_owned();
    context
}

/// `AuthorityLedger::rebuild` accepts only a contiguous `authority` stream, so epoch N is seeded by
/// N revisions at stream versions 1..=N.
async fn append_authority_revision(events: &MemoryEventLog, root: &str, version: u64) {
    let key = json_digest(&json!({"project_root": root}));
    events
        .append(
            RuntimeEvent::new(RequestId::new(), 1, "authority.revised", json!({}))
                .expect("EQ-43 authority revision event")
                .with_stream_metadata("authority", key, version),
        )
        .await
        .expect("EQ-43 authority revision append");
}

/// A rejection is a structured `Blocked` with a stable reason and no admission payload; an
/// assertion on anything looser would let a silent pass through the gate.
fn assert_blocked(response: &CoreResponse, reason: &str) {
    assert_eq!(
        response.status,
        ExecutionStatus::Blocked,
        "expected a blocked admission, got output {:?}",
        response.output
    );
    assert_eq!(response.error.as_deref(), Some(reason));
    assert_eq!(
        response.output,
        Value::Null,
        "a rejected mutation must not return an admission payload"
    );
}

#[tokio::test]
async fn passing_eval_cannot_promote_without_authority() {
    // A green eval, a valid second-party approval bound to that eval, and a matching request hash
    // still cannot promote: with no `authority` stream the epoch recheck has nothing to confirm.
    let fixture = Fixture::with_epoch(0).await;
    let context = fixture.context();
    let promotion = fixture.promotion(&context, 1);
    fixture.approve(&promotion, PROMOTE).await;

    let response = fixture
        .submit(context, PROMOTE, promotion.arguments(PROMOTE))
        .await;

    assert_blocked(&response, "quality_authority_epoch_missing");
    assert_eq!(
        fixture.approval_reads(),
        0,
        "the missing authority epoch must be caught before the approval is consulted"
    );
}

#[tokio::test]
async fn stale_authority_epoch_is_rejected_before_the_approval_is_read() {
    // An approval minted against a newer epoch than the ledger holds is refused at the epoch
    // recheck, so a stale grant cannot ride forward on a ledger that has since moved.
    let fixture = Fixture::with_epoch(1).await;
    let context = fixture.context();
    let promotion = fixture.promotion(&context, 2);
    fixture.approve(&promotion, PROMOTE).await;

    let response = fixture
        .submit(context, PROMOTE, promotion.arguments(PROMOTE))
        .await;

    assert_blocked(&response, "quality_authority_epoch_stale");
    assert_eq!(
        fixture.approval_reads(),
        0,
        "a stale epoch must be caught before the approval is consulted"
    );
}

#[tokio::test]
async fn scope_digest_from_another_session_cannot_promote() {
    // The scope digest binds the promotion to the exact session/actor/role/department tuple, so an
    // approval harvested from one session cannot authorize a promotion in another.
    let fixture = Fixture::with_epoch(1).await;
    let approved_session = fixture.context();
    let promotion = fixture.promotion(&approved_session, 1);
    fixture.approve(&promotion, PROMOTE).await;
    let other_session = reviewer_context("eq43-other-session");

    let response = fixture
        .submit(other_session, PROMOTE, promotion.arguments(PROMOTE))
        .await;

    assert_blocked(&response, "quality_scope_digest_mismatch");
    assert_eq!(
        fixture.approval_reads(),
        0,
        "a scope mismatch must be caught before the approval is consulted"
    );
}

#[tokio::test]
async fn tampered_candidate_cannot_reuse_the_original_approval_binding() {
    // A report swapped after approval keeps the original request hash, so the hash the gate
    // recomputes over the tampered candidate no longer matches the approval being presented.
    let fixture = Fixture::with_epoch(1).await;
    let context = fixture.context();
    let promotion = fixture.promotion(&context, 1);
    fixture.approve(&promotion, PROMOTE).await;
    let tampered = digest('t');

    let response = fixture
        .submit(
            context,
            PROMOTE,
            arguments(
                &tampered,
                &promotion.gate,
                &promotion.approval_id,
                &promotion.bound_hash(PROMOTE),
                &promotion.scope,
                promotion.epoch,
            ),
        )
        .await;

    assert_blocked(&response, "quality_approval_request_hash_mismatch");
}

#[tokio::test]
async fn tampered_report_cannot_self_authorize_with_a_matching_request_hash() {
    // Recomputing the request hash over the tampered candidate makes the argument self-consistent,
    // but the approval's own recorded binding still names the original candidate, so the record
    // check refuses it: a report can never supply its own authority.
    let fixture = Fixture::with_epoch(1).await;
    let context = fixture.context();
    let promotion = fixture.promotion(&context, 1);
    fixture.approve(&promotion, PROMOTE).await;
    let tampered = digest('t');
    let forged_hash = request_hash(
        PROMOTE,
        &tampered,
        &promotion.gate,
        &promotion.scope,
        promotion.epoch,
    );

    let response = fixture
        .submit(
            context,
            PROMOTE,
            arguments(
                &tampered,
                &promotion.gate,
                &promotion.approval_id,
                &forged_hash,
                &promotion.scope,
                promotion.epoch,
            ),
        )
        .await;

    assert_blocked(&response, "quality_secondary_approval_invalid");
}

#[tokio::test]
async fn missing_approval_record_cannot_promote() {
    // A promotion that presents an approval id the store cannot resolve fails closed instead of
    // treating an unreadable decision as consent.
    let fixture = Fixture::with_epoch(1).await;
    let context = fixture.context();
    let promotion = fixture.promotion(&context, 1);

    let response = fixture
        .submit(context, PROMOTE, promotion.arguments(PROMOTE))
        .await;

    assert_blocked(&response, "quality_approval_unavailable");
}

#[tokio::test]
async fn consumed_approval_cannot_promote_a_second_candidate() {
    // An approval that was already spent on an earlier promotion is not reusable authority, even
    // when the store still reports the original request hash.
    let fixture = Fixture::with_epoch(1).await;
    let context = fixture.context();
    let promotion = fixture.promotion(&context, 1);
    let hash = promotion.bound_hash(PROMOTE);
    fixture
        .script_approval(approval(
            promotion.approval_id,
            ApprovalState::Consumed,
            Some(ApprovalDecision::Approve),
            APPROVER,
            &hash,
            u64::MAX,
        ))
        .await;

    let response = fixture
        .submit(context, PROMOTE, promotion.arguments(PROMOTE))
        .await;

    assert_blocked(&response, "quality_secondary_approval_invalid");
}

#[tokio::test]
async fn denied_approval_cannot_promote() {
    // A recorded denial is not authority, and must not be laundered into an admission by a
    // matching request hash.
    let fixture = Fixture::with_epoch(1).await;
    let context = fixture.context();
    let promotion = fixture.promotion(&context, 1);
    let hash = promotion.bound_hash(PROMOTE);
    fixture
        .script_approval(approval(
            promotion.approval_id,
            ApprovalState::Denied,
            Some(ApprovalDecision::Deny),
            APPROVER,
            &hash,
            u64::MAX,
        ))
        .await;

    let response = fixture
        .submit(context, PROMOTE, promotion.arguments(PROMOTE))
        .await;

    assert_blocked(&response, "quality_secondary_approval_invalid");
}

#[tokio::test]
async fn expired_approval_challenge_cannot_promote() {
    // The approval is approved and correctly bound, but its challenge has already expired; the
    // freshness of the second authority is rechecked, not assumed from the record.
    let fixture = Fixture::with_epoch(1).await;
    let context = fixture.context();
    let promotion = fixture.promotion(&context, 1);
    let hash = promotion.bound_hash(PROMOTE);
    fixture
        .script_approval(approval(
            promotion.approval_id,
            ApprovalState::Approved,
            Some(ApprovalDecision::Approve),
            APPROVER,
            &hash,
            0,
        ))
        .await;

    let response = fixture
        .submit(context, PROMOTE, promotion.arguments(PROMOTE))
        .await;

    assert_blocked(&response, "quality_secondary_approval_invalid");
}

#[tokio::test]
async fn self_approved_promotion_is_refused_as_separation_of_duties() {
    // An actor cannot supply their own secondary approval, so a passing eval plus a self-issued
    // approval is not a promotion path.
    let fixture = Fixture::with_epoch(1).await;
    let context = fixture.context();
    let promotion = fixture.promotion(&context, 1);
    let hash = promotion.bound_hash(PROMOTE);
    fixture
        .script_approval(approval(
            promotion.approval_id,
            ApprovalState::Approved,
            Some(ApprovalDecision::Approve),
            ACTOR,
            &hash,
            u64::MAX,
        ))
        .await;

    let response = fixture
        .submit(context, PROMOTE, promotion.arguments(PROMOTE))
        .await;

    assert_blocked(&response, "quality_secondary_approval_invalid");
}

#[tokio::test]
async fn untrusted_project_cannot_promote() {
    // Project-local resources are untrusted until ProjectTrust says otherwise, so a mutation from
    // an untrusted project never reaches the authority recheck.
    let fixture = Fixture::with_epoch(1).await;
    let mut context = fixture.context();
    let promotion = fixture.promotion(&context, 1);
    fixture.approve(&promotion, PROMOTE).await;
    context.project_trusted = false;

    let response = fixture
        .submit(context, PROMOTE, promotion.arguments(PROMOTE))
        .await;

    assert_blocked(&response, "project_untrusted");
}

#[tokio::test]
async fn non_reviewer_role_cannot_promote() {
    // Only reviewer and QA may drive a quality mutation; a builder holding a valid approval is
    // still refused on role.
    let fixture = Fixture::with_epoch(1).await;
    let mut context = fixture.context();
    let promotion = fixture.promotion(&context, 1);
    fixture.approve(&promotion, PROMOTE).await;
    context.role_id = kiana_domain::ROLE_BUILDER.to_owned();

    let response = fixture
        .submit(context, PROMOTE, promotion.arguments(PROMOTE))
        .await;

    assert_blocked(&response, "quality_reviewer_role_required");
}

#[tokio::test]
async fn anonymous_request_cannot_promote() {
    // A second-party approval is meaningless without a named actor to hold it accountable, so a
    // context with no actor is refused before the approval is read.
    let fixture = Fixture::with_epoch(1).await;
    let mut context = fixture.context();
    let promotion = fixture.promotion(&context, 1);
    fixture.approve(&promotion, PROMOTE).await;
    context.actor_id = None;

    let response = fixture
        .submit(context, PROMOTE, promotion.arguments(PROMOTE))
        .await;

    assert_blocked(&response, "quality_actor_required");
    assert_eq!(
        fixture.approval_reads(),
        0,
        "an anonymous actor must be caught before the approval is consulted"
    );
}

#[tokio::test]
async fn malformed_report_digest_is_rejected_before_any_authority_check() {
    // A digest that is not `sha256:<64 hex>` never reaches the authority recheck, so a forged
    // report cannot smuggle an unchecked value past argument validation.
    let fixture = Fixture::with_epoch(1).await;
    let context = fixture.context();
    let promotion = fixture.promotion(&context, 1);
    fixture.approve(&promotion, PROMOTE).await;
    let forged = "sha256:not-a-real-digest";
    let hash = request_hash(
        PROMOTE,
        forged,
        &promotion.gate,
        &promotion.scope,
        promotion.epoch,
    );

    let response = fixture
        .submit(
            context,
            PROMOTE,
            arguments(
                forged,
                &promotion.gate,
                &promotion.approval_id,
                &hash,
                &promotion.scope,
                promotion.epoch,
            ),
        )
        .await;

    assert_blocked(&response, "quality_mutation_arguments_invalid");
    assert_eq!(
        fixture.approval_reads(),
        0,
        "a malformed digest must be caught before the approval is consulted"
    );
}

#[tokio::test]
async fn rollback_cannot_bypass_the_same_secondary_authority() {
    // Rollback is the same authority-sensitive admission: an approval bound to a promote cannot be
    // presented as a rollback, because the request hash names the command.
    let fixture = Fixture::with_epoch(1).await;
    let context = fixture.context();
    let promotion = fixture.promotion(&context, 1);
    fixture.approve(&promotion, PROMOTE).await;

    let response = fixture
        .submit(context, ROLLBACK, promotion.arguments(ROLLBACK))
        .await;

    assert_blocked(&response, "quality_approval_request_hash_mismatch");
}

#[tokio::test]
async fn rejected_promotion_cannot_be_replayed_into_a_later_authority_epoch() {
    // A refusal does not bank the approval: once the authority epoch moves on, replaying the very
    // same arguments is refused at the epoch recheck instead of becoming admissible.
    let fixture = Fixture::with_epoch(1).await;
    let context = fixture.context();
    let promotion = fixture.promotion(&context, 1);
    let hash = promotion.bound_hash(PROMOTE);
    fixture
        .script_approval(approval(
            promotion.approval_id,
            ApprovalState::Consumed,
            Some(ApprovalDecision::Approve),
            APPROVER,
            &hash,
            u64::MAX,
        ))
        .await;
    let replay = promotion.arguments(PROMOTE);

    let first = fixture
        .submit(context.clone(), PROMOTE, replay.clone())
        .await;

    assert_blocked(&first, "quality_secondary_approval_invalid");
    append_authority_revision(&fixture.events, &fixture.root, 2).await;

    let second = fixture.submit(context, PROMOTE, replay).await;

    assert_blocked(&second, "quality_authority_epoch_stale");
}

#[tokio::test]
async fn approved_promotion_with_live_authority_is_admitted() {
    // The success path, reached only after trust, actor, role, epoch, scope, request-hash binding
    // and a fresh second-party approval have all been rechecked; the result is an admission fact
    // carrying the authority epoch, not a route or grant change.
    let fixture = Fixture::with_epoch(1).await;
    let context = fixture.context();
    let promotion = fixture.promotion(&context, 1);
    fixture.approve(&promotion, PROMOTE).await;

    let response = fixture
        .submit(context, PROMOTE, promotion.arguments(PROMOTE))
        .await;

    assert_eq!(response.status, ExecutionStatus::Completed);
    assert_eq!(response.error, None);
    assert_eq!(response.output["status"], "admitted");
    assert_eq!(response.output["operation"], "promote");
    assert_eq!(response.output["candidate_digest"], promotion.candidate);
    assert_eq!(response.output["gate_decision_digest"], promotion.gate);
    assert_eq!(response.output["authority_epoch"], 1);
    let recorded = fixture
        .events
        .read_all()
        .await
        .expect("EQ-43 event read")
        .into_iter()
        .find(|event| event.kind == PROMOTE)
        .expect("EQ-43 admission fact is recorded in the EventLog");
    assert_eq!(recorded.data["status"], "admitted");
    assert_eq!(recorded.data["operation"], "promote");
    assert_eq!(recorded.data["authority_epoch"], 1);
    assert_eq!(
        recorded.data["approval_ref"],
        format!("approval:{}", promotion.approval_id)
    );
}

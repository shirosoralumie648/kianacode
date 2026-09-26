//! Pure quality calculations over committed, durable evidence.
//!
//! The quality crate does not own an EventLog, start a Runner, contact a provider or dispatch a
//! capability. EQ-17--19 only select and normalize bounded durable evidence; digests, diffing,
//! evaluation and persistence are explicit later stages.

mod aggregation;
mod assertions;
mod baseline;
mod candidate;
mod canonical;
mod capture;
mod catalog;
mod context;
mod diff;
mod digest;
mod evaluator;
mod evidence;
mod experiment;
mod fixtures;
mod flake;
mod gate;
mod judge;
mod metrics;
mod normalize;
mod recovery;
mod runtime;
mod safety;
mod scenario;
mod volatile;
mod workflow;

pub use aggregation::{
    evaluate_aggregation, AggregateVerdict, AggregationEvaluator, AggregationInput,
    DimensionAggregate, AGGREGATION_EVALUATOR_ID, AGGREGATION_INPUT_SCHEMA,
};
pub use assertions::{
    evaluate_assertion, evaluate_assertions, Assertion, AssertionError, AssertionMode,
    AssertionResult, ASSERTION_SCHEMA, MAX_ASSERTIONS,
};
pub use baseline::{
    evaluate_baseline_comparison, BaselineComparisonInput, BaselineEvaluator, BaselineRecord,
    BaselineRegistry, BASELINE_EVALUATOR_ID, BASELINE_INPUT_SCHEMA,
};
pub use candidate::{
    CandidateDimension, CandidateEvaluator, CandidateInput, CandidateStatus, QualityCandidate,
    CANDIDATE_EVALUATOR_ID, CANDIDATE_INPUT_SCHEMA,
};
pub use canonical::{
    canonical_json, ArrayPolicy, CanonicalEvent, CanonicalEventTrace, CanonicalizationError,
    CANONICAL_NORMALIZATION_VERSION,
};
pub use capture::{
    capture_golden_trace, CaptureError, CaptureSource, CaptureSourceKind,
    GoldenTraceCaptureReceipt, GoldenTraceCaptureRequest, CAPTURE_SCHEMA,
    MAX_DESTINATION_REF_BYTES,
};
pub use catalog::{
    build_catalog, CatalogEntry, CatalogError, CatalogOptions, CatalogScenario, CatalogStatus,
    CatalogTier, ScenarioCatalog, CATALOG_SCHEMA, MAX_CATALOG_ENTRIES,
};
pub use context::{
    evaluate_context_memory, CompactionEvidence, ContextBudgetEvidence, ContextEvidenceStatus,
    ContextFreshness, ContextMemoryEvaluator, ContextMemoryInput, ContextQueryEvidence,
    MemoryHitEvidence, CONTEXT_MEMORY_EVALUATOR_ID, CONTEXT_MEMORY_INPUT_SCHEMA,
};
pub use diff::{TraceDiff, TraceDiffClass, TraceDivergence, TRACE_DIFF_SCHEMA};
pub use digest::{
    artifact_digest, event_digest, receipt_digest, trace_digest, DigestError, DigestKind,
    VersionedEvidenceDigest, DIGEST_SCHEMA,
};
pub use evaluator::{
    canonical_findings, digest_findings, findings_digest, sort_findings, validate_finding,
    DeterministicEvaluator, EvaluationInput, EvaluationResult, Evaluator, EvaluatorError,
    EvaluatorRegistry, Finding, FindingError, EVALUATION_RESULT_SCHEMA, EVALUATOR_INPUT_SCHEMA,
    FINDING_SCHEMA, MAX_EVALUATORS, MAX_EVALUATOR_INPUT_BYTES, MAX_FINDINGS,
    MAX_FINDING_CODE_BYTES, MAX_FINDING_EVIDENCE_REF_BYTES, MAX_FINDING_MESSAGE_BYTES,
    MAX_FINDING_VALUE_BYTES, MAX_FINDING_VALUE_DEPTH,
};
pub use evidence::{
    evaluate_evidence_receipt, ArtifactEvidence, EvidenceReceiptEvaluator, EvidenceReceiptInput,
    ProvenanceEvidence, ReceiptAssertion, ReceiptEvidence, RedactionEvidence, SourceCursorEvidence,
    EVIDENCE_RECEIPT_EVALUATOR_ID, EVIDENCE_RECEIPT_INPUT_SCHEMA,
};
pub use experiment::{
    replay_experiment, EvalExperiment, EvalResultIndex, ExperimentEvent, ExperimentEventKind,
    ExperimentReplayError, ExperimentReplayEvaluator, ExperimentReplayInput, ExperimentStatus,
    EXPERIMENT_REPLAY_EVALUATOR_ID, EXPERIMENT_REPLAY_INPUT_SCHEMA,
};
pub use fixtures::{
    core_negative_fixture_matrix, validate_core_negative_fixture_matrix, FixtureError,
    FixtureFamily, TraceFixture, FIXTURE_SCHEMA,
};
pub use flake::{
    evaluate_flake, AttemptOutcome, FlakeAttempt, FlakeClassifierEvaluator, FlakeFinalStatus,
    FlakeInput, FlakeQuarantine, InfraFailureClass, FLAKE_EVALUATOR_ID, FLAKE_INPUT_SCHEMA,
};
pub use gate::{
    evaluate_gate, GateEvaluator, GateInput, GateVerdict, QualityGateConfig, QualityGateDecision,
    GATE_EVALUATOR_ID, GATE_INPUT_SCHEMA,
};
pub use judge::{
    evaluate_semantic_judge, JudgeAvailability, JudgeVerdict, SemanticJudgeConfig,
    SemanticJudgeEvaluator, SemanticJudgeInput, SemanticJudgeRequest, SemanticJudgeResult,
    SEMANTIC_JUDGE_EVALUATOR_ID, SEMANTIC_JUDGE_INPUT_SCHEMA,
};
pub use metrics::{
    evaluate_performance_cost, CostBucket, PerformanceCostEvaluator, PerformanceCostInput,
    PerformanceMetrics, PerformanceThresholds, PERFORMANCE_COST_EVALUATOR_ID,
    PERFORMANCE_COST_INPUT_SCHEMA,
};
pub use normalize::{
    DurableEvent, DurableEventSelection, NormalizationError, TraceNormalizer,
    TRACE_NORMALIZATION_VERSION,
};
pub use recovery::{
    evaluate_recovery_replay, CrashRestartEvidence, RecoveryReplayEvaluator, RecoveryReplayInput,
    ReplayEvidence, ReplayFenceEvidence, UnknownReconcileEvidence, RECOVERY_REPLAY_EVALUATOR_ID,
    RECOVERY_REPLAY_INPUT_SCHEMA,
};
pub use runtime::{
    evaluate_runtime_correctness, RuntimeCorrectnessEvaluator, RuntimeCorrectnessInput,
    RUNTIME_CORRECTNESS_EVALUATOR_ID, RUNTIME_CORRECTNESS_INPUT_SCHEMA,
};
pub use safety::{
    evaluate_capability_safety, CapabilitySafetyEvaluator, CapabilitySafetyInput,
    ObservedSafetyEffect, SafetyAction, SafetyEffectKind, SafetyFinalStatus, SafetyGrant,
    SafetyVerdict, SafetyVerdictEvidence, CAPABILITY_SAFETY_EVALUATOR_ID,
    CAPABILITY_SAFETY_INPUT_SCHEMA,
};
pub use scenario::{
    CleanupReceipt, ScenarioArtifact, ScenarioError, ScenarioOutcome, ScenarioReport,
    ScenarioRunner, ScenarioSpec, ScopePredicate, ScrubbedEnvironment, CLEANUP_RECEIPT_SCHEMA,
    SCENARIO_SCHEMA, SCRUBBED_ENVIRONMENT_SCHEMA,
};
pub use volatile::{
    normalize_volatile, normalize_volatile_trace, standard_uuid_rule_paths, VolatileError,
    VolatileEvent, VolatileEventTrace, VolatileKind, VolatilePolicy, VolatileReplacement,
    VolatileRule, MAX_VOLATILE_REPLACEMENTS, MAX_VOLATILE_RULES, VOLATILE_NORMALIZATION_VERSION,
};
pub use workflow::{
    evaluate_workflow_swarm, WorkflowNodeEvidence, WorkflowNodeStatus, WorkflowSwarmEvaluator,
    WorkflowSwarmInput, WORKFLOW_SWARM_EVALUATOR_ID, WORKFLOW_SWARM_INPUT_SCHEMA,
};

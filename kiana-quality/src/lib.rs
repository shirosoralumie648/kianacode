//! Pure quality calculations over committed, durable evidence.
//!
//! The quality crate does not own an EventLog, start a Runner, contact a provider or dispatch a
//! capability. EQ-17--19 only select and normalize bounded durable evidence; digests, diffing,
//! evaluation and persistence are explicit later stages.

mod assertions;
mod canonical;
mod capture;
mod catalog;
mod diff;
mod digest;
mod fixtures;
mod normalize;
mod scenario;
mod volatile;

pub use assertions::{
    evaluate_assertion, evaluate_assertions, Assertion, AssertionError, AssertionMode,
    AssertionResult, ASSERTION_SCHEMA, MAX_ASSERTIONS,
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
pub use diff::{TraceDiff, TraceDiffClass, TraceDivergence, TRACE_DIFF_SCHEMA};
pub use digest::{
    artifact_digest, event_digest, receipt_digest, trace_digest, DigestError, DigestKind,
    VersionedEvidenceDigest, DIGEST_SCHEMA,
};
pub use fixtures::{
    core_negative_fixture_matrix, validate_core_negative_fixture_matrix, FixtureError,
    FixtureFamily, TraceFixture, FIXTURE_SCHEMA,
};
pub use normalize::{
    DurableEvent, DurableEventSelection, NormalizationError, TraceNormalizer,
    TRACE_NORMALIZATION_VERSION,
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

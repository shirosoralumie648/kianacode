//! Pure quality calculations over committed, durable evidence.
//!
//! The quality crate does not own an EventLog, start a Runner, contact a provider or dispatch a
//! capability. EQ-17--19 only select and normalize bounded durable evidence; digests, diffing,
//! evaluation and persistence are explicit later stages.

mod canonical;
mod normalize;
mod volatile;

pub use canonical::{
    canonical_json, ArrayPolicy, CanonicalEvent, CanonicalEventTrace, CanonicalizationError,
    CANONICAL_NORMALIZATION_VERSION,
};
pub use normalize::{
    DurableEvent, DurableEventSelection, NormalizationError, TraceNormalizer,
    TRACE_NORMALIZATION_VERSION,
};
pub use volatile::{
    normalize_volatile, normalize_volatile_trace, standard_uuid_rule_paths, VolatileError,
    VolatileEvent, VolatileEventTrace, VolatileKind, VolatilePolicy, VolatileReplacement,
    VolatileRule, MAX_VOLATILE_REPLACEMENTS, MAX_VOLATILE_RULES, VOLATILE_NORMALIZATION_VERSION,
};

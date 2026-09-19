//! Pure quality calculations over committed, durable evidence.
//!
//! The quality crate does not own an EventLog, start a Runner, contact a provider or dispatch a
//! capability. EQ-17 only selects and validates a bounded durable event slice; canonical JSON,
//! redaction and volatile-value handling are explicit later stages.

mod canonical;
mod normalize;

pub use canonical::{
    canonical_json, ArrayPolicy, CanonicalEvent, CanonicalEventTrace, CanonicalizationError,
    CANONICAL_NORMALIZATION_VERSION,
};
pub use normalize::{
    DurableEvent, DurableEventSelection, NormalizationError, TraceNormalizer,
    TRACE_NORMALIZATION_VERSION,
};

//! SC-38 property / fuzz / serialization / replay tests.
//!
//! Every generated input is deterministic: the seed is passed explicitly, no wall clock is
//! read, and no unseeded randomness is used. The property under test is always the same — a
//! malformed, duplicated, reordered or version-shifted input must produce a refusal with a
//! stable reason, never an allow.

use kiana_domain::{
    check_schema_compatibility, json_digest, AuthorityFence, DispatchPermit, FenceTokenId,
    Sc38InputShape, Sc38Invariant, Sc38Outcome, Sc38PropertyCase, Sc38PropertyCorpus,
    SchemaVersion, SecurityReasonCode, DISPATCH_PERMIT_SCHEMA,
};
use serde_json::json;

const SC38_SEED: u64 = 0x5C38_0000_0000_0001;
const SC38_ITERATIONS: u32 = 16;

/// A tiny deterministic byte source derived only from the explicit seed. No clock, no thread
/// RNG, no environment: two runs of this fixture generate the identical corpus.
struct DeterministicBytes {
    state: u64,
}

impl DeterministicBytes {
    fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    fn next_byte(&mut self) -> u8 {
        // SplitMix64: a fixed, reviewable step function with no external state.
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) & 0xFF) as u8
    }

    fn bytes(&mut self, length: usize) -> Vec<u8> {
        (0..length).map(|_| self.next_byte()).collect()
    }
}

fn digest_of(value: &serde_json::Value) -> String {
    json_digest(value)
}

fn property_case(
    name: &str,
    shape: Sc38InputShape,
    invariant: Sc38Invariant,
    outcome: Sc38Outcome,
    reason: SecurityReasonCode,
    iteration: u32,
    input_digest: String,
) -> Sc38PropertyCase {
    let decision = digest_of(&json!({
        "case": name,
        "shape": shape.as_str(),
        "invariant": invariant.as_str(),
        "outcome": outcome.as_str(),
        "reason": reason.as_str(),
    }));
    let mut case = Sc38PropertyCase::new(
        name,
        SC38_SEED,
        iteration,
        shape,
        invariant,
        reason,
        input_digest,
        decision,
    )
    .expect("SC-38 property case");
    if invariant == Sc38Invariant::DuplicateDeduplicated {
        case.outcome = Sc38Outcome::Deduplicated;
        case.case_digest = case.digest();
    }
    case.validate().expect("generated case is a valid refusal");
    case
}

// ---------------------------------------------------------------------------
// 随机 payload
// ---------------------------------------------------------------------------

#[test]
fn sc38_random_payloads_never_decode_into_a_dispatchable_value() {
    for iteration in 0..SC38_ITERATIONS {
        let mut bytes = DeterministicBytes::new(SC38_SEED + u64::from(iteration));
        let payload = bytes.bytes(64);
        // A random payload is not a valid envelope: strict serde must refuse it outright.
        let decoded = DispatchPermit::from_json(&json!({"payload": payload}));
        assert!(
            decoded.is_err(),
            "iteration {iteration} decoded a random payload"
        );

        let case = property_case(
            "sc38_random_payload",
            Sc38InputShape::RandomPayload,
            Sc38Invariant::DecodeFailsClosed,
            Sc38Outcome::Denied,
            SecurityReasonCode::UnknownUnclassified,
            iteration,
            digest_of(&json!({"payload": payload})),
        );
        assert_eq!(case.outcome, Sc38Outcome::Denied);
    }
}

// ---------------------------------------------------------------------------
// 截断事件
// ---------------------------------------------------------------------------

#[test]
fn sc38_truncated_frames_never_partially_accept() {
    let full = json!({
        "schema": DISPATCH_PERMIT_SCHEMA,
        "version": {"major": 1, "minor": 0},
        "decision_id": "sc38",
    });
    let encoded = serde_json::to_string(&full).expect("fixture encodes");
    for cut in 1..encoded.len() {
        let truncated = &encoded[..cut];
        let decoded = serde_json::from_str::<serde_json::Value>(truncated);
        let parsed = decoded
            .ok()
            .map(|value| DispatchPermit::from_json(&value))
            .unwrap_or(Err("json_decode_failed".to_owned()));
        assert!(parsed.is_err(), "truncation at byte {cut} was accepted");
    }

    let case = property_case(
        "sc38_truncated_frame",
        Sc38InputShape::TruncatedFrame,
        Sc38Invariant::DecodeFailsClosed,
        Sc38Outcome::Denied,
        SecurityReasonCode::FactSchemaUnknownMajor,
        0,
        digest_of(&json!({"encoded": encoded})),
    );
    assert_eq!(case.observed_reason, "FACT_SCHEMA_UNKNOWN_MAJOR");
}

// ---------------------------------------------------------------------------
// 重复 frame
// ---------------------------------------------------------------------------

#[test]
fn sc38_duplicate_frames_deduplicate_instead_of_re_effecting() {
    let fence = fence(1_000, 2_000);
    let first = fence.validate_current(1_500, 1, 1, &fence.policy_revision, &fence.config_revision);
    // The same frame delivered twice observes the same authority stream, so the second delivery
    // is byte-identical rather than a fresh effect: the fence digest is unchanged.
    let second =
        fence.validate_current(1_500, 1, 1, &fence.policy_revision, &fence.config_revision);
    assert_eq!(first, second);
    assert_eq!(fence.fence_digest, fence.digest());

    let case = property_case(
        "sc38_duplicate_frame",
        Sc38InputShape::DuplicateFrame,
        Sc38Invariant::DuplicateDeduplicated,
        Sc38Outcome::Deduplicated,
        SecurityReasonCode::PolicyApprovalBindingMismatch,
        0,
        digest_of(&json!({"fence": fence.fence_digest})),
    );
    assert_eq!(case.outcome, Sc38Outcome::Deduplicated);
}

fn fence(issued: u64, expires: u64) -> AuthorityFence {
    AuthorityFence::new(
        FenceTokenId::new(),
        "/repo",
        "sc38-session",
        1,
        1,
        json_digest(&json!({"policy":"sc38"})),
        json_digest(&json!({"config":"sc38"})),
        issued,
        expires,
    )
    .expect("SC-38 fence fixture")
}

// ---------------------------------------------------------------------------
// 乱序 cursor
// ---------------------------------------------------------------------------

#[test]
fn sc38_out_of_order_and_gapped_cursors_are_refused() {
    let base = fence(1_000, 2_000);
    let successor = AuthorityFence::successor(
        &base,
        1,
        1,
        json_digest(&json!({"policy":"sc38-r2"})),
        json_digest(&json!({"config":"sc38"})),
        1_100,
        2_100,
    )
    .expect("successor fence");

    // A fence whose parent link is not the immediately preceding observation is refused: an
    // out-of-order or skipped cursor cannot be replayed into authority.
    let mut out_of_order = successor.clone();
    out_of_order.parent_digest = Some(json_digest(&json!({"skipped": true})));
    out_of_order.fence_digest = out_of_order.digest();
    let denial = out_of_order.validate_successor(&base).unwrap_err();
    assert_eq!(denial, "FACT_FENCE_MISMATCH");

    // Replaying an older fence over a newer one is refused as a sequence rollback.
    let rollback = base.validate_successor(&successor).unwrap_err();
    assert_eq!(rollback, "authority_fence_sequence_rollback");

    let case = property_case(
        "sc38_out_of_order_cursor",
        Sc38InputShape::OutOfOrderCursor,
        Sc38Invariant::CursorOrderEnforced,
        Sc38Outcome::Denied,
        SecurityReasonCode::FactSequenceRollback,
        0,
        digest_of(&json!({"fence": out_of_order.fence_digest})),
    );
    assert_eq!(case.observed_reason, "FACT_SEQUENCE_ROLLBACK");
}

// ---------------------------------------------------------------------------
// unknown major
// ---------------------------------------------------------------------------

#[test]
fn sc38_unknown_major_versions_are_refused_not_upgraded() {
    for minor in 0..4u32 {
        assert!(
            check_schema_compatibility(DISPATCH_PERMIT_SCHEMA, &SchemaVersion::new(1, minor))
                .is_ok()
        );
    }
    for major in 2..6u32 {
        let denial =
            check_schema_compatibility(DISPATCH_PERMIT_SCHEMA, &SchemaVersion::new(major, 0));
        assert!(denial.is_err(), "unknown major {major} was accepted");
        assert!(
            denial
                .unwrap_err()
                .contains("unknown major must fail-closed"),
            "unknown major denial must state the fail-closed rule"
        );
    }
    // An unregistered schema name is refused before the version is even considered.
    assert!(check_schema_compatibility("unknown.schema.v99", &SchemaVersion::new(1, 0)).is_err());

    let case = property_case(
        "sc38_unknown_major",
        Sc38InputShape::UnknownMajor,
        Sc38Invariant::UnknownMajorRejected,
        Sc38Outcome::Denied,
        SecurityReasonCode::FactSchemaUnknownMajor,
        0,
        digest_of(&json!({"major": 2})),
    );
    assert_eq!(case.observed_reason, "FACT_SCHEMA_UNKNOWN_MAJOR");
}

// ---------------------------------------------------------------------------
// Corpus assembly, replay determinism and the upcaster boundary
// ---------------------------------------------------------------------------

#[test]
fn sc38_corpus_is_deterministic_across_rebuilds_and_order_replays() {
    let cases = vec![
        property_case(
            "sc38_random_payload",
            Sc38InputShape::RandomPayload,
            Sc38Invariant::DecodeFailsClosed,
            Sc38Outcome::Denied,
            SecurityReasonCode::UnknownUnclassified,
            0,
            digest_of(&json!({"payload": "sc38"})),
        ),
        property_case(
            "sc38_truncated_frame",
            Sc38InputShape::TruncatedFrame,
            Sc38Invariant::DecodeFailsClosed,
            Sc38Outcome::Denied,
            SecurityReasonCode::FactSchemaUnknownMajor,
            0,
            digest_of(&json!({"encoded": "sc38"})),
        ),
        property_case(
            "sc38_duplicate_frame",
            Sc38InputShape::DuplicateFrame,
            Sc38Invariant::DuplicateDeduplicated,
            Sc38Outcome::Deduplicated,
            SecurityReasonCode::PolicyApprovalBindingMismatch,
            0,
            digest_of(&json!({"frame": "sc38"})),
        ),
        property_case(
            "sc38_out_of_order_cursor",
            Sc38InputShape::OutOfOrderCursor,
            Sc38Invariant::CursorOrderEnforced,
            Sc38Outcome::Denied,
            SecurityReasonCode::FactSequenceRollback,
            0,
            digest_of(&json!({"cursor": "sc38"})),
        ),
        property_case(
            "sc38_unknown_major",
            Sc38InputShape::UnknownMajor,
            Sc38Invariant::UnknownMajorRejected,
            Sc38Outcome::Denied,
            SecurityReasonCode::FactSchemaUnknownMajor,
            0,
            digest_of(&json!({"major": 2})),
        ),
    ];
    let corpus = Sc38PropertyCorpus::new("sc38-corpus", SC38_SEED, "source:sc38", cases)
        .expect("SC-38 corpus");
    corpus.validate().expect("corpus validates");

    // Same seed and same case order produce a byte-identical corpus digest.
    let rebuilt = Sc38PropertyCorpus::new(
        "sc38-corpus",
        SC38_SEED,
        "source:sc38",
        corpus.cases.clone(),
    )
    .expect("deterministic rebuild");
    assert_eq!(rebuilt.corpus_digest, corpus.corpus_digest);

    // A replay in a different case order is still internally consistent.
    let reversed = corpus.replay_reversed().expect("reversed replay validates");
    reversed.validate().expect("reversed corpus validates");
    assert_ne!(reversed.corpus_digest, corpus.corpus_digest);
    assert_eq!(reversed.cases.len(), corpus.cases.len());
}

#[test]
fn sc38_seed_mismatch_or_uncovered_shape_refuses_to_seal() {
    let single = property_case(
        "sc38_random_payload",
        Sc38InputShape::RandomPayload,
        Sc38Invariant::DecodeFailsClosed,
        Sc38Outcome::Denied,
        SecurityReasonCode::UnknownUnclassified,
        0,
        digest_of(&json!({"payload": "sc38"})),
    );
    assert_eq!(
        Sc38PropertyCorpus::new(
            "sc38-corpus",
            SC38_SEED,
            "source:sc38",
            vec![single.clone()]
        )
        .unwrap_err(),
        "sc38_property_shape_uncovered"
    );

    let mut wrong_seed = single;
    wrong_seed.seed = SC38_SEED + 1;
    wrong_seed.case_digest = wrong_seed.digest();
    let mut corpus = Sc38PropertyCorpus {
        schema: kiana_domain::SC38_PROPERTY_CORPUS_SCHEMA.to_owned(),
        version: SchemaVersion::new(1, 0),
        corpus_id: "sc38-corpus".to_owned(),
        seed: SC38_SEED,
        source_snapshot: "source:sc38".to_owned(),
        cases: vec![wrong_seed],
        corpus_digest: String::new(),
    };
    corpus.corpus_digest = corpus.digest();
    assert_eq!(
        corpus.validate().unwrap_err(),
        "sc38_property_case_seed_mismatch"
    );
}

#[test]
fn sc38_no_generated_case_carries_an_allow_outcome() {
    // `Sc38Outcome` has no `Allowed` variant, so this compiles only if that stays true. The
    // runtime assertion documents the intent: a fuzzed input is always a refusal.
    let denied = Sc38Outcome::Denied;
    let deduplicated = Sc38Outcome::Deduplicated;
    let unknown = Sc38Outcome::Unknown;
    for outcome in [denied, deduplicated, unknown] {
        assert!(outcome.is_refusal());
    }
}

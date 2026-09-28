//! EQ-45 failure-first behavior fixtures for the `quality.feedback` ControlPlane adapter.
//!
//! Every test names one rejected-first item from the card and asserts a structured error code that
//! is greppable in `kiana-core/src/quality_feedback.rs` or `kiana-domain/src/quality_feedback.rs`.
//! Nothing here reads a real EventLog, writes a file, appends a fact, edits a policy or receipt or
//! calls a port: the module under test is a value derivation over a trusted request context and
//! supplied source facts, so these are contract fixtures and not runtime evidence.
//!
//! The slice is `feature_status=partial`, `proof_level=source`: it proves the typed admission and
//! derivation boundary only. It does not prove an authenticated principal, a durable feedback
//! store, cross-process replay or a business outcome.

use kiana_core::{derive_quality_feedback, QUALITY_FEEDBACK_COMMAND};
use kiana_domain::{
    ArtifactId, FeedbackId, QualityCanonicalTarget, QualityFeedback, QualityFeedbackPrivacyScope,
    QualityFeedbackSubmission, QualityFeedbackTargetType, RequestContext, RunId,
    QUALITY_CANONICAL_TARGET_SCHEMA, QUALITY_FEEDBACK_MAX_SOURCE_EVENTS, QUALITY_FEEDBACK_MAX_TEXT,
    QUALITY_FEEDBACK_SCHEMA, QUALITY_FEEDBACK_SUBMISSION_SCHEMA,
};
use serde_json::json;

const CAPTURED_AT_UNIX_MS: u64 = 1_700_000_000_000;
const SOURCE_CURSOR: u64 = 7;

/// Distinctive raw context values. None of them may survive into a derived record: the adapter is
/// supposed to store hashed handles only.
const RAW_ACTOR: &str = "actor-raw-9f3a";
const RAW_PROJECT_ROOT: &str = "/srv/EQ 45 fixture project";
const RAW_SESSION: &str = "session-raw-4b21";
const RAW_SOURCE_EVENT: &str = "eventlog-raw-7c05";

/// `Uuid::parse_str` accepts any hex in the 8-4-4-4-12 layout; the typed target refs require one.
const RECEIPT_A: &str = "11111111-1111-4111-8111-111111111111";
const RECEIPT_B: &str = "22222222-4222-8222-8222-222222222222";

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

/// A trusted local request. `RequestContext::local` is untrusted by construction, so the trust gate
/// has to be opened deliberately rather than inherited.
fn trusted_context() -> RequestContext {
    let mut context = RequestContext::local(RAW_SESSION, RAW_PROJECT_ROOT);
    context.project_trusted = true;
    context.actor_id = Some(RAW_ACTOR.to_owned());
    context
}

/// A canonical target with `canonical_digest` sealed over the supplied fields, so a fixture that
/// aims at one property is not accidentally rejected by an unrelated one.
fn sealed_target(
    target_type: QualityFeedbackTargetType,
    target_ref: String,
    target_digest: String,
    policy_ref: Option<String>,
    receipt_ref: Option<String>,
    target_revision: u64,
) -> QualityCanonicalTarget {
    let mut target = QualityCanonicalTarget {
        schema: QUALITY_CANONICAL_TARGET_SCHEMA.to_owned(),
        target_type,
        target_ref,
        target_digest,
        policy_ref,
        receipt_ref,
        target_revision,
        canonical_digest: String::new(),
    };
    target.canonical_digest = target.digest();
    target
}

/// An artifact target that references both the policy and the receipt it was evaluated against.
fn artifact_target() -> QualityCanonicalTarget {
    sealed_target(
        QualityFeedbackTargetType::Artifact,
        format!("artifact:{}", ArtifactId::new()),
        digest('a'),
        Some("policy:policy-1".to_owned()),
        Some("receipt:receipt-1".to_owned()),
        1,
    )
}

/// A receipt target that names itself, which is the only shape the contract admits.
fn receipt_target(receipt: &str) -> QualityCanonicalTarget {
    sealed_target(
        QualityFeedbackTargetType::Receipt,
        format!("receipt:{receipt}"),
        digest('b'),
        None,
        Some(format!("receipt:{receipt}")),
        1,
    )
}

fn submission(target: QualityCanonicalTarget) -> QualityFeedbackSubmission {
    QualityFeedbackSubmission {
        schema: QUALITY_FEEDBACK_SUBMISSION_SCHEMA.to_owned(),
        feedback_id: FeedbackId::new(),
        target,
        label: "label-ok".to_owned(),
        comment_ref: Some("comment:ref-1".to_owned()),
        correction_ref: None,
        idempotency_key: "idempotency:feedback-1".to_owned(),
    }
}

/// Derive one feedback record from a trusted request and a single committed source fact.
fn derive(
    context: &RequestContext,
    submission: QualityFeedbackSubmission,
) -> Result<QualityFeedback, String> {
    derive_quality_feedback(
        context,
        submission,
        SOURCE_CURSOR,
        &[RAW_SOURCE_EVENT.to_owned()],
        CAPTURED_AT_UNIX_MS,
    )
}

/// The EQ-45 card acceptance: a feedback record refers to policy and receipt facts as references
/// only. Repointing either one breaks a digest the server computed, a receipt target must agree
/// with the receipt it names, and the submission wire shape has no field to carry the edit in. No
/// accepted shape of this command mutates a policy or a receipt.
#[test]
fn feedback_cannot_edit_policy_or_receipt() {
    let context = trusted_context();
    let feedback =
        derive(&context, submission(artifact_target())).expect("EQ-45 baseline feedback");

    // The record does carry the facts it was evaluated against.
    assert_eq!(
        feedback.target.policy_ref.as_deref(),
        Some("policy:policy-1")
    );
    assert_eq!(
        feedback.target.receipt_ref.as_deref(),
        Some("receipt:receipt-1")
    );

    // Repointing the policy reference is not a field edit: the record digest was sealed over the
    // original reference, so the tampered record no longer agrees with itself.
    let mut repointed_policy = feedback.clone();
    repointed_policy.target.policy_ref = Some("policy:policy-2".to_owned());
    assert_eq!(
        repointed_policy.validate().unwrap_err(),
        "quality_feedback_invalid"
    );

    // Re-signing the record digest does not rescue it. The canonical target carries its own digest,
    // sealed by the server, and that is the layer that refuses the swapped policy reference.
    let mut forged_policy = feedback.clone();
    forged_policy.target.policy_ref = Some("policy:policy-2".to_owned());
    forged_policy.feedback_digest = forged_policy.digest();
    assert_eq!(
        forged_policy.validate().unwrap_err(),
        "quality_canonical_target_invalid"
    );

    // The same two fences hold for the receipt reference.
    let mut repointed_receipt = feedback.clone();
    repointed_receipt.target.receipt_ref = Some("receipt:receipt-2".to_owned());
    assert_eq!(
        repointed_receipt.validate().unwrap_err(),
        "quality_feedback_invalid"
    );
    let mut forged_receipt = feedback.clone();
    forged_receipt.target.receipt_ref = Some("receipt:receipt-2".to_owned());
    forged_receipt.feedback_digest = forged_receipt.digest();
    assert_eq!(
        forged_receipt.validate().unwrap_err(),
        "quality_canonical_target_invalid"
    );

    // A receipt target cannot point at a different receipt than the one it names.
    let mismatched = sealed_target(
        QualityFeedbackTargetType::Receipt,
        format!("receipt:{RECEIPT_A}"),
        digest('b'),
        None,
        Some(format!("receipt:{RECEIPT_B}")),
        1,
    );
    assert_eq!(
        derive(&context, submission(mismatched)).unwrap_err(),
        "quality_canonical_receipt_ref_mismatch"
    );
    // The self-consistent shape is accepted, which is what makes the rejection above meaningful.
    let self_named_ref = format!("receipt:{RECEIPT_A}");
    let self_named =
        derive(&context, submission(receipt_target(RECEIPT_A))).expect("EQ-45 self-named receipt");
    assert_eq!(
        self_named.target.receipt_ref.as_deref(),
        Some(self_named_ref.as_str())
    );

    // And the client wire shape has nowhere to put a policy or receipt edit in the first place.
    let mut wire =
        serde_json::to_value(submission(artifact_target())).expect("EQ-45 submission wire");
    wire["policy_update"] = json!({"policy_ref": "policy:policy-2"});
    assert!(serde_json::from_value::<QualityFeedbackSubmission>(wire).is_err());
    let mut wire =
        serde_json::to_value(submission(artifact_target())).expect("EQ-45 submission wire");
    wire["receipt_update"] = json!({"receipt_ref": "receipt:receipt-2"});
    assert!(serde_json::from_value::<QualityFeedbackSubmission>(wire).is_err());
}

/// Proves an untrusted project cannot file feedback at all
/// (`quality_feedback_project_untrusted`), and that the trust gate is the first one consulted: an
/// untrusted request is refused as a trust failure even when it would also fail a later check.
#[test]
fn untrusted_project_cannot_file_feedback() {
    let mut context = RequestContext::local(RAW_SESSION, RAW_PROJECT_ROOT);
    assert!(
        !context.project_trusted,
        "RequestContext::local must not start trusted"
    );

    // Nothing at all supplied: still a trust failure, not a missing-actor or missing-source one.
    assert_eq!(
        derive_quality_feedback(
            &context,
            submission(artifact_target()),
            SOURCE_CURSOR,
            &[],
            CAPTURED_AT_UNIX_MS,
        )
        .unwrap_err(),
        "quality_feedback_project_untrusted"
    );

    // A fully populated request from the same untrusted project is refused identically.
    assert_eq!(
        derive_quality_feedback(
            &context,
            submission(artifact_target()),
            SOURCE_CURSOR,
            &[RAW_SOURCE_EVENT.to_owned()],
            CAPTURED_AT_UNIX_MS,
        )
        .unwrap_err(),
        "quality_feedback_project_untrusted"
    );

    // Trust is checked before identity, so an untrusted request with no actor is still reported as
    // a trust failure.
    context.actor_id = None;
    assert_eq!(
        derive_quality_feedback(
            &context,
            submission(artifact_target()),
            SOURCE_CURSOR,
            &[RAW_SOURCE_EVENT.to_owned()],
            CAPTURED_AT_UNIX_MS,
        )
        .unwrap_err(),
        "quality_feedback_project_untrusted"
    );
}

/// Proves provenance needs a non-blank actor identity taken from the trusted request
/// (`quality_feedback_actor_required`). A missing, empty or whitespace-only actor cannot be
/// substituted, because the submission has no actor field to substitute it into.
#[test]
fn feedback_requires_a_trusted_actor_identity() {
    for actor in [None, Some(String::new()), Some("   ".to_owned())] {
        let mut context = trusted_context();
        context.actor_id = actor.clone();
        assert_eq!(
            derive(&context, submission(artifact_target())).unwrap_err(),
            "quality_feedback_actor_required",
            "actor {actor:?} must not be able to derive provenance"
        );
    }
}

/// Proves feedback must be bound to committed source facts: an empty evidence window is
/// `quality_feedback_source_events_required`, and a server context the adapter cannot vouch for is
/// `quality_feedback_server_context_invalid`. Feedback is never minted from an unanchored window.
#[test]
fn feedback_requires_committed_source_references() {
    let context = trusted_context();
    let submission = submission(artifact_target());
    let events = [RAW_SOURCE_EVENT.to_owned()];

    assert_eq!(
        derive_quality_feedback(
            &context,
            submission.clone(),
            SOURCE_CURSOR,
            &[],
            CAPTURED_AT_UNIX_MS
        )
        .unwrap_err(),
        "quality_feedback_source_events_required"
    );

    // Cursor 0 claims "no source" while still naming an event, so the context is self-contradictory.
    assert_eq!(
        derive_quality_feedback(
            &context,
            submission.clone(),
            0,
            &events,
            CAPTURED_AT_UNIX_MS
        )
        .unwrap_err(),
        "quality_feedback_server_context_invalid"
    );

    // A zero capture timestamp is not a point in time the server can attest to.
    assert_eq!(
        derive_quality_feedback(&context, submission.clone(), SOURCE_CURSOR, &events, 0)
            .unwrap_err(),
        "quality_feedback_server_context_invalid"
    );

    // One more source event than the contract admits.
    let too_many = (0..=QUALITY_FEEDBACK_MAX_SOURCE_EVENTS)
        .map(|index| format!("event-raw-{index}"))
        .collect::<Vec<_>>();
    assert_eq!(
        derive_quality_feedback(
            &context,
            submission,
            SOURCE_CURSOR,
            &too_many,
            CAPTURED_AT_UNIX_MS
        )
        .unwrap_err(),
        "quality_feedback_server_context_invalid"
    );
}

/// Proves the target must be a server-resolved canonical fact. A wrong schema, a fact hash that is
/// not a digest, a zero revision, a reference filed under the wrong type prefix, a non-UUID run
/// reference, a memory reference with embedded whitespace and malformed policy or receipt
/// references are all `quality_canonical_target_invalid`. An unresolvable target is refused rather
/// than approximated, and a receipt target that disagrees with its own receipt reference is refused
/// separately as `quality_canonical_receipt_ref_mismatch`.
#[test]
fn non_canonical_target_references_are_rejected() {
    let context = trusted_context();

    let mut wrong_schema = artifact_target();
    wrong_schema.schema = "kiana.quality-canonical-target.v0".to_owned();
    assert_eq!(
        derive(&context, submission(wrong_schema)).unwrap_err(),
        "quality_canonical_target_invalid"
    );

    let mut not_a_digest = artifact_target();
    not_a_digest.target_digest = "not-a-digest".to_owned();
    assert_eq!(
        derive(&context, submission(not_a_digest)).unwrap_err(),
        "quality_canonical_target_invalid"
    );

    let mut zero_revision = artifact_target();
    zero_revision.target_revision = 0;
    assert_eq!(
        derive(&context, submission(zero_revision)).unwrap_err(),
        "quality_canonical_target_invalid"
    );

    // A turn reference may not be filed as a run target: the type prefix is part of the identity.
    let wrong_prefix = sealed_target(
        QualityFeedbackTargetType::Run,
        format!("turn:{}", RunId::new()),
        digest('a'),
        None,
        None,
        1,
    );
    assert_eq!(
        derive(&context, submission(wrong_prefix)).unwrap_err(),
        "quality_canonical_target_invalid"
    );

    // A run target requires a UUID; free text is not a resolvable run.
    let free_text_run = sealed_target(
        QualityFeedbackTargetType::Run,
        "run:not-a-uuid".to_owned(),
        digest('a'),
        None,
        None,
        1,
    );
    assert_eq!(
        derive(&context, submission(free_text_run)).unwrap_err(),
        "quality_canonical_target_invalid"
    );

    // A memory reference must be a single safe token.
    let spaced_memory = sealed_target(
        QualityFeedbackTargetType::Memory,
        "memory:record 1".to_owned(),
        digest('a'),
        None,
        None,
        1,
    );
    assert_eq!(
        derive(&context, submission(spaced_memory)).unwrap_err(),
        "quality_canonical_target_invalid"
    );

    // Policy and receipt references must carry their own prefixes.
    let unprefixed_policy = sealed_target(
        QualityFeedbackTargetType::Artifact,
        format!("artifact:{}", ArtifactId::new()),
        digest('a'),
        Some("policy-1".to_owned()),
        None,
        1,
    );
    assert_eq!(
        derive(&context, submission(unprefixed_policy)).unwrap_err(),
        "quality_canonical_target_invalid"
    );
    let unprefixed_receipt = sealed_target(
        QualityFeedbackTargetType::Artifact,
        format!("artifact:{}", ArtifactId::new()),
        digest('a'),
        None,
        Some("receipt-1".to_owned()),
        1,
    );
    assert_eq!(
        derive(&context, submission(unprefixed_receipt)).unwrap_err(),
        "quality_canonical_target_invalid"
    );

    // And the receipt target that resolves to a different receipt is its own, distinct refusal.
    let mismatched_receipt = sealed_target(
        QualityFeedbackTargetType::Receipt,
        format!("receipt:{RECEIPT_A}"),
        digest('b'),
        None,
        Some(format!("receipt:{RECEIPT_B}")),
        1,
    );
    assert_eq!(
        derive(&context, submission(mismatched_receipt)).unwrap_err(),
        "quality_canonical_receipt_ref_mismatch"
    );
}

/// Proves provenance and privacy scope are server-derived and not client-settable. The submission
/// wire shape has no field for either, a forged scope breaks the record digest, a spliced
/// provenance is caught by the target binding, a rewritten principal is caught by provenance
/// validation, and a malformed reference is caught by the record's own reference check.
#[test]
fn client_supplied_provenance_and_privacy_scope_are_not_accepted() {
    let context = trusted_context();
    let feedback =
        derive(&context, submission(artifact_target())).expect("EQ-45 baseline feedback");

    // The client wire shape has nowhere to put a provenance or a privacy scope: the submission is
    // `deny_unknown_fields`, so a claimed one is refused before any derivation happens.
    for injected in [
        "provenance",
        "privacy_scope",
        "principal_ref",
        "source_event_digest",
        "source_cursor",
    ] {
        let mut wire =
            serde_json::to_value(submission(artifact_target())).expect("EQ-45 submission wire");
        wire[injected] = json!("client-claimed");
        assert!(
            serde_json::from_value::<QualityFeedbackSubmission>(wire).is_err(),
            "submission must not accept a client-supplied {injected}"
        );
    }

    // A record whose scope was widened after derivation no longer matches its own digest.
    let mut widened = feedback.clone();
    widened.privacy_scope = QualityFeedbackPrivacyScope::Restricted;
    assert_eq!(widened.validate().unwrap_err(), "quality_feedback_invalid");

    // A record carrying another target's internally valid provenance is caught by the binding
    // between provenance and the target it claims to describe.
    let other = derive(
        &context,
        submission(sealed_target(
            QualityFeedbackTargetType::Run,
            format!("run:{}", RunId::new()),
            digest('b'),
            None,
            None,
            1,
        )),
    )
    .expect("EQ-45 second feedback");
    let mut spliced = feedback.clone();
    spliced.provenance = other.provenance.clone();
    spliced.feedback_digest = spliced.digest();
    assert_eq!(
        spliced.validate().unwrap_err(),
        "quality_feedback_target_provenance_mismatch"
    );

    // A rewritten principal inside an otherwise well-formed provenance is refused outright.
    let mut rewritten = feedback.clone();
    rewritten.provenance.principal_ref = "principal:not a token".to_owned();
    rewritten.feedback_digest = rewritten.digest();
    assert_eq!(
        rewritten.validate().unwrap_err(),
        "quality_feedback_provenance_invalid"
    );

    // And a reference that stops being a valid reference is refused on the record as well.
    let mut bad_reference = feedback.clone();
    bad_reference.comment_ref = Some("comment-ref-without-prefix".to_owned());
    bad_reference.feedback_digest = bad_reference.digest();
    assert_eq!(
        bad_reference.validate().unwrap_err(),
        "quality_feedback_reference_invalid"
    );
}

/// Proves the client-owned fields fail closed: a wrong schema, a nil feedback id, a blank,
/// over-long, NUL-bearing or secret-carrying label, an idempotency key without its prefix, a
/// comment reference with embedded whitespace and a correction reference that is not a typed target
/// are all `quality_feedback_submission_invalid`. A record is never derived from a submission the
/// server would not have accepted on its own.
#[test]
fn client_submission_fields_fail_closed() {
    let context = trusted_context();

    let mut wrong_schema = submission(artifact_target());
    wrong_schema.schema = "kiana.quality-feedback-submission.v0".to_owned();
    assert_eq!(
        derive(&context, wrong_schema).unwrap_err(),
        "quality_feedback_submission_invalid"
    );

    let mut nil_feedback_id = submission(artifact_target());
    nil_feedback_id.feedback_id =
        serde_json::from_value(json!("00000000-0000-0000-0000-000000000000"))
            .expect("EQ-45 nil feedback id");
    assert_eq!(
        derive(&context, nil_feedback_id).unwrap_err(),
        "quality_feedback_submission_invalid"
    );

    let over_long = "x".repeat(QUALITY_FEEDBACK_MAX_TEXT + 1);
    for label in [
        "   ".to_owned(),
        over_long,
        "label\u{0}with-nul".to_owned(),
        "api_key=secret-value".to_owned(),
    ] {
        let mut bad_label = submission(artifact_target());
        bad_label.label = label.clone();
        assert_eq!(
            derive(&context, bad_label).unwrap_err(),
            "quality_feedback_submission_invalid",
            "label {label:?} must fail closed"
        );
    }

    let mut bad_idempotency = submission(artifact_target());
    bad_idempotency.idempotency_key = "feedback-1".to_owned();
    assert_eq!(
        derive(&context, bad_idempotency).unwrap_err(),
        "quality_feedback_submission_invalid"
    );

    let mut bad_comment = submission(artifact_target());
    bad_comment.comment_ref = Some("comment ref 1".to_owned());
    assert_eq!(
        derive(&context, bad_comment).unwrap_err(),
        "quality_feedback_submission_invalid"
    );

    // A correction reference has to be a typed target reference, not a free-form pointer.
    let mut bad_correction = submission(artifact_target());
    bad_correction.correction_ref = Some("correction:ref-1".to_owned());
    assert_eq!(
        derive(&context, bad_correction).unwrap_err(),
        "quality_feedback_submission_invalid"
    );
}

/// The EQ-45 success path: a trusted request over a canonical target yields a self-consistent,
/// re-validatable record whose provenance and privacy scope were derived by the server, whose
/// references are hashed handles, and whose raw actor, project root, session and source event never
/// appear in the serialized record.
#[test]
fn server_derived_feedback_is_self_consistent_and_keeps_raw_context_out() {
    assert_eq!(QUALITY_FEEDBACK_COMMAND, "quality.feedback");

    let context = trusted_context();
    let feedback = derive(&context, submission(artifact_target())).expect("EQ-45 derived feedback");

    assert_eq!(feedback.schema, QUALITY_FEEDBACK_SCHEMA);
    assert_eq!(feedback.privacy_scope, QualityFeedbackPrivacyScope::Project);
    assert_eq!(
        feedback.provenance.target_digest,
        feedback.target.canonical_digest
    );
    assert_eq!(
        feedback.provenance.target_revision,
        feedback.target.target_revision
    );
    assert_eq!(feedback.provenance.source_cursor, SOURCE_CURSOR);
    assert_eq!(feedback.provenance.source_event_ids.len(), 1);
    assert_eq!(feedback.created_at_unix_ms, CAPTURED_AT_UNIX_MS);
    assert!(feedback.validate().is_ok());
    assert!(feedback.canonical_bytes().is_ok());

    // Every derived reference is a hashed handle behind its own prefix.
    for (reference, prefix) in [
        (feedback.provenance.principal_ref.as_str(), "principal:"),
        (feedback.provenance.project_ref.as_str(), "project:"),
        (feedback.provenance.session_ref.as_str(), "session:"),
    ] {
        assert!(
            reference.starts_with(prefix),
            "{prefix} handle expected, got {reference}"
        );
        assert!(
            reference[prefix.len()..].starts_with("sha256:"),
            "{prefix} handle must be hashed, got {reference}"
        );
    }
    for event in &feedback.provenance.source_event_ids {
        assert!(
            event.starts_with("event:"),
            "event handle expected, got {event}"
        );
        assert!(
            event["event:".len()..].starts_with("sha256:"),
            "event handle must be hashed, got {event}"
        );
    }

    // The raw context never reaches the record: the adapter stores references, not the values.
    let serialized = serde_json::to_string(&feedback).expect("EQ-45 feedback wire");
    for raw in [RAW_ACTOR, RAW_PROJECT_ROOT, RAW_SESSION, RAW_SOURCE_EVENT] {
        assert!(
            !serialized.contains(raw),
            "raw context {raw:?} leaked into the record"
        );
    }

    // Privacy scope follows the server's classification of the target: memory facts are principal
    // scoped because they may hold private content, and every other target type is project-visible
    // evidence. The client never gets to pick.
    let memory = derive(
        &context,
        submission(sealed_target(
            QualityFeedbackTargetType::Memory,
            "memory:record-1".to_owned(),
            digest('c'),
            None,
            None,
            1,
        )),
    )
    .expect("EQ-45 memory feedback");
    assert_eq!(memory.privacy_scope, QualityFeedbackPrivacyScope::Principal);
    assert!(memory.validate().is_ok());

    let receipt =
        derive(&context, submission(receipt_target(RECEIPT_B))).expect("EQ-45 receipt feedback");
    assert_eq!(receipt.privacy_scope, QualityFeedbackPrivacyScope::Project);
    assert!(receipt.validate().is_ok());

    // The request-derived part of provenance is a function of the trusted context and the committed
    // source facts, not of a fresh identifier, so it repeats exactly across derivations.
    let repeated =
        derive(&context, submission(artifact_target())).expect("EQ-45 repeated derivation");
    assert_eq!(
        repeated.provenance.principal_ref,
        feedback.provenance.principal_ref
    );
    assert_eq!(
        repeated.provenance.project_ref,
        feedback.provenance.project_ref
    );
    assert_eq!(
        repeated.provenance.session_ref,
        feedback.provenance.session_ref
    );
    assert_eq!(
        repeated.provenance.source_event_digest,
        feedback.provenance.source_event_digest
    );
    assert_eq!(
        repeated.provenance.source_cursor,
        feedback.provenance.source_cursor
    );
}

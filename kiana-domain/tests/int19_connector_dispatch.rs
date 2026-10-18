use kiana_domain::{json_digest, ConnectorDispatchLifecycle, ConnectorDispatchStage, InvocationId};
use serde_json::{json, Value};

fn digest(label: &str) -> String {
    json_digest(&json!({"label": label}))
}

fn prepared() -> ConnectorDispatchLifecycle {
    ConnectorDispatchLifecycle::prepared(
        InvocationId::new(),
        1,
        digest("command"),
        digest("binding"),
        digest("payload"),
        digest("idempotency"),
    )
    .expect("prepared lifecycle")
}

#[test]
fn lifecycle_requires_prepared_dispatch_observe_commit_order() {
    let prepared = prepared();
    let dispatching = prepared
        .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
        .expect("dispatching");
    let observed = dispatching
        .advance(
            ConnectorDispatchStage::Observed,
            Some(digest("receipt")),
            Some(digest("observation")),
            None,
            None,
        )
        .expect("observed");
    let committed = observed
        .advance(
            ConnectorDispatchStage::ResultCommitted,
            observed.receipt_digest.clone(),
            observed.observation_digest.clone(),
            Some(digest("result")),
            None,
        )
        .expect("committed");

    assert_eq!(committed.stage, ConnectorDispatchStage::ResultCommitted);
    assert!(committed.stage.is_terminal());
    assert_eq!(committed.validate(), Ok(()));
    assert_eq!(
        committed
            .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
            .unwrap_err(),
        "connector_dispatch_terminal_resurrection"
    );
}

#[test]
fn unknown_is_terminal_and_may_keep_only_observation_evidence() {
    let dispatching = prepared()
        .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
        .expect("dispatching");
    let unknown = dispatching
        .advance(
            ConnectorDispatchStage::Unknown,
            None,
            None,
            None,
            Some("adapter_timeout".to_owned()),
        )
        .expect("unknown");
    assert_eq!(unknown.stage, ConnectorDispatchStage::Unknown);
    assert_eq!(unknown.validate(), Ok(()));
    assert_eq!(
        unknown
            .advance(
                ConnectorDispatchStage::ResultCommitted,
                Some(digest("receipt")),
                Some(digest("observation")),
                Some(digest("result")),
                None,
            )
            .unwrap_err(),
        "connector_dispatch_terminal_resurrection"
    );

    let observed = prepared()
        .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
        .expect("dispatching")
        .advance(
            ConnectorDispatchStage::Observed,
            Some(digest("receipt")),
            Some(digest("observation")),
            None,
            None,
        )
        .expect("observed");
    let unknown_with_evidence = observed
        .advance(
            ConnectorDispatchStage::Unknown,
            observed.receipt_digest.clone(),
            observed.observation_digest.clone(),
            None,
            Some("provider_outcome_unknown".to_owned()),
        )
        .expect("unknown with evidence");
    assert_eq!(unknown_with_evidence.stage, ConnectorDispatchStage::Unknown);
    assert_eq!(unknown_with_evidence.validate(), Ok(()));
}

#[test]
fn malformed_phase_artifacts_and_unknown_fields_fail_closed() {
    let prepared = prepared();
    let dispatching = prepared
        .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
        .expect("dispatching");
    assert_eq!(
        dispatching
            .advance(
                ConnectorDispatchStage::Observed,
                Some(digest("receipt")),
                None,
                None,
                None,
            )
            .unwrap_err(),
        "connector_dispatch_observation_incomplete"
    );

    let observed = dispatching
        .advance(
            ConnectorDispatchStage::Observed,
            Some(digest("receipt")),
            Some(digest("observation")),
            None,
            None,
        )
        .expect("observed");
    assert_eq!(
        observed
            .advance(
                ConnectorDispatchStage::ResultCommitted,
                Some(digest("different_receipt")),
                observed.observation_digest.clone(),
                Some(digest("result")),
                None,
            )
            .unwrap_err(),
        "connector_dispatch_observation_changed"
    );

    let mut value = serde_json::to_value(prepared).expect("encode");
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ConnectorDispatchLifecycle>(value).is_err());
}

fn dispatching() -> ConnectorDispatchLifecycle {
    prepared()
        .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
        .expect("dispatching")
}

fn observed() -> ConnectorDispatchLifecycle {
    dispatching()
        .advance(
            ConnectorDispatchStage::Observed,
            Some(digest("receipt")),
            Some(digest("observation")),
            None,
            None,
        )
        .expect("observed")
}

fn committed() -> ConnectorDispatchLifecycle {
    let observed = observed();
    observed
        .advance(
            ConnectorDispatchStage::ResultCommitted,
            observed.receipt_digest.clone(),
            observed.observation_digest.clone(),
            Some(digest("result")),
            None,
        )
        .expect("committed")
}

fn unknown() -> ConnectorDispatchLifecycle {
    dispatching()
        .advance(
            ConnectorDispatchStage::Unknown,
            None,
            None,
            None,
            Some("adapter_timeout".to_owned()),
        )
        .expect("unknown")
}

/// A phase cannot be skipped and a phase cannot be entered with the evidence of a later phase.
/// Every refusal happens on the value model alone: no adapter, journal or authorization is
/// reachable from this module.
#[test]
fn skipped_phases_and_premature_artifacts_are_refused_before_any_dispatch() {
    // Prepared can only become Dispatching or Unknown; it can never jump to an observed or
    // committed result, with or without evidence attached.
    for (next, receipt, observation, result, reason) in [
        (
            ConnectorDispatchStage::Observed,
            Some(digest("receipt")),
            Some(digest("observation")),
            None,
            None,
        ),
        (
            ConnectorDispatchStage::ResultCommitted,
            Some(digest("receipt")),
            Some(digest("observation")),
            Some(digest("result")),
            None,
        ),
        (
            ConnectorDispatchStage::ResultCommitted,
            None,
            None,
            None,
            None,
        ),
    ] {
        assert_eq!(
            prepared()
                .advance(next, receipt, observation, result, reason)
                .unwrap_err(),
            "connector_dispatch_transition_invalid"
        );
    }

    // Dispatching cannot become ResultCommitted without an observation fact.
    let dispatching = dispatching();
    assert_eq!(
        dispatching
            .advance(
                ConnectorDispatchStage::ResultCommitted,
                Some(digest("receipt")),
                Some(digest("observation")),
                Some(digest("result")),
                None,
            )
            .unwrap_err(),
        "connector_dispatch_transition_invalid"
    );

    // The two pre-effect phases must not carry any effect artifact.
    assert_eq!(
        dispatching
            .advance(
                ConnectorDispatchStage::Dispatching,
                Some(digest("receipt")),
                None,
                None,
                None,
            )
            .unwrap_err(),
        "connector_dispatch_pre_effect_artifacts_forbidden"
    );

    // A committed result cannot be produced from an observed phase that dropped its evidence.
    assert_eq!(
        dispatching
            .advance(
                ConnectorDispatchStage::Observed,
                Some(digest("receipt")),
                None,
                None,
                None,
            )
            .unwrap_err(),
        "connector_dispatch_observation_incomplete"
    );
    assert_eq!(
        observed()
            .advance(
                ConnectorDispatchStage::ResultCommitted,
                observed().receipt_digest.clone(),
                observed().observation_digest.clone(),
                None,
                None,
            )
            .unwrap_err(),
        "connector_dispatch_result_commit_incomplete"
    );

    // The same invariants hold when a caller hands in a hand-built (deserialized) value instead
    // of walking the transition function.
    let mut pre_effect = serde_json::to_value(dispatching()).expect("encode");
    pre_effect["receipt_digest"] = json!(digest("receipt"));
    let pre_effect: ConnectorDispatchLifecycle =
        serde_json::from_value(pre_effect).expect("deserialize");
    assert_eq!(
        pre_effect.validate().unwrap_err(),
        "connector_dispatch_pre_effect_artifacts_forbidden"
    );

    let mut incomplete = serde_json::to_value(observed()).expect("encode");
    incomplete["observation_digest"] = Value::Null;
    let incomplete: ConnectorDispatchLifecycle =
        serde_json::from_value(incomplete).expect("deserialize");
    assert_eq!(
        incomplete.validate().unwrap_err(),
        "connector_dispatch_observation_incomplete"
    );

    let mut missing_result = serde_json::to_value(committed()).expect("encode");
    missing_result["result_digest"] = Value::Null;
    let missing_result: ConnectorDispatchLifecycle =
        serde_json::from_value(missing_result).expect("deserialize");
    assert_eq!(
        missing_result.validate().unwrap_err(),
        "connector_dispatch_result_commit_incomplete"
    );
}

/// ResultCommitted and Unknown are terminal. A late provider result, a replayed worker or a
/// deserialized clone cannot move them, and an unknown phase cannot smuggle a result digest.
#[test]
fn late_results_cannot_resurrect_a_terminal_attempt() {
    let committed = committed();
    for next in [
        ConnectorDispatchStage::Prepared,
        ConnectorDispatchStage::Dispatching,
        ConnectorDispatchStage::Observed,
        ConnectorDispatchStage::Unknown,
    ] {
        assert_eq!(
            committed
                .advance(
                    next,
                    committed.receipt_digest.clone(),
                    committed.observation_digest.clone(),
                    committed.result_digest.clone(),
                    Some("late_result".to_owned()),
                )
                .unwrap_err(),
            "connector_dispatch_terminal_resurrection"
        );
    }

    let unknown = unknown();
    for next in [
        ConnectorDispatchStage::Prepared,
        ConnectorDispatchStage::Dispatching,
        ConnectorDispatchStage::Observed,
        ConnectorDispatchStage::ResultCommitted,
    ] {
        assert_eq!(
            unknown
                .advance(
                    next,
                    Some(digest("late_receipt")),
                    Some(digest("late_observation")),
                    Some(digest("late_result")),
                    Some("late_result".to_owned()),
                )
                .unwrap_err(),
            "connector_dispatch_terminal_resurrection"
        );
    }

    // A clone/replay that went through serde is still the same terminal fact.
    let replayed: ConnectorDispatchLifecycle =
        serde_json::from_value(serde_json::to_value(committed()).expect("encode"))
            .expect("deserialize");
    assert_eq!(replayed.validate(), Ok(()));
    assert_eq!(
        replayed
            .advance(
                ConnectorDispatchStage::ResultCommitted,
                None,
                None,
                None,
                None
            )
            .unwrap_err(),
        "connector_dispatch_terminal_resurrection"
    );

    // An Unknown phase may keep the observation evidence it already had, but never a result.
    let observed = observed();
    let unknown_with_evidence = observed
        .advance(
            ConnectorDispatchStage::Unknown,
            observed.receipt_digest.clone(),
            observed.observation_digest.clone(),
            None,
            Some("provider_outcome_unknown".to_owned()),
        )
        .expect("unknown with observation evidence");
    assert_eq!(unknown_with_evidence.validate(), Ok(()));

    let mut forged = serde_json::to_value(unknown_with_evidence).expect("encode");
    forged["result_digest"] = json!(digest("late_result"));
    let forged: ConnectorDispatchLifecycle = serde_json::from_value(forged).expect("deserialize");
    assert_eq!(
        forged.validate().unwrap_err(),
        "connector_dispatch_unknown_evidence_invalid"
    );

    let mut half = serde_json::to_value(unknown()).expect("encode");
    half["receipt_digest"] = json!(digest("late_receipt"));
    let half: ConnectorDispatchLifecycle = serde_json::from_value(half).expect("deserialize");
    assert_eq!(
        half.validate().unwrap_err(),
        "connector_dispatch_unknown_evidence_invalid"
    );
}

/// Every header field, evidence digest, reason and the self-certifying lifecycle digest fail
/// closed on forged input; unknown fields are rejected by the strict decoder.
#[test]
fn forged_lifecycle_headers_digests_and_reasons_fail_closed() {
    for (field, forged, expected) in [
        (
            "schema",
            json!("kiana.connector-dispatch-lifecycle.v2"),
            "connector_dispatch_lifecycle_header_invalid",
        ),
        (
            "invocation_id",
            json!("00000000-0000-0000-0000-000000000000"),
            "connector_dispatch_lifecycle_header_invalid",
        ),
        (
            "attempt",
            json!(0),
            "connector_dispatch_lifecycle_header_invalid",
        ),
        (
            "command_digest",
            json!("not-a-digest"),
            "connector_dispatch_lifecycle_header_invalid",
        ),
        (
            "binding_digest",
            json!("sha256:zz"),
            "connector_dispatch_lifecycle_header_invalid",
        ),
        (
            "payload_digest",
            json!(""),
            "connector_dispatch_lifecycle_header_invalid",
        ),
        (
            "idempotency_key_digest",
            json!(""),
            "connector_dispatch_lifecycle_header_invalid",
        ),
    ] {
        let mut candidate = serde_json::to_value(prepared()).expect("encode");
        candidate[field] = forged;
        let candidate: ConnectorDispatchLifecycle =
            serde_json::from_value(candidate).expect("deserialize");
        assert_eq!(candidate.validate().unwrap_err(), expected, "field {field}");
    }

    for (field, forged, expected) in [
        (
            "receipt_digest",
            json!("sha256:not-hex"),
            "connector_dispatch_receipt_digest_invalid",
        ),
        (
            "observation_digest",
            json!("sha256:not-hex"),
            "connector_dispatch_observation_digest_invalid",
        ),
        (
            "result_digest",
            json!("sha256:not-hex"),
            "connector_dispatch_result_digest_invalid",
        ),
    ] {
        let mut candidate = serde_json::to_value(observed()).expect("encode");
        candidate[field] = forged;
        let candidate: ConnectorDispatchLifecycle =
            serde_json::from_value(candidate).expect("deserialize");
        assert_eq!(candidate.validate().unwrap_err(), expected, "field {field}");
    }

    for reason in [
        json!(""),
        json!("   "),
        json!("line\nbreak"),
        json!("carriage\rreturn"),
        json!(format!("x{}", "y".repeat(256))),
    ] {
        let mut value = serde_json::to_value(unknown()).expect("encode");
        value["reason"] = reason;
        let candidate: ConnectorDispatchLifecycle =
            serde_json::from_value(value).expect("deserialize");
        assert_eq!(
            candidate.validate().unwrap_err(),
            "connector_dispatch_reason_invalid"
        );
    }

    // Mutating any covered field without re-deriving the digest is detected, and the constructor
    // refuses malformed inputs outright.
    let mut value = serde_json::to_value(committed()).expect("encode");
    value["command_digest"] = json!(digest("forged_command"));
    let candidate: ConnectorDispatchLifecycle = serde_json::from_value(value).expect("deserialize");
    assert_eq!(
        candidate.validate().unwrap_err(),
        "connector_dispatch_lifecycle_digest_mismatch"
    );

    for (command, binding, payload, idempotency) in [
        (
            "not-a-digest".to_owned(),
            digest("binding"),
            digest("payload"),
            digest("idempotency"),
        ),
        (
            digest("command"),
            "sha256:zz".to_owned(),
            digest("payload"),
            digest("idempotency"),
        ),
        (
            digest("command"),
            digest("binding"),
            String::new(),
            digest("idempotency"),
        ),
        (
            digest("command"),
            digest("binding"),
            digest("payload"),
            "sha256:".to_owned(),
        ),
    ] {
        assert_eq!(
            ConnectorDispatchLifecycle::prepared(
                InvocationId::new(),
                1,
                command,
                binding,
                payload,
                idempotency,
            )
            .unwrap_err(),
            "connector_dispatch_lifecycle_header_invalid"
        );
    }

    let mut value = serde_json::to_value(prepared()).expect("encode");
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ConnectorDispatchLifecycle>(value).is_err());
}

/// The canonical digest covers the immutable identity, the phase and every evidence field, so
/// two attempts with different facts can never share a lifecycle identity.
#[test]
fn lifecycle_digest_covers_every_identity_phase_and_evidence_field() {
    let committed = committed();
    let baseline = committed.digest();
    assert_eq!(committed.lifecycle_digest, baseline);

    let mutations: [(&str, Value); 11] = [
        ("invocation_id", json!(InvocationId::new())),
        ("attempt", json!(2)),
        ("command_digest", json!(digest("other_command"))),
        ("binding_digest", json!(digest("other_binding"))),
        ("payload_digest", json!(digest("other_payload"))),
        ("idempotency_key_digest", json!(digest("other_key"))),
        ("stage", json!("unknown")),
        ("receipt_digest", json!(digest("other_receipt"))),
        ("observation_digest", json!(digest("other_observation"))),
        ("result_digest", json!(digest("other_result"))),
        ("reason", json!("some_reason")),
    ];
    for (field, value) in mutations {
        let mut candidate = serde_json::to_value(&committed).expect("encode");
        candidate[field] = value;
        let candidate: ConnectorDispatchLifecycle =
            serde_json::from_value(candidate).expect("deserialize");
        assert_ne!(candidate.digest(), baseline, "digest must cover {field}");
    }

    // Serialization is stable: a round-trip reproduces the same digest and validates.
    let replayed: ConnectorDispatchLifecycle =
        serde_json::from_value(serde_json::to_value(&committed).expect("encode"))
            .expect("deserialize");
    assert_eq!(replayed, committed);
    assert_eq!(replayed.digest(), baseline);
    assert_eq!(replayed.validate(), Ok(()));
}

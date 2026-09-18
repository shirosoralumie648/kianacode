use kiana_domain::{
    AttemptId, BillingUnknownReason, NormalizedUsage, UsageConfidence, UsageObservation,
    UsagePresence, UsageSource, UsageVector,
};

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

#[test]
fn usage_vector_distinguishes_absent_zero_and_present() {
    let mut vector = UsageVector::zero();
    vector.input_tokens = None;
    vector.output_tokens = Some(0);
    vector.cache_read_tokens = Some(12);
    vector.validate().unwrap();
    assert_eq!(vector.input_presence(), UsagePresence::Absent);
    assert_eq!(vector.output_presence(), UsagePresence::ExplicitZero);
    assert_eq!(
        UsagePresence::of(vector.cache_read_tokens),
        UsagePresence::Present
    );
}

#[test]
fn normalized_usage_binds_source_basis_sequence_and_unknown_reason() {
    let usage = NormalizedUsage::new(
        kiana_domain::UsageId::new(),
        AttemptId::new(),
        kiana_domain::RunId::new(),
        Some("provider:test".to_owned()),
        "model:test",
        "route:v1",
        UsageVector::zero(),
        UsageSource::Provider,
        UsageObservation::Final,
        Some(2),
        UsageConfidence::Known,
        None,
        "provider_reported_final",
        digest('a'),
    )
    .unwrap();
    usage.validate().unwrap();

    let partial = NormalizedUsage::new(
        kiana_domain::UsageId::new(),
        AttemptId::new(),
        kiana_domain::RunId::new(),
        None,
        "local-model",
        "route:local",
        UsageVector::zero(),
        UsageSource::LocalExecutor,
        UsageObservation::Delta,
        Some(1),
        UsageConfidence::Partial,
        Some(BillingUnknownReason::ProviderUnreported),
        "local_partial",
        digest('b'),
    )
    .unwrap();
    assert_eq!(partial.sequence, Some(1));
    assert_eq!(
        partial.unknown_reason,
        Some(BillingUnknownReason::ProviderUnreported)
    );
}

#[test]
fn normalized_usage_rejects_missing_sequence_reason_and_digest_drift() {
    let missing_sequence = NormalizedUsage::new(
        kiana_domain::UsageId::new(),
        AttemptId::new(),
        kiana_domain::RunId::new(),
        None,
        "model",
        "route",
        UsageVector::zero(),
        UsageSource::Derived,
        UsageObservation::Delta,
        None,
        UsageConfidence::Partial,
        Some(BillingUnknownReason::Partial),
        "derived",
        digest('c'),
    );
    assert_eq!(
        missing_sequence.unwrap_err(),
        "normalized_usage_delta_sequence_required"
    );

    let missing_reason = NormalizedUsage::new(
        kiana_domain::UsageId::new(),
        AttemptId::new(),
        kiana_domain::RunId::new(),
        None,
        "model",
        "route",
        UsageVector::zero(),
        UsageSource::Derived,
        UsageObservation::Final,
        None,
        UsageConfidence::Unknown,
        None,
        "unknown",
        digest('d'),
    );
    assert_eq!(
        missing_reason.unwrap_err(),
        "normalized_usage_unknown_reason_required"
    );

    let mut tampered = NormalizedUsage::new(
        kiana_domain::UsageId::new(),
        AttemptId::new(),
        kiana_domain::RunId::new(),
        None,
        "model",
        "route",
        UsageVector::zero(),
        UsageSource::Derived,
        UsageObservation::Final,
        None,
        UsageConfidence::Known,
        None,
        "known",
        digest('e'),
    )
    .unwrap();
    tampered.vector.tool_calls = 1;
    assert_eq!(
        tampered.validate().unwrap_err(),
        "normalized_usage_header_invalid"
    );
}

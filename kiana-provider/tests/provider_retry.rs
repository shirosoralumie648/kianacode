use kiana_domain::{
    ModelProtocol, ProviderFaultCase, ProviderFaultCorpus, ProviderFaultDisposition,
    ProviderFaultKind,
};

fn fault_case(
    case_id: &str,
    fault: ProviderFaultKind,
    disposition: ProviderFaultDisposition,
    expected_error: Option<&str>,
    expected_attempts: u8,
) -> ProviderFaultCase {
    ProviderFaultCase::new(
        case_id,
        ModelProtocol::OpenAiChat,
        fault,
        29,
        disposition,
        expected_error.map(str::to_owned),
        expected_attempts,
        "p4-j7-29-openai-synthetic",
    )
    .expect("fault case")
}

fn corpus() -> ProviderFaultCorpus {
    ProviderFaultCorpus::new(
        "p4-j7-29-provider-faults",
        "p4-j7-29.v1",
        vec![
            fault_case(
                "arbitrary-chunking",
                ProviderFaultKind::ArbitraryChunking,
                ProviderFaultDisposition::PreserveOutput,
                None,
                1,
            ),
            fault_case(
                "out-of-order-event",
                ProviderFaultKind::OutOfOrderEvent,
                ProviderFaultDisposition::RejectBeforeSend,
                Some("provider_event_order_invalid"),
                0,
            ),
            fault_case(
                "duplicate-event-id",
                ProviderFaultKind::DuplicateEventId,
                ProviderFaultDisposition::RejectBeforeSend,
                Some("provider_duplicate_event_id"),
                0,
            ),
            fault_case(
                "truncated-frame",
                ProviderFaultKind::TruncatedFrame,
                ProviderFaultDisposition::RejectBeforeSend,
                Some("provider_frame_truncated"),
                0,
            ),
            fault_case(
                "oversized-frame",
                ProviderFaultKind::OversizedFrame,
                ProviderFaultDisposition::RejectBeforeSend,
                Some("provider_frame_limit"),
                0,
            ),
            fault_case(
                "cancelled",
                ProviderFaultKind::Cancellation,
                ProviderFaultDisposition::Cancelled,
                None,
                1,
            ),
            fault_case(
                "incomplete-tool-call",
                ProviderFaultKind::IncompleteToolCall,
                ProviderFaultDisposition::RejectBeforeSend,
                Some("provider_tool_json_invalid"),
                0,
            ),
            fault_case(
                "missing-usage",
                ProviderFaultKind::MissingUsage,
                ProviderFaultDisposition::UnknownNoRetry,
                Some("provider_usage_unknown"),
                1,
            ),
            fault_case(
                "unsupported-capability",
                ProviderFaultKind::UnsupportedCapability,
                ProviderFaultDisposition::RejectBeforeSend,
                Some("model_capability_unsupported"),
                0,
            ),
        ],
    )
    .expect("fault corpus")
}

#[test]
fn fault_corpus_covers_each_declared_failure_mode_and_stays_offline() {
    let corpus = corpus();
    corpus.validate().expect("valid fault corpus");
    assert_eq!(corpus.cases.len(), ProviderFaultKind::ALL.len());
    assert!(corpus
        .cases
        .iter()
        .all(|case| { !case.external_connection_allowed && case.expected_attempts <= 1 }));
    assert!(corpus
        .cases
        .iter()
        .any(|case| case.disposition == ProviderFaultDisposition::UnknownNoRetry));
    assert!(
        corpus
            .canonical_bytes()
            .expect("canonical fault corpus")
            .len()
            > 128
    );
}

#[test]
fn unknown_usage_is_not_a_success_and_never_adds_an_automatic_attempt() {
    let case = corpus()
        .cases
        .iter()
        .find(|case| case.fault == ProviderFaultKind::MissingUsage)
        .expect("missing usage case");
    assert_eq!(case.disposition, ProviderFaultDisposition::UnknownNoRetry);
    assert_eq!(case.expected_attempts, 1);
    assert!(case.expected_error.is_some());
}

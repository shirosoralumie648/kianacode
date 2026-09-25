use std::fs;

#[test]
fn offline_contracts_are_shared_and_have_no_effect_authority() {
    let contracts = fs::read_to_string("../kiana-domain/src/provider_contracts.rs")
        .expect("provider contracts source");
    for marker in [
        "PROVIDER_CONTRACT_MATRIX_SCHEMA",
        "ProviderContractCapability",
        "ProviderContractMatrix",
        "PROVIDER_CONTRACT_MATRIX_PROTOCOLS",
        "Self::Legacy",
        "provider_contract_matrix_coverage_incomplete",
        "ProviderCassetteMode",
        "provider_replay_external_connection_forbidden",
        "ProviderFaultKind",
        "ProviderFaultCorpus",
        "provider_fault_corpus_coverage_incomplete",
    ] {
        assert!(
            contracts.contains(marker),
            "provider contract marker missing: {marker}"
        );
    }
    for forbidden in [
        "reqwest::Client",
        "TcpStream",
        "TcpListener",
        "tokio::spawn",
        "ModelBudgetPort",
        "CapabilityBrokerPort",
        "EventStorePort",
        "ProviderGateway",
    ] {
        assert!(
            !contracts.contains(forbidden),
            "offline domain contracts gained effect authority: {forbidden}"
        );
    }
}

#[test]
fn fixture_replay_reuses_response_boundary_without_a_second_transport_path() {
    let response =
        fs::read_to_string("../kiana-provider/src/response.rs").expect("provider response source");
    for marker in [
        "pub fn replay_stream_fixture",
        "Accumulator::new",
        "framer.push",
        "provider_stream_incomplete",
    ] {
        assert!(
            response.contains(marker),
            "fixture replay marker missing: {marker}"
        );
    }
    for forbidden in [
        "reqwest::Client",
        "TcpStream",
        "TcpListener",
        "tokio::spawn",
        "ModelBudgetPort",
        "ProviderGateway",
        "complete_admitted",
    ] {
        assert!(
            !response.contains(forbidden),
            "fixture replay widened the transport/effect boundary: {forbidden}"
        );
    }
}

#[test]
fn provider_contract_fixtures_cannot_silently_open_live_connections() {
    for path in [
        "../kiana-provider/tests/provider_contract.rs",
        "../kiana-provider/tests/provider_streaming.rs",
        "../kiana-provider/tests/provider_retry.rs",
    ] {
        let source = fs::read_to_string(path).expect("provider contract fixture source");
        for forbidden in [
            "TcpStream",
            "TcpListener",
            "reqwest::Client",
            "complete_admitted",
        ] {
            assert!(
                !source.contains(forbidden),
                "offline fixture {path} contains live/effect marker: {forbidden}"
            );
        }
    }
    let cassette = fs::read_to_string("../kiana-provider/fixtures/provider/p4-j7-29-cassette.json")
        .expect("provider cassette fixture");
    assert!(cassette.contains("\"mode\": \"replay\""));
    assert!(cassette.contains("\"external_connection_allowed\": false"));
    assert!(!cassette.contains("http://") && !cassette.contains("https://"));
}

#[test]
fn provider_retry_client_disables_sdk_implicit_retries() {
    let config = fs::read_to_string("../kiana-provider/src/config.rs").expect("provider config");
    assert!(config.contains("retry(reqwest::retry::never())"));
}

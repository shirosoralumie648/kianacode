use kiana_domain::{
    ProviderProductChainEvidence, ProviderProductChainMatrix, ProviderProductChainMode,
    ProviderProductChainScenario, ProviderProductChainStage, ProviderProductChainStageEvidence,
    ProviderProductChainTerminal, ProviderProductSurface, ProviderProductSurfaceSnapshot,
};
use std::fs;

const RUN: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROUTE: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const RECEIPT: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const FILE_EFFECT: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const USAGE: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn stages(scenario: ProviderProductChainScenario) -> Vec<ProviderProductChainStageEvidence> {
    let stages = match scenario {
        ProviderProductChainScenario::CodingRoundTrip
        | ProviderProductChainScenario::SlowSubscriber
        | ProviderProductChainScenario::RunContinueResume => vec![
            ProviderProductChainStage::ModelRequest,
            ProviderProductChainStage::ModelReply,
            ProviderProductChainStage::CapabilityDispatch,
            ProviderProductChainStage::ToolResult,
            ProviderProductChainStage::RuntimeEvent,
            ProviderProductChainStage::Receipt,
        ],
        ProviderProductChainScenario::CancelBetweenModelFinishAndDispatch => vec![
            ProviderProductChainStage::ModelRequest,
            ProviderProductChainStage::ModelReply,
            ProviderProductChainStage::RuntimeEvent,
            ProviderProductChainStage::Receipt,
        ],
        ProviderProductChainScenario::BudgetDenied => vec![
            ProviderProductChainStage::RuntimeEvent,
            ProviderProductChainStage::Receipt,
        ],
    };
    stages
        .into_iter()
        .enumerate()
        .map(|(index, stage)| {
            ProviderProductChainStageEvidence::new(stage, index as u64 + 1, digest('f'))
                .expect("stage evidence")
        })
        .collect()
}

fn surfaces(
    terminal: ProviderProductChainTerminal,
    file_effect_digest: Option<&str>,
    usage_digest: Option<&str>,
    result_unknown: bool,
) -> Vec<ProviderProductSurfaceSnapshot> {
    ProviderProductSurface::ALL
        .into_iter()
        .map(|surface| {
            ProviderProductSurfaceSnapshot::new(
                surface,
                terminal,
                RECEIPT,
                file_effect_digest.map(str::to_owned),
                usage_digest.map(str::to_owned),
                7,
                result_unknown,
            )
            .expect("surface snapshot")
        })
        .collect()
}

fn case(scenario: ProviderProductChainScenario) -> ProviderProductChainEvidence {
    let (terminal, file_effect, usage, model_requests, dispatches, results, cancel, slow, late) =
        match scenario {
            ProviderProductChainScenario::CodingRoundTrip => (
                ProviderProductChainTerminal::Completed,
                Some(FILE_EFFECT.to_owned()),
                Some(USAGE.to_owned()),
                2,
                1,
                1,
                false,
                false,
                false,
            ),
            ProviderProductChainScenario::CancelBetweenModelFinishAndDispatch => (
                ProviderProductChainTerminal::Cancelled,
                None,
                None,
                1,
                0,
                0,
                true,
                false,
                true,
            ),
            ProviderProductChainScenario::SlowSubscriber => (
                ProviderProductChainTerminal::Completed,
                Some(FILE_EFFECT.to_owned()),
                Some(USAGE.to_owned()),
                2,
                1,
                1,
                false,
                true,
                true,
            ),
            ProviderProductChainScenario::RunContinueResume => (
                ProviderProductChainTerminal::Completed,
                Some(FILE_EFFECT.to_owned()),
                Some(USAGE.to_owned()),
                2,
                1,
                1,
                false,
                false,
                false,
            ),
            ProviderProductChainScenario::BudgetDenied => (
                ProviderProductChainTerminal::BudgetDenied,
                None,
                None,
                0,
                0,
                0,
                false,
                false,
                false,
            ),
        };
    ProviderProductChainEvidence::new(
        format!("case-{scenario:?}"),
        scenario,
        terminal,
        ProviderProductChainMode::LoopbackCassette,
        false,
        RUN,
        ROUTE,
        RECEIPT,
        file_effect.clone(),
        usage.clone(),
        model_requests,
        dispatches,
        results,
        1,
        stages(scenario),
        surfaces(terminal, file_effect.as_deref(), usage.as_deref(), false),
        cancel,
        slow,
        late,
        false,
    )
    .expect("product chain case")
}

#[test]
fn product_chain_matrix_covers_model_tool_event_receipt_and_four_surfaces() {
    let matrix = ProviderProductChainMatrix::new(
        RUN,
        ProviderProductChainScenario::ALL
            .into_iter()
            .map(case)
            .collect(),
    )
    .expect("product chain matrix");
    matrix.validate().expect("valid product chain matrix");
    assert_eq!(matrix.cases.len(), 5);
    assert!(matrix
        .case(ProviderProductChainScenario::CodingRoundTrip)
        .is_some());
    assert!(matrix.canonical_bytes().expect("canonical matrix").len() > 256);
}

#[test]
fn product_chain_rejects_surface_drift_and_cancelled_tool_dispatch() {
    let mut drift = case(ProviderProductChainScenario::CodingRoundTrip);
    drift.surfaces[0].receipt_digest = digest('1');
    assert_eq!(
        drift.validate().unwrap_err(),
        "provider_product_chain_surface_drift"
    );

    let mut cancelled = case(ProviderProductChainScenario::CancelBetweenModelFinishAndDispatch);
    cancelled.capability_dispatch_count = 1;
    assert_eq!(
        cancelled.validate().unwrap_err(),
        "provider_product_chain_cancel_fence_invalid"
    );
}

#[test]
fn slow_subscriber_requires_durable_terminal_and_late_increment_fence() {
    let mut slow = case(ProviderProductChainScenario::SlowSubscriber);
    slow.slow_subscriber_terminal_retained = false;
    assert_eq!(
        slow.validate().unwrap_err(),
        "provider_product_chain_slow_subscriber_terminal_lost"
    );

    let mut matrix_cases = ProviderProductChainScenario::ALL
        .into_iter()
        .map(case)
        .collect::<Vec<_>>();
    matrix_cases.pop();
    assert_eq!(
        ProviderProductChainMatrix::new(RUN, matrix_cases).unwrap_err(),
        "provider_product_chain_matrix_coverage_missing"
    );
}

#[test]
fn fixture_is_metadata_only_and_does_not_claim_physical_effect_proof() {
    let raw = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/p4-j7-30-product-chain.json"
    ))
    .expect("product chain fixture");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("fixture json");
    assert_eq!(value["schema"], "kiana.provider-product-chain-fixture.v1");
    assert_eq!(value["mode"], "loopback_cassette");
    assert_eq!(value["surfaces"].as_array().expect("surfaces").len(), 4);
    assert_eq!(value["physical_proof"], false);
    assert!(!raw.contains("/home/") && !raw.contains("/tmp/") && !raw.contains("shell_command"));
}

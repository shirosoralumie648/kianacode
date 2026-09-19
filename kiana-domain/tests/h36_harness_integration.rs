use kiana_domain::{
    HarnessCaseStatus, HarnessIntegrationCase, HarnessIntegrationMatrix, HarnessScenario,
    HarnessSurface,
};

const SPINE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RECEIPT: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn case(surface: HarnessSurface, scenario: HarnessScenario) -> HarnessIntegrationCase {
    let verified = surface == HarnessSurface::Cli && scenario == HarnessScenario::ShortTask;
    let unknown = surface == HarnessSurface::Web && scenario == HarnessScenario::Cancel;
    HarnessIntegrationCase::new(
        surface,
        scenario,
        if verified {
            HarnessCaseStatus::Verified
        } else if unknown {
            HarnessCaseStatus::Blocked
        } else {
            HarnessCaseStatus::NotImplemented
        },
        if verified {
            "fake CI receipt"
        } else if unknown {
            "result_unknown reconcile fixture pending"
        } else {
            "runtime scenario fixture pending"
        },
        SPINE,
        "fake",
        false,
        verified.then(|| RECEIPT.to_owned()),
        verified && scenario == HarnessScenario::Cancel,
        true,
        verified && surface == HarnessSurface::Desktop,
        unknown,
    )
    .unwrap()
}

fn matrix() -> HarnessIntegrationMatrix {
    let surfaces = [
        HarnessSurface::Cli,
        HarnessSurface::Workbench,
        HarnessSurface::Web,
        HarnessSurface::Desktop,
    ];
    let scenarios = [
        HarnessScenario::ShortTask,
        HarnessScenario::ToolCall,
        HarnessScenario::Repair,
        HarnessScenario::Steer,
        HarnessScenario::Cancel,
        HarnessScenario::Approval,
        HarnessScenario::Compaction,
        HarnessScenario::Restart,
    ];
    let cases = surfaces
        .into_iter()
        .flat_map(|surface| {
            scenarios
                .into_iter()
                .map(move |scenario| case(surface, scenario))
        })
        .collect();
    HarnessIntegrationMatrix::new(SPINE, cases).unwrap()
}

#[test]
fn matrix_covers_all_surfaces_and_harness_scenarios() {
    let matrix = matrix();
    assert_eq!(matrix.cases.len(), 32);
    assert!(matrix.validate().is_ok());
    assert!(!matrix.live_closeout_ready());
    assert!(matrix
        .live_closeout_blockers()
        .iter()
        .any(|blocker| blocker == "live_evidence_missing:desktop"));
}

#[test]
fn verified_and_desktop_receipts_are_required() {
    let missing = HarnessIntegrationCase::new(
        HarnessSurface::Cli,
        HarnessScenario::ToolCall,
        HarnessCaseStatus::Verified,
        "missing receipt",
        SPINE,
        "fake",
        false,
        None,
        false,
        true,
        false,
        false,
    );
    assert_eq!(
        missing.unwrap_err(),
        "harness_integration_verified_evidence_missing"
    );

    let desktop = HarnessIntegrationCase::new(
        HarnessSurface::Desktop,
        HarnessScenario::ShortTask,
        HarnessCaseStatus::Verified,
        "missing desktop state",
        SPINE,
        "fake",
        false,
        Some(RECEIPT.to_owned()),
        false,
        true,
        false,
        false,
    );
    assert_eq!(
        desktop.unwrap_err(),
        "harness_integration_desktop_receipt_missing"
    );

    let verified_unknown = HarnessIntegrationCase::new(
        HarnessSurface::Cli,
        HarnessScenario::ShortTask,
        HarnessCaseStatus::Verified,
        "unknown result",
        SPINE,
        "live_opt_in",
        true,
        Some(RECEIPT.to_owned()),
        false,
        true,
        false,
        true,
    );
    assert_eq!(
        verified_unknown.unwrap_err(),
        "harness_integration_verified_unknown_conflict"
    );
}

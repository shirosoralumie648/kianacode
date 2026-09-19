use kiana_domain::{
    SupplyChainGateStatus, SupplyChainReleaseDisposition, SupplyChainReleaseEvidence,
};

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const E: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn evidence(
    gate_status: SupplyChainGateStatus,
    disposition: SupplyChainReleaseDisposition,
    receipt: Option<&str>,
    unknown: bool,
    limitations: Vec<String>,
) -> Result<SupplyChainReleaseEvidence, String> {
    SupplyChainReleaseEvidence::new(
        "release-1",
        A,
        B,
        gate_status,
        disposition,
        vec![C.to_owned(), D.to_owned()],
        (disposition != SupplyChainReleaseDisposition::NotReleased)
            .then(|| "approval:release".to_owned()),
        receipt.map(str::to_owned),
        E,
        unknown,
        limitations,
    )
}

#[test]
fn approved_and_published_release_states_require_distinct_evidence() {
    let approved = evidence(
        SupplyChainGateStatus::Ready,
        SupplyChainReleaseDisposition::Approved,
        None,
        false,
        Vec::new(),
    )
    .expect("approved handoff");
    approved.validate().expect("approved validates");

    let published = evidence(
        SupplyChainGateStatus::Ready,
        SupplyChainReleaseDisposition::Published,
        Some(A),
        false,
        Vec::new(),
    )
    .expect("published handoff");
    published.validate().expect("published validates");
}

#[test]
fn blocked_gate_and_unknown_publish_fail_closed() {
    let blocked = evidence(
        SupplyChainGateStatus::Blocked,
        SupplyChainReleaseDisposition::Published,
        Some(A),
        false,
        Vec::new(),
    );
    assert_eq!(
        blocked.expect_err("blocked gate cannot publish"),
        "supply_chain_blocked_release"
    );

    let unknown = evidence(
        SupplyChainGateStatus::Ready,
        SupplyChainReleaseDisposition::Published,
        Some(A),
        true,
        Vec::new(),
    );
    assert_eq!(
        unknown.expect_err("unknown publish cannot verify"),
        "supply_chain_unknown_publish_cannot_verify"
    );
}

#[test]
fn not_released_handoff_retains_a_reason() {
    let value = evidence(
        SupplyChainGateStatus::Ready,
        SupplyChainReleaseDisposition::NotReleased,
        None,
        false,
        vec!["CI fixture did not perform an external release action".to_owned()],
    )
    .expect("not released handoff");
    value.validate().expect("not released validates");
}

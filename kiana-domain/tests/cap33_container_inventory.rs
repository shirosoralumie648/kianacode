use kiana_domain::{ContainerInventoryRecord, ContainerInventoryState};

const SCOPE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PLAN: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const ROOT: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const RUNTIME: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const LEASE: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn active() -> ContainerInventoryRecord {
    ContainerInventoryRecord::new(
        "container-1",
        "owner-1",
        SCOPE,
        PLAN,
        ROOT,
        RUNTIME,
        LEASE,
        3,
        2,
        11,
        ContainerInventoryState::Active,
        true,
        true,
    )
    .unwrap()
}

#[test]
fn active_inventory_recovers_only_with_matching_identity_and_fence() {
    let record = active();
    record.recover("owner-1", SCOPE, PLAN, ROOT, 3).unwrap();
    assert_eq!(record.restart_epoch, 2);
}

#[test]
fn stale_unknown_and_identity_drift_are_not_recovered() {
    let record = active();
    assert_eq!(
        record.recover("owner-2", SCOPE, PLAN, ROOT, 3).unwrap_err(),
        "container_inventory_identity_mismatch"
    );
    assert_eq!(
        record.recover("owner-1", SCOPE, PLAN, ROOT, 4).unwrap_err(),
        "container_inventory_lease_stale"
    );

    let unknown = ContainerInventoryRecord::new(
        "container-1",
        "owner-1",
        SCOPE,
        PLAN,
        ROOT,
        RUNTIME,
        LEASE,
        3,
        2,
        11,
        ContainerInventoryState::Unknown,
        false,
        false,
    )
    .unwrap();
    assert_eq!(
        unknown
            .recover("owner-1", SCOPE, PLAN, ROOT, 3)
            .unwrap_err(),
        "container_inventory_requires_reconcile"
    );
}

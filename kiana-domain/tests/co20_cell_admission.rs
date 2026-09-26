use kiana_domain::*;

fn resources(path: &str) -> CellAdmissionResources {
    CellAdmissionResources {
        plan_id: SpawnPlanId::new(),
        cell_id: CellId::new(),
        budget_lease_id: BudgetLeaseId::new(),
        capability_grant_id: CapabilityGrantId::new(),
        supervision_lease_id: SupervisionLeaseId::new(),
        owned_paths: vec![path.to_owned()],
    }
}

fn admission(id: &str, path: &str, now: u64) -> CellAdmission {
    let resource = resources(path);
    CellAdmission::reserve(
        id,
        format!("idempotency:{id}"),
        RunId::new(),
        resource.cell_id,
        resource,
        now,
    )
    .expect("admission")
}

#[test]
fn cell_admission_failure_cannot_leak_budget_paths_or_live_grants() {
    let mut ledger = CellAdmissionLedger::default();
    let first = admission("admission-1", "src/", 10);
    ledger.reserve(first.clone()).expect("reserve first");
    let conflicting = admission("admission-2", "src/", 11);
    assert_eq!(
        ledger.reserve(conflicting.clone()).unwrap_err(),
        "cell_admission_resource_conflict"
    );
    assert_eq!(ledger.admissions.len(), 1);
    assert!(!ledger.get("admission-1").unwrap().resources_released);

    ledger
        .rollback("admission-1", "cell_start_failed", 12)
        .expect("rollback");
    ledger.reserve(conflicting).expect("reserve after rollback");
    assert_eq!(ledger.admissions.len(), 2);
    assert!(ledger.get("admission-1").unwrap().resources_released);
    assert!(!ledger.get("admission-2").unwrap().resources_released);
}

#[test]
fn cell_retirement_releases_only_its_own_resources_once() {
    let mut ledger = CellAdmissionLedger::default();
    let first = admission("admission-1", "src/", 10);
    let second = admission("admission-2", "tests/", 10);
    ledger.reserve(first).expect("reserve first");
    ledger.reserve(second).expect("reserve second");
    ledger.commit("admission-1", 11).expect("commit first");
    ledger.commit("admission-2", 11).expect("commit second");

    ledger
        .get_mut_for_fixture("admission-1")
        .expect("first")
        .mark_capability_started()
        .expect("start");
    assert_eq!(
        ledger.retire("admission-1", "active", 12).unwrap_err(),
        "cell_admission_active_capability_release_forbidden"
    );
    ledger
        .get_mut_for_fixture("admission-1")
        .expect("first")
        .mark_capability_finished()
        .expect("finish");
    ledger
        .retire("admission-1", "terminal", 13)
        .expect("retire");
    ledger
        .retire("admission-1", "terminal", 13)
        .expect("idempotent retire");
    assert!(ledger.get("admission-1").unwrap().resources_released);
    assert!(!ledger.get("admission-2").unwrap().resources_released);
    assert_eq!(
        ledger.get("admission-2").unwrap().status,
        CellAdmissionStatus::Committed
    );
}

#[test]
fn unknown_admission_is_preserved_for_reconciliation_and_survives_reopen() {
    let mut ledger = CellAdmissionLedger::default();
    ledger
        .reserve(admission("admission-unknown", "src/", 10))
        .expect("reserve");
    ledger
        .mark_unknown("admission-unknown", "commit_result_unknown", 20)
        .expect("unknown");
    assert_eq!(
        ledger
            .rollback("admission-unknown", "late rollback", 21)
            .unwrap_err(),
        "cell_admission_release_conflict"
    );
    assert_eq!(
        ledger.get("admission-unknown").unwrap().status,
        CellAdmissionStatus::Unknown
    );
    let reopened: CellAdmissionLedger =
        serde_json::from_value(serde_json::to_value(&ledger).expect("serialize")).expect("reopen");
    assert_eq!(reopened, ledger);
}

// The fixture needs mutable access without exposing a second public mutation API in production;
// this extension is local to the integration test.
trait FixtureMutation {
    fn get_mut_for_fixture(&mut self, id: &str) -> Option<&mut CellAdmission>;
}

impl FixtureMutation for CellAdmissionLedger {
    fn get_mut_for_fixture(&mut self, id: &str) -> Option<&mut CellAdmission> {
        self.admissions.get_mut(id)
    }
}

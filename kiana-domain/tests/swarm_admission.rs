use kiana_domain::{
    DispatchIntentId, SwarmAdmissionLedger, SwarmAdmissionOutcome, SwarmAdmissionRequest,
    SwarmAdmissionResource, SWARM_ADMISSION_SCHEMA,
};
use serde_json::json;

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn request(idempotency_key: &str, fingerprint: &str) -> SwarmAdmissionRequest {
    SwarmAdmissionRequest {
        schema: SWARM_ADMISSION_SCHEMA.to_owned(),
        idempotency_key: idempotency_key.to_owned(),
        work_fingerprint: fingerprint.to_owned(),
        payload_digest: DIGEST_B.to_owned(),
        budget_units: 2,
        path_locks: vec!["path-a".to_owned()],
        data_locks: vec!["data-a".to_owned()],
        claim_key: "claim-a".to_owned(),
        grant_digest: DIGEST_A.to_owned(),
        supervision_key: "supervision-a".to_owned(),
        intent_key: "intent-a".to_owned(),
        authority_epoch: 3,
        config_revision: "config-1".to_owned(),
        policy_revision: "policy-1".to_owned(),
    }
}

#[test]
fn admission_reserves_all_resources_and_replays_idempotently() {
    let mut ledger = SwarmAdmissionLedger::new(4);
    let first = ledger.admit(&request("idem-a", DIGEST_A)).unwrap();
    assert_eq!(first.outcome, SwarmAdmissionOutcome::Reserved);
    assert_eq!(ledger.available_budget, 2);
    let revision = ledger.revision;

    let replay = ledger.admit(&request("idem-a", DIGEST_A)).unwrap();
    assert_eq!(replay.outcome, SwarmAdmissionOutcome::Replayed);
    assert_eq!(replay.admission_id, first.admission_id);
    assert_eq!(ledger.revision, revision);
}

#[test]
fn duplicate_payload_conflict_and_active_fingerprint_fail_closed() {
    let mut ledger = SwarmAdmissionLedger::new(8);
    ledger.admit(&request("idem-a", DIGEST_A)).unwrap();

    let mut conflict = request("idem-a", DIGEST_A);
    conflict.payload_digest = DIGEST_A.to_owned();
    assert_eq!(
        ledger.admit(&conflict),
        Err("swarm_admission_idempotency_conflict")
    );

    let mut active = request("idem-b", DIGEST_A);
    active.grant_digest = DIGEST_B.to_owned();
    active.claim_key = "claim-b".to_owned();
    active.supervision_key = "supervision-b".to_owned();
    active.intent_key = "intent-b".to_owned();
    active.path_locks = vec!["path-b".to_owned()];
    active.data_locks = vec!["data-b".to_owned()];
    assert_eq!(
        ledger.admit(&active),
        Err("swarm_admission_work_fingerprint_active")
    );
}

#[test]
fn failed_resource_preflight_has_no_partial_reservation() {
    let mut ledger = SwarmAdmissionLedger::new(8);
    let before = ledger.clone();
    let result = ledger.admit_with_fault(
        &request("idem-a", DIGEST_A),
        Some(SwarmAdmissionResource::Grant),
    );
    assert_eq!(result, Err("swarm_admission_atomic_preflight_fault"));
    assert_eq!(ledger, before);

    let mut conflict = request("idem-b", DIGEST_B);
    conflict.path_locks = vec!["data-a".to_owned()];
    assert_eq!(
        conflict.validate(),
        Err("swarm_admission_path_data_overlap")
    );
}

#[test]
fn unknown_fields_and_invalid_digest_are_rejected() {
    let mut value = serde_json::to_value(request("idem-a", DIGEST_A)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<SwarmAdmissionRequest>(value).is_err());

    let mut invalid = request("idem-a", DIGEST_A);
    invalid.grant_digest = "not-a-digest".to_owned();
    assert_eq!(invalid.validate(), Err("swarm_admission_request_invalid"));
    assert!(!DispatchIntentId::new().as_uuid().is_nil());
}

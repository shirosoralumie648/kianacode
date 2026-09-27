use kiana_domain::{
    schema_contract, FenceTokenId, InstanceId, OperationId, StopMethod, StopReport,
    SupervisorAction, SupervisorBackend, SupervisorObservation, SupervisorOutcome,
    SupervisorRequest, SupervisorSignal, SUPERVISOR_OBSERVATION_SCHEMA, SUPERVISOR_REQUEST_SCHEMA,
};
use uuid::Uuid;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn operation() -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(1))
}

fn instance() -> InstanceId {
    InstanceId::from_uuid(Uuid::from_u128(2))
}

fn fence() -> FenceTokenId {
    FenceTokenId::from_uuid(Uuid::from_u128(3))
}

fn request(action: SupervisorAction, generation: u64) -> SupervisorRequest {
    SupervisorRequest::new(
        operation(),
        instance(),
        D,
        D,
        D,
        fence(),
        4,
        7,
        SupervisorBackend::Fake,
        action,
        "fake:service",
        generation,
        1_000,
    )
    .unwrap()
}

fn confirmed_stop(execution_id: &str, method: StopMethod) -> StopReport {
    StopReport::new(
        execution_id,
        Some(42),
        Some(42),
        method,
        method != StopMethod::None,
        method == StopMethod::Kill,
        true,
        kiana_domain::ProcessGroupState::Empty,
        true,
        25,
    )
    .unwrap()
}

#[test]
fn supervisor_schemas_and_backends_are_registered() {
    for schema in [SUPERVISOR_REQUEST_SCHEMA, SUPERVISOR_OBSERVATION_SCHEMA] {
        let contract = schema_contract(schema).unwrap();
        assert_eq!(contract.owner_crate, "kiana-domain");
        assert!(!contract.allow_unknown_fields);
    }
    for (backend, prefix) in [
        (SupervisorBackend::Systemd, "systemd:"),
        (SupervisorBackend::Launchd, "launchd:"),
        (SupervisorBackend::WindowsService, "windows:"),
        (SupervisorBackend::Container, "container:"),
        (SupervisorBackend::Fake, "fake:"),
    ] {
        assert_eq!(backend.target_prefix(), prefix);
    }
}

#[test]
fn supervisor_start_stop_restart_require_observation_and_fence() {
    let start = request(SupervisorAction::Start, 1);
    let started = SupervisorObservation::new(
        &start,
        SupervisorOutcome::Started,
        SupervisorSignal::None,
        None,
        2,
        "observation:fake:start",
    )
    .unwrap();
    started.validate_against(&start).unwrap();

    let stop = request(SupervisorAction::Stop, 2);
    let stopped = SupervisorObservation::new(
        &stop,
        SupervisorOutcome::Stopped,
        SupervisorSignal::Term,
        Some(confirmed_stop("stop", StopMethod::Term)),
        2,
        "observation:fake:stop",
    )
    .unwrap();
    stopped.validate_against(&stop).unwrap();

    let restart = request(SupervisorAction::Restart, 2);
    let restarted = SupervisorObservation::new(
        &restart,
        SupervisorOutcome::Restarted,
        SupervisorSignal::Kill,
        Some(confirmed_stop("restart", StopMethod::Kill)),
        3,
        "observation:fake:restart",
    )
    .unwrap();
    restarted.validate_against(&restart).unwrap();

    let mut forged = restarted.clone();
    forged.observed_generation = 2;
    forged.observation_digest = forged.digest();
    assert_eq!(
        forged.validate_against(&restart).unwrap_err(),
        "supervisor_restart_fence_invalid"
    );
}

#[test]
fn supervisor_denies_pid_targets_and_unobserved_force_kill() {
    assert!(SupervisorRequest::new(
        operation(),
        instance(),
        D,
        D,
        D,
        fence(),
        4,
        7,
        SupervisorBackend::Fake,
        SupervisorAction::Stop,
        "pid:42",
        1,
        1_000,
    )
    .is_err());

    let stop = request(SupervisorAction::Stop, 2);
    let unconfirmed = StopReport::new(
        "forced-kill",
        Some(42),
        Some(42),
        StopMethod::Kill,
        false,
        true,
        false,
        kiana_domain::ProcessGroupState::Unknown,
        false,
        120_000,
    )
    .unwrap();
    let forged = SupervisorObservation::new(
        &stop,
        SupervisorOutcome::Stopped,
        SupervisorSignal::Kill,
        Some(unconfirmed),
        2,
        "observation:fake:forced-kill",
    );
    assert_eq!(forged.unwrap_err(), "supervisor_stop_observation_required");

    let timed_out = SupervisorObservation::new(
        &stop,
        SupervisorOutcome::TimedOut,
        SupervisorSignal::Kill,
        Some(
            StopReport::new(
                "forced-kill-timeout",
                Some(42),
                Some(42),
                StopMethod::Kill,
                false,
                true,
                false,
                kiana_domain::ProcessGroupState::Unknown,
                false,
                120_000,
            )
            .unwrap(),
        ),
        2,
        "observation:fake:timeout",
    )
    .unwrap();
    timed_out.validate_against(&stop).unwrap();
}

#[test]
fn supervisor_round_trip_and_tamper_are_bounded() {
    let request = request(SupervisorAction::Stop, 1);
    let encoded = serde_json::to_string(&request).unwrap();
    assert_eq!(
        serde_json::from_str::<SupervisorRequest>(&encoded).unwrap(),
        request
    );
    let mut tampered = request.clone();
    tampered.lease_digest =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned();
    assert_eq!(
        tampered.validate().unwrap_err(),
        "supervisor_request_digest_mismatch"
    );
}

use kiana_domain::ClockObservation;
use kiana_ports::{require_trusted_deadline, ClockPort, PortError};

struct FakeClock {
    wall: u64,
    monotonic: u64,
}

impl ClockPort for FakeClock {
    fn source(&self) -> &str {
        "fake-clock"
    }

    fn wall_now_unix_ms(&self) -> Result<u64, PortError> {
        Ok(self.wall)
    }

    fn monotonic_now_ms(&self) -> Result<u64, PortError> {
        Ok(self.monotonic)
    }
}

#[test]
fn fake_clock_observation_is_repeatable_and_deadline_gate_is_fail_closed() {
    let clock = FakeClock {
        wall: 1_000,
        monotonic: 10,
    };
    let first = clock.observe(None).unwrap();
    let second = clock.observe(Some(&first)).unwrap();
    assert_eq!(second.revision, 2);
    assert_eq!(second.trust, kiana_domain::ClockTrust::Trusted);
    assert_eq!(
        require_trusted_deadline(&clock, Some(&first), 2_000).unwrap(),
        second
    );

    let rollback = FakeClock {
        wall: 999,
        monotonic: 9,
    };
    assert!(matches!(
        require_trusted_deadline(&rollback, Some(&second), 2_000),
        Err(PortError::Failed(reason)) if reason == "clock_untrusted"
    ));
}

#[test]
fn persisted_observation_round_trips_without_raw_runtime_state() {
    let clock = FakeClock {
        wall: 4_000,
        monotonic: 30,
    };
    let observation = clock.observe(None).unwrap();
    let encoded = serde_json::to_value(&observation).unwrap();
    assert_eq!(
        serde_json::from_value::<ClockObservation>(encoded).unwrap(),
        observation
    );
}

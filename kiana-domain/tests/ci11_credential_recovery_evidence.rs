use kiana_domain::{
    CredentialLeaseProjectionState, CredentialRecoveryBlocker, CredentialRecoveryProjection,
    CredentialRecoveryStatus,
};

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const E: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const F: &str = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn projection(
    status: CredentialRecoveryStatus,
    lease: CredentialLeaseProjectionState,
    blockers: Vec<CredentialRecoveryBlocker>,
    explicit: bool,
    resume: bool,
    unknown: bool,
) -> Result<CredentialRecoveryProjection, String> {
    CredentialRecoveryProjection::new(
        "run:1",
        A,
        B,
        C,
        D,
        E,
        F,
        2,
        3,
        "config:2",
        "credential:2",
        lease,
        blockers,
        status,
        explicit,
        resume,
        unknown,
    )
}

#[test]
fn restart_projection_stays_paused_until_explicit_re_admission() {
    let value = projection(
        CredentialRecoveryStatus::ReAdmissionRequired,
        CredentialLeaseProjectionState::Missing,
        vec![
            CredentialRecoveryBlocker::LeaseMissing,
            CredentialRecoveryBlocker::StaleAuthorityEpoch,
        ],
        false,
        false,
        false,
    )
    .expect("paused recovery projection");
    value.validate().expect("paused projection validates");
}

#[test]
fn auto_resume_and_unknown_without_reconcile_fail_closed() {
    let auto = projection(
        CredentialRecoveryStatus::Paused,
        CredentialLeaseProjectionState::Active,
        Vec::new(),
        false,
        true,
        false,
    );
    assert_eq!(
        auto.expect_err("paused projection cannot auto resume"),
        "credential_recovery_auto_resume_forbidden"
    );

    let unknown = projection(
        CredentialRecoveryStatus::Unknown,
        CredentialLeaseProjectionState::Unknown,
        Vec::new(),
        false,
        false,
        true,
    );
    assert_eq!(
        unknown.expect_err("unknown needs blocker"),
        "credential_recovery_unknown_reason_missing"
    );
}

#[test]
fn ready_requires_active_lease_and_explicit_re_admission() {
    let value = projection(
        CredentialRecoveryStatus::Ready,
        CredentialLeaseProjectionState::Active,
        Vec::new(),
        true,
        true,
        false,
    )
    .expect("ready recovery projection");
    value.validate().expect("ready projection validates");
}

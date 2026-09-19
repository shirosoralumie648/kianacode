use kiana_protocol::{
    EvidenceLimitation, UiHostCapability, UiHostCapabilityDisposition, UiLiveHostEvidence,
    UiLiveHostStatus, UiSurface, UI_HOST_CAPABILITY_SCHEMA,
};

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn host_capability(direct_effect: bool) -> UiHostCapability {
    UiHostCapability {
        schema: UI_HOST_CAPABILITY_SCHEMA.to_owned(),
        capability_id: "editor.selection".to_owned(),
        actions: vec!["selection.read".to_owned()],
        scope_digest: digest('a'),
        disposition: UiHostCapabilityDisposition::Advertised,
        direct_effect,
        delegated_to_kiana: true,
        reason: None,
    }
}

fn verified() -> UiLiveHostEvidence {
    UiLiveHostEvidence::new(
        "acp-1",
        UiSurface::Ide,
        "editor-host",
        "1.2.3",
        digest('b'),
        digest('c'),
        Some("session:1".to_owned()),
        UiLiveHostStatus::Verified,
        Some("approval:ui-live".to_owned()),
        vec![host_capability(false)],
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        Some(digest('d')),
        Vec::new(),
    )
    .expect("verified host evidence")
}

#[test]
fn verified_host_evidence_requires_the_full_server_owned_session_boundary() {
    let evidence = verified();
    evidence.validate().expect("host evidence validates");
    let encoded = serde_json::to_value(&evidence).expect("encode evidence");
    assert_eq!(
        serde_json::from_value::<UiLiveHostEvidence>(encoded).expect("decode evidence"),
        evidence
    );
}

#[test]
fn host_direct_effect_is_rejected() {
    let mut capability = host_capability(true);
    assert_eq!(
        capability
            .validate()
            .expect_err("host effect must be rejected"),
        "ui_host_capability_direct_effect_forbidden"
    );
    capability.direct_effect = false;
    capability.delegated_to_kiana = false;
    assert_eq!(
        capability
            .validate()
            .expect_err("advertised capability must delegate"),
        "ui_host_capability_delegation_required"
    );
}

#[test]
fn opt_in_and_unknown_host_states_keep_approval_and_limitations_explicit() {
    let missing = UiLiveHostEvidence::new(
        "acp-1",
        UiSurface::Ide,
        "editor-host",
        "1.2.3",
        digest('b'),
        digest('c'),
        None,
        UiLiveHostStatus::OptedIn,
        None,
        Vec::new(),
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        None,
        Vec::new(),
    );
    assert_eq!(
        missing.expect_err("opt-in needs approval and limitation"),
        "ui_live_host_opt_in_evidence_incomplete"
    );

    let unknown = UiLiveHostEvidence::new(
        "acp-1",
        UiSurface::Ide,
        "editor-host",
        "1.2.3",
        digest('b'),
        digest('c'),
        None,
        UiLiveHostStatus::Unknown,
        None,
        Vec::new(),
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        None,
        vec![EvidenceLimitation {
            code: "host_restarted".to_owned(),
            detail: "session must be reconciled through the Kiana server".to_owned(),
        }],
    )
    .expect("unknown host evidence");
    unknown.validate().expect("unknown evidence validates");
}

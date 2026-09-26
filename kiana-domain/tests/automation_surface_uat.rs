use kiana_domain::*;

fn hash(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn uat() -> AutomationSurfaceUat {
    let command = hash('a');
    let snapshot = hash('b');
    let cases = [
        AutomationUatSurface::Cli,
        AutomationUatSurface::Web,
        AutomationUatSurface::Workbench,
        AutomationUatSurface::Desktop,
        AutomationUatSurface::Mcp,
    ]
    .into_iter()
    .map(|surface| {
        let mut value = AutomationSurfaceUatCase {
            schema: AUT23_SURFACE_UAT_SCHEMA.to_owned(),
            surface,
            command_digest: command.clone(),
            snapshot_digest: snapshot.clone(),
            source_cursor: 12,
            authority_epoch: 7,
            denied: true,
            broker_calls: 0,
            handler_calls: 0,
            direct_route: false,
            digest: String::new(),
        };
        value.digest = value.canonical_digest();
        value
    })
    .collect();
    AutomationSurfaceUat::new(cases)
}

#[test]
fn five_surfaces_share_one_snapshot_and_denial_has_zero_calls() {
    let value = uat();
    value.validate().expect("surface UAT");
    assert!(value.cases.iter().all(|case| case.broker_calls == 0));
}

#[test]
fn foreign_snapshot_or_direct_route_is_rejected() {
    let mut foreign = uat();
    foreign.cases[1].snapshot_digest = hash('c');
    foreign.cases[1].digest = foreign.cases[1].canonical_digest();
    foreign.digest = foreign.canonical_digest();
    assert_eq!(
        foreign.validate().unwrap_err(),
        "aut23_surface_snapshot_parity_invalid"
    );

    let mut direct = uat();
    direct.cases[0].direct_route = true;
    direct.cases[0].digest = direct.cases[0].canonical_digest();
    direct.digest = direct.canonical_digest();
    assert_eq!(
        direct.validate().unwrap_err(),
        "aut23_surface_route_or_deny_invalid"
    );
}

#[test]
fn missing_surface_and_denied_effect_are_rejected() {
    let mut missing = uat();
    missing.cases.pop();
    missing.digest = missing.canonical_digest();
    assert_eq!(missing.validate().unwrap_err(), "aut23_uat_header_invalid");

    let mut effect = uat();
    effect.cases[0].broker_calls = 1;
    effect.cases[0].digest = effect.cases[0].canonical_digest();
    effect.digest = effect.canonical_digest();
    assert_eq!(
        effect.validate().unwrap_err(),
        "aut23_surface_route_or_deny_invalid"
    );
}

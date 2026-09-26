use kiana_domain::{
    CompanyProjectView, CompanyReadModelFreshness, CompanyReadModelSnapshot, ProjectStatus,
};
use kiana_entrypoints::company_surface_parity::{
    CompanySurface, CompanySurfaceFrame, CompanySurfaceParity, StaleSurfaceAction,
};

fn snapshot() -> CompanyReadModelSnapshot {
    CompanyReadModelSnapshot::new(
        "project-1",
        10,
        Some(10),
        3,
        7,
        CompanyProjectView {
            project_id: "project-1".to_owned(),
            project_status: ProjectStatus::Active,
            baseline_version: Some(1),
            packet_views: Vec::new(),
            milestone_ids: Vec::new(),
            acceptance_status: None,
            delivery_status: None,
            closing_receipt_id: None,
            blockers: Vec::new(),
            evidence_links: Vec::new(),
        },
    )
    .expect("snapshot")
}

#[test]
fn cli_workbench_web_and_desktop_show_the_same_company_terminal_and_next_action() {
    let snapshot = snapshot();
    assert_eq!(snapshot.freshness, CompanyReadModelFreshness::CaughtUp);
    let frames = [
        CompanySurface::Cli,
        CompanySurface::Workbench,
        CompanySurface::Web,
        CompanySurface::Desktop,
    ]
    .into_iter()
    .map(|surface| CompanySurfaceFrame::from_snapshot(surface, &snapshot, "inspect").unwrap())
    .collect();
    let parity = CompanySurfaceParity::from_snapshot(&snapshot, frames).expect("parity");
    parity.validate_against(&snapshot).expect("same state");
}

#[test]
fn stale_web_action_and_reconnected_delta_cannot_repeat_company_transition() {
    let snapshot = snapshot();
    let mut stale = StaleSurfaceAction {
        project_id: "project-1".to_owned(),
        snapshot_digest: snapshot.snapshot_digest.clone(),
        revision: 2,
        authority_epoch: 7,
        action: "resume".to_owned(),
    };
    assert_eq!(
        stale.validate_against(&snapshot).unwrap_err(),
        "company_surface_action_stale"
    );
    stale.revision = snapshot.revision;
    stale.authority_epoch = snapshot.authority_epoch;
    stale.validate_against(&snapshot).expect("fresh action");
}

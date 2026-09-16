use kiana_domain::{validate_dependency_dag, WorkPacket};
use std::collections::BTreeMap;

fn packet(id: &str, dependencies: &[&str]) -> WorkPacket {
    WorkPacket::builder_task(id, format!("work for {id}")).with_dependencies(
        dependencies
            .iter()
            .map(|dependency| (*dependency).to_owned()),
    )
}

#[test]
fn dependency_cycle_is_rejected_deterministically() {
    let graph = BTreeMap::from([
        ("zeta".to_owned(), packet("zeta", &["alpha"])),
        ("alpha".to_owned(), packet("alpha", &["middle"])),
        ("middle".to_owned(), packet("middle", &["zeta"])),
    ]);
    let first = validate_dependency_dag(&graph).unwrap_err();
    let second = validate_dependency_dag(&graph).unwrap_err();
    assert_eq!(first, second);
    assert_eq!(first.code, "packet_dependency_cycle");
    assert_eq!(first.packet_id, "alpha");
    assert_eq!(first.cycle, vec!["alpha", "middle", "zeta", "alpha"]);
}

#[test]
fn dependency_missing_and_duplicate_edges_fail_closed() {
    let missing = BTreeMap::from([("root".to_owned(), packet("root", &["absent"]))]);
    let error = validate_dependency_dag(&missing).unwrap_err();
    assert_eq!(error.code, "packet_dependency_missing");
    assert_eq!(error.packet_id, "absent");

    let duplicate = BTreeMap::from([("root".to_owned(), packet("root", &["dep", "dep"]))]);
    assert_eq!(
        validate_dependency_dag(&duplicate).unwrap_err().code,
        "packet_dependency_duplicate"
    );
}

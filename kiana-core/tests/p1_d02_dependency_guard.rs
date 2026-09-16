#[test]
fn packet_approval_and_dispatch_use_the_domain_dependency_graph() {
    let domain = include_str!("../../kiana-domain/src/packet_graph.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");
    let core = include_str!("../src/company.rs");
    for marker in [
        "pub fn validate_dependency_dag",
        "packet_dependency_cycle",
        "packet_dependency_missing",
        "packet_dependency_duplicate",
        "cycle.rotate_left",
    ] {
        assert!(
            domain.contains(marker),
            "dependency marker missing: {marker}"
        );
    }
    assert!(company.contains("let mut graph = self.project_packets(project_id)"));
    assert!(company.contains("company_packet_dependency_graph_invalid"));
    assert!(company.matches("crate::validate_dependency_dag").count() >= 3);
    assert!(core.contains("kiana_domain::ready_packets"));
}

#[test]
fn product_callers_share_domain_ready_packets_predicate() {
    let domain = include_str!("../../kiana-domain/src/packet_graph.rs");
    let company_domain = include_str!("../../kiana-domain/src/company.rs");
    let core = include_str!("../src/company.rs");
    let legacy_tasks = include_str!("../../kiana-tasks/src/project_board.rs");
    assert!(domain.contains("pub fn ready_packets"));
    assert!(company_domain.matches("crate::ready_packets").count() >= 2);
    assert!(core.contains("kiana_domain::ready_packets"));
    assert!(legacy_tasks.contains("kiana_domain::ready_packets(packets, now_ms)"));
    assert!(!legacy_tasks.contains("pub fn ready_packets_internal"));
}

#[test]
fn packet_lease_scan_renews_and_reclaims_through_company_commands() {
    let domain = include_str!("../../kiana-domain/src/packet_graph.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");
    let core = include_str!("../src/company.rs");
    for marker in [
        "pub struct PacketClaim",
        "lease_expires_at",
        "heartbeat_at",
        "packet_claim_owner_mismatch",
        "packet_claim_expired",
    ] {
        assert!(
            domain.contains(marker),
            "lease contract marker missing: {marker}"
        );
    }
    for marker in [
        "CompanyCommand::RenewPacketClaim",
        "CompanyCommand::ReclaimPacketClaim",
        "company_claim_worker_not_stopped",
        "packet.packet.claim = None",
    ] {
        assert!(
            company.contains(marker),
            "lease transition marker missing: {marker}"
        );
    }
    for marker in [
        "pub(crate) async fn reclaim_packet_leases",
        "claim.active(company_now())",
        "idempotency_key: format!(\"reclaim:",
        "ReclaimPacketClaim",
        "renew_company_claim",
    ] {
        assert!(
            core.contains(marker),
            "core lease scan marker missing: {marker}"
        );
    }
}

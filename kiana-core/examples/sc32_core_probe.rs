use kiana_core::{
    AuditProjectionClaim, AuditProjectionCommitReport, AuditProjectionCommitStatus,
    AuditProjectionHead, AuditProjectionRebuildProof, AuditProjectionRebuildReport,
};
use serde_json::json;

const PROJECTOR: &str = "kiana-query/audit-projector";
fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}
fn head() -> AuditProjectionHead {
    AuditProjectionHead::new(PROJECTOR, 1, 10, digest('a')).unwrap()
}
fn claim(
    eg: u64,
    ng: u64,
    pc: u64,
    nc: u64,
    ps: String,
    ns: String,
    rb: Option<u64>,
) -> AuditProjectionClaim {
    AuditProjectionClaim::new(PROJECTOR, eg, ng, pc, nc, &ps, &ns, rb).unwrap()
}
fn resign_claim(mut v: AuditProjectionClaim) -> AuditProjectionClaim {
    v.claim_digest = v.digest();
    v
}
fn resign_report(mut v: AuditProjectionCommitReport) -> AuditProjectionCommitReport {
    v.report_digest = v.digest();
    v
}

fn main() {
    let h = head();

    // a_claim_that_moves_backwards_to_an_older_generation_is_not_an_exact_replay
    let r = AuditProjectionCommitReport::evaluate(
        &h,
        &claim(1, 1, 10, 9, digest('a'), digest('a'), None),
    )
    .unwrap();
    println!("[rollback-in-replay-clothing] status={:?} reason={:?} (test expects Rejected / audit_projection_cursor_regression)",
        r.status, r.reason);

    // a_claim_re_stating_the_same_cursor_with_a_different_state_is_not_advanced
    let r = AuditProjectionCommitReport::evaluate(
        &h,
        &claim(1, 2, 10, 10, digest('a'), digest('b'), None),
    )
    .unwrap();
    println!("[restate-cursor-new-state] status={:?} reason={:?} (test expects Rejected / audit_projection_cursor_not_advanced)",
        r.status, r.reason);

    // tampered report: rejection stripped of its reason
    let rej = resign_report(
        AuditProjectionCommitReport::evaluate(
            &h,
            &claim(1, 2, 10, 12, digest('a'), digest('b'), None),
        )
        .unwrap(),
    );
    let mut silent = rej.clone();
    silent.reason = String::new();
    silent.remediation = String::new();
    println!(
        "[silent rejection] err={:?} (test expects audit_projector_claim_digest_mismatch)",
        resign_report(silent)
            .validate_against(&h, &claim(1, 2, 10, 12, digest('a'), digest('b'), None))
            .unwrap_err()
    );

    // tampered rebuild report: converged + no reason
    let proof = AuditProjectionRebuildProof::new(
        PROJECTOR,
        1,
        10,
        &digest('a'),
        &digest('e'),
        10,
        &digest('z'),
        &digest('e'),
    )
    .unwrap();
    let rep = match AuditProjectionRebuildReport::evaluate(&h, &proof) {
        Ok(r) => r,
        Err(e) => {
            println!("[rebuild divergent] evaluate ERR={e}");
            kiana_core::AuditProjectionRebuildReport {
                schema: String::new(),
                version: kiana_domain::SchemaVersion::new(1, 0),
                projector: String::new(),
                status: kiana_core::AuditProjectionRebuildStatus::Divergent,
                generation: 1,
                source_cursor: 10,
                state_digest: digest('a'),
                reason: String::new(),
                remediation: String::new(),
                proof_digest: digest('e'),
                report_digest: digest('e'),
            }
        }
    };
    let mut conv = rep.clone();
    conv.status = kiana_core::AuditProjectionRebuildStatus::Converged;
    conv.reason = String::new();
    conv.remediation = String::new();
    conv.report_digest = conv.digest();
    println!(
        "[rebuild forged converged] err={:?} (test expects audit_projector_claim_digest_mismatch)",
        conv.validate_against(&h, &proof).unwrap_err()
    );

    let p2 = AuditProjectionRebuildProof::new(
        PROJECTOR,
        1,
        10,
        &digest('a'),
        &digest('e'),
        10,
        &digest('a'),
        &digest('e'),
    )
    .unwrap();
    let converged_rep = {
        let p2 = AuditProjectionRebuildProof::new(
            PROJECTOR,
            1,
            10,
            &digest('a'),
            &digest('e'),
            10,
            &digest('a'),
            &digest('e'),
        )
        .unwrap();
        AuditProjectionRebuildReport::evaluate(&h, &p2).unwrap()
    };
    println!(
        "[converged rep state] state_digest={}",
        converged_rep.state_digest
    );
    let mut edited = converged_rep;
    let before = edited.state_digest.clone();
    edited.state_digest = digest('a');
    println!("[rebuild edited state] before={} after={} SAME={} err={:?} (test expects audit_projection_rebuild_binding_invalid)",
        &before[..14], &edited.state_digest[..14], before == edited.state_digest,
        edited.validate_against(&h, &p2).map(|_| "OK"));

    // control cases that should pass
    for (name, c, want) in [
        (
            "exact replay",
            claim(1, 1, 10, 10, digest('a'), digest('a'), None),
            "ExactReplay/",
        ),
        (
            "cas race winner",
            claim(1, 2, 10, 11, digest('a'), digest('b'), None),
            "Advanced/",
        ),
        (
            "gen skip",
            claim(1, 3, 10, 11, digest('a'), digest('b'), None),
            "audit_projection_generation_not_monotonic",
        ),
        (
            "state drift",
            claim(1, 2, 10, 11, digest('f'), digest('b'), None),
            "audit_projection_state_digest_drift",
        ),
        (
            "gap",
            claim(1, 2, 10, 12, digest('a'), digest('b'), None),
            "audit_projection_cursor_gap",
        ),
        (
            "rebuild@10",
            claim(1, 2, 10, 20, digest('a'), digest('b'), Some(10)),
            "Advanced/",
        ),
        (
            "rebuild@0",
            claim(1, 2, 10, 20, digest('a'), digest('b'), Some(0)),
            "Advanced/",
        ),
        (
            "forged origin",
            claim(1, 2, 10, 20, digest('a'), digest('b'), Some(4)),
            "audit_projection_rebuild_origin_mismatch",
        ),
    ] {
        match AuditProjectionCommitReport::evaluate(&h, &c) {
            Ok(r) => println!(
                "  {name:22} status={:?} reason={:?} (want {want})",
                r.status, r.reason
            ),
            Err(e) => println!("  {name:22} CONSTRUCTION/EVAL ERR={e} (want {want})"),
        };
    }
    let _ = json!({});
    let _ = AuditProjectionCommitStatus::Advanced;
}

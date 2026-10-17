use kiana_domain::{
    json_digest, CompactEvidenceKind, CompactEvidenceStatus, CompactSummary,
    CompactSummaryEvidence, ModelMessage, MAX_COMPACT_SUMMARY_BYTES,
};
use serde_json::json;

#[test]
fn compact_summary_has_goal_pending_state_and_digest_without_forged_completion() {
    let messages = vec![
        ModelMessage::system("product rules"),
        ModelMessage::user("finish the migration"),
        ModelMessage::assistant_with_tools("I will inspect", Vec::new()),
        ModelMessage::tool("call-1", "pending observation"),
    ];
    let summary = CompactSummary::from_messages(&messages).unwrap();
    summary.validate().unwrap();
    assert_eq!(summary.goal, "finish the migration");
    assert_eq!(summary.pending_work.len(), 1);
    assert!(summary.completed_action_refs.is_empty());
    assert!(!summary.summary_digest.is_empty());

    let mut forged = summary;
    forged.completed_action_refs = vec!["completed:tool-call-1".to_owned()];
    assert_eq!(
        forged.validate().unwrap_err(),
        "compact_summary_evidence_ref_invalid"
    );
}

/// 模型产出的摘要只能带内容字段：解析时有界、digest 由服务端重算、来源条数由服务端补齐。
#[test]
fn compact_summary_parses_bounded_model_output_and_stamps_the_digest() {
    let text = serde_json::to_string(&json!({
        "schema": "kiana.compact-summary.v1",
        "version": {"major": 1, "minor": 0},
        "goal": "finish the migration",
        "constraints": ["read-only"],
        "decision_refs": [],
        "completed_action_refs": [],
        "verification_refs": [],
        "workspace_revisions": [],
        "pending_work": ["run the focused test"],
        "next_action": "run cargo fmt",
        "source_event_start": null,
        "source_event_end": null,
        "source_message_count": 0,
        "summary_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
    }))
    .unwrap();
    let summary = CompactSummary::parse_model_output(&text).expect("model output parses");
    assert_eq!(summary.source_message_count, 1, "服务端补齐最小来源条数");
    assert_ne!(
        summary.summary_digest,
        "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "模型自报 digest 必须被重算覆盖"
    );
    assert_eq!(summary.summary_digest, summary.digest());
    summary.validate_shape().unwrap();

    // 非 JSON、空文本和超长输出在解析前就被拒绝，不做无界解析。
    assert_eq!(
        CompactSummary::parse_model_output("").unwrap_err(),
        "compact_summary_empty"
    );
    assert_eq!(
        CompactSummary::parse_model_output("not json").unwrap_err(),
        "compact_summary_parse_failed"
    );
    assert_eq!(
        CompactSummary::parse_model_output("```json\n{}\n```").unwrap_err(),
        "compact_summary_parse_failed"
    );
    let oversized = format!("{{\"goal\":\"{}\"}}", "x".repeat(MAX_COMPACT_SUMMARY_BYTES));
    assert_eq!(
        CompactSummary::parse_model_output(&oversized).unwrap_err(),
        "compact_summary_size_exceeded"
    );
}

/// 模型自报的 evidence 不构成授权：只有服务端绑定的权威投影才能让引用生效。
#[test]
fn compact_summary_evidence_must_come_from_the_authoritative_projection() {
    let self_blessed = serde_json::to_string(&json!({
        "schema": "kiana.compact-summary.v1",
        "version": {"major": 1, "minor": 0},
        "goal": "finish the migration",
        "constraints": [],
        "decision_refs": [],
        "completed_action_refs": ["file:REPORT.md"],
        "verification_refs": [],
        "workspace_revisions": [],
        "pending_work": [],
        "next_action": "claim success",
        "source_event_start": null,
        "source_event_end": null,
        "source_message_count": 1,
        "evidence": [{
            "reference": "file:REPORT.md",
            "kind": "completed_action",
            "status": "confirmed",
            "fact_digest": json_digest(&json!({"file": "REPORT.md"})),
        }]
    }))
    .unwrap();
    let summary = CompactSummary::parse_model_output(&self_blessed).expect("shape parses");
    summary
        .validate()
        .expect("自报证据在结构上自洽，因此结构校验不能作为授权依据");
    assert_eq!(
        summary.validate_against_evidence(&[]).unwrap_err(),
        "compact_summary_evidence_not_authoritative",
        "没有权威投影时，模型自报的完成事实必须被拒绝"
    );

    // 权威投影里存在已确认事实时引用才成立；未确认或换 kind 都不行。
    let confirmed = CompactSummaryEvidence::new(
        "file:REPORT.md",
        CompactEvidenceKind::CompletedAction,
        CompactEvidenceStatus::Confirmed,
        json_digest(&json!({"file": "REPORT.md"})),
    )
    .unwrap();
    let pending = CompactSummaryEvidence::new(
        "file:REPORT.md",
        CompactEvidenceKind::CompletedAction,
        CompactEvidenceStatus::Pending,
        json_digest(&json!({"file": "REPORT.md"})),
    )
    .unwrap();
    let mut bound = CompactSummary::parse_model_output(&self_blessed).unwrap();
    bound.evidence = vec![confirmed.clone()];
    bound.stamp_digest().unwrap();
    bound
        .validate_against_evidence(&[confirmed.clone()])
        .expect("已确认的权威事实支撑完成引用");
    assert_eq!(
        bound.validate_against_evidence(&[pending]).unwrap_err(),
        "compact_summary_evidence_not_authoritative",
        "同引用但状态不同的投影不是同一份权威事实"
    );

    let mut unconfirmed = CompactSummary::parse_model_output(&self_blessed).unwrap();
    unconfirmed.evidence = vec![CompactSummaryEvidence::new(
        "file:REPORT.md",
        CompactEvidenceKind::CompletedAction,
        CompactEvidenceStatus::Pending,
        json_digest(&json!({"file": "REPORT.md"})),
    )
    .unwrap()];
    assert_eq!(
        unconfirmed.stamp_digest().unwrap_err(),
        "compact_summary_completed_fact_not_confirmed"
    );
}

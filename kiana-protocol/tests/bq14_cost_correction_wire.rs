//! BQ-14 `cost.correction` 命令的版本化 wire fixture（先拒绝、后成功）。
//!
//! 命令仍走共享 versioned command 路由；本文件只证明 wire 形状、审批绑定与回执摘要形状，
//! 不授权、不追加 EventLog、不消费审批。本地测试不在会话内运行（CI-only），手动 lane 为
//! `.github/workflows/bq14-cost-correction.yml`。

use kiana_domain::{
    ApprovalId, AttemptId, CostCorrectionApproval, CostCorrectionCommand, CostLedgerEntry,
    CostLedgerEntryKind, EventId, LedgerEntryId, Money, RateCardId, RequestId, RunId,
};
use kiana_protocol::{
    CostCorrectionCommandRequest, CostCorrectionCommandResponse, RequestBody, RequestEnvelope,
    RequestMetadata, COST_CORRECTION_COMMAND_NAME, COST_CORRECTION_COMMAND_REQUEST_SCHEMA,
    COST_CORRECTION_COMMAND_RESPONSE_SCHEMA,
};

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn target() -> CostLedgerEntry {
    CostLedgerEntry::new(
        LedgerEntryId::new(),
        CostLedgerEntryKind::Consumption,
        RunId::new(),
        Some(AttemptId::new()),
        None,
        "USD",
        10,
        EventId::new(),
        7,
        1,
        digest('a'),
    )
    .expect("entry")
    .with_rate_card(RateCardId::new(), 5)
    .expect("rate card")
    .with_estimated_cost(Money::new("USD", 400).expect("money"))
    .expect("estimate")
}

fn draft(target: &CostLedgerEntry) -> CostCorrectionCommand {
    CostCorrectionCommand::draft(
        RequestId::new(),
        kiana_domain::CostCorrectionId::new(),
        target,
        Some(Money::new("USD", -40).expect("money")),
        None,
        "invoice reconciliation",
        vec!["invoice:opaque-ref".to_owned()],
        "requester",
        "bq14-wire-idempotency",
        20,
    )
    .expect("draft")
}

fn approved(target: &CostLedgerEntry) -> CostCorrectionCommand {
    let draft = draft(target);
    let approval = CostCorrectionApproval::new(
        ApprovalId::new(),
        draft.command_id,
        draft.command_digest.clone(),
        target.entry_digest.clone(),
        "approver",
        21,
    )
    .expect("approval");
    draft.with_approval(approval).expect("approved command")
}

#[test]
fn approved_correction_rides_the_shared_versioned_command_route() {
    let target = target();
    let command = approved(&target);
    let envelope = RequestEnvelope::cost_correction_command(
        RequestMetadata::local("session", "/workspace"),
        command.clone(),
    )
    .expect("valid correction command");

    let RequestBody::Command(encoded) = &envelope.body else {
        panic!("correction must use the shared command route");
    };
    assert_eq!(encoded.name, COST_CORRECTION_COMMAND_NAME);
    assert_eq!(encoded.name, kiana_domain::COST_CORRECTION_COMMAND);
    assert_eq!(
        encoded.arguments["schema"],
        COST_CORRECTION_COMMAND_REQUEST_SCHEMA
    );
    assert_eq!(
        encoded.arguments["command"]["command_id"],
        command.command_id.to_string()
    );
    assert_eq!(
        encoded.arguments["command"]["target_entry_digest"],
        target.entry_digest
    );
    assert_eq!(encoded.arguments["command"]["approver_id"], "approver");

    let serialized = serde_json::to_string(&envelope).expect("encode envelope");
    assert!(!serialized.contains("transcript"));
    assert!(!serialized.contains("prompt"));
    let decoded: RequestEnvelope = serde_json::from_str(&serialized).expect("decode envelope");
    assert_eq!(decoded, envelope);
}

#[test]
fn unapproved_or_model_text_commands_are_refused_before_the_wire() {
    let target = target();
    let metadata = || RequestMetadata::local("session", "/workspace");

    let err = RequestEnvelope::cost_correction_command(metadata(), draft(&target))
        .expect_err("unapproved correction must not encode");
    assert_eq!(err, "cost_correction_approval_required");

    let mut model_text = approved(&target);
    model_text.reason = "\nmodel transcript: raise the invoice".to_owned();
    model_text.command_digest = model_text.digest();
    let err = RequestEnvelope::cost_correction_command(metadata(), model_text)
        .expect_err("model text must not encode");
    assert_eq!(err, "cost_correction_reason_invalid");

    // 未知字段不能借通用命令路由混入：typed request 自身拒绝。
    let request = CostCorrectionCommandRequest::new(approved(&target));
    let mut value = serde_json::to_value(&request).expect("json");
    value["transcript"] = serde_json::json!("model said raise the invoice");
    assert!(serde_json::from_value::<CostCorrectionCommandRequest>(value).is_err());
}

#[test]
fn response_shape_is_digest_bound_and_rejects_drift() {
    let target = target();
    let command = approved(&target);
    let response = CostCorrectionCommandResponse {
        schema: COST_CORRECTION_COMMAND_RESPONSE_SCHEMA.to_owned(),
        command_id: command.command_id,
        correction_id: command.correction_id,
        correction_digest: digest('b'),
        target_entry_digest: target.entry_digest.clone(),
        source_cursor: 9,
        replayed: false,
    };
    response.validate().expect("well formed projection");
    let decoded: CostCorrectionCommandResponse =
        serde_json::from_str(&serde_json::to_string(&response).expect("encode")).expect("decode");
    assert_eq!(decoded, response);

    let mut wrong_schema = response.clone();
    wrong_schema.schema = "kiana.cost-correction-command-response.v2".to_owned();
    assert_eq!(
        wrong_schema.validate().unwrap_err(),
        "cost_correction_command_response_invalid"
    );

    let mut zero_cursor = response.clone();
    zero_cursor.source_cursor = 0;
    assert_eq!(
        zero_cursor.validate().unwrap_err(),
        "cost_correction_command_response_invalid"
    );

    let mut bad_correction_digest = response.clone();
    bad_correction_digest.correction_digest = "sha256:not-hex".to_owned();
    assert_eq!(
        bad_correction_digest.validate().unwrap_err(),
        "cost_correction_correction_digest_invalid"
    );

    let mut bad_target_digest = response;
    bad_target_digest.target_entry_digest = "not-a-digest".to_owned();
    assert_eq!(
        bad_target_digest.validate().unwrap_err(),
        "cost_correction_target_entry_digest_invalid"
    );

    let mut unknown_field = serde_json::to_value(&response_projection()).expect("json");
    unknown_field["fingerprint"] = serde_json::json!("model-chosen");
    assert!(serde_json::from_value::<CostCorrectionCommandResponse>(unknown_field).is_err());
}

/// 独立构造一份形状正确的回执投影，供未知字段解码用例复用。
fn response_projection() -> CostCorrectionCommandResponse {
    let target = target();
    let command = approved(&target);
    CostCorrectionCommandResponse {
        schema: COST_CORRECTION_COMMAND_RESPONSE_SCHEMA.to_owned(),
        command_id: command.command_id,
        correction_id: command.correction_id,
        correction_digest: digest('c'),
        target_entry_digest: target.entry_digest,
        source_cursor: 3,
        replayed: true,
    }
}

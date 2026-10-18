//! BQ-22 provider invoice import、差异检测与 correction workflow 的合成夹具（先拒绝、后成功）。
//!
//! 覆盖卡片拒绝面：未认证 receipt、重复 invoice、period/model/usage/amount 不匹配、
//! 差异静默覆盖/未知被当成实测；以及成功列：账单导入生成 measured/correction，差异、未知
//! 与待人工项可追踪。全部事实在内存中合成，不连接 EventStore、不消费审批、不调用 provider，
//! 也不存在第二条修正账本；修正批准与落账仍属 BQ-14。

use kiana_domain::{
    compare_provider_invoice, parse_provider_invoice_document, validate_event_payload,
    validate_runtime_event, ApprovalId, AttemptId, CostCorrectionApproval, CostCorrectionCommand,
    CostCorrectionId, CostLedgerEntry, CostLedgerEntryKind, EventId, InvoiceAuthenticationEvidence,
    InvoiceComparison, InvoiceComparisonState, InvoiceImportOutcome, InvoiceLedgerExpectation,
    InvoiceObservationKind, InvoiceObservationOutcome, LedgerEntryId, Money, ProviderInvoiceImport,
    ProviderInvoiceLedger, ProviderInvoiceObservation, ProviderReceiptRef, RateCardId, RequestId,
    RunId, INVOICE_AUTHENTICATION_MAX_AGE_UNIX_MS, INVOICE_COMPARISON_SCHEMA, INVOICE_IMPORT_EVENT,
    INVOICE_OBSERVATION_EVENT, MAX_PROVIDER_INVOICE_DOCUMENT_BYTES,
    PROVIDER_INVOICE_DOCUMENT_SCHEMA, PROVIDER_INVOICE_SCHEMA,
};
use serde_json::json;

const D1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const D3: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const NOW: u64 = 200_000_000;
const EPOCH: u64 = 7;

/// 固定重复字符的 sha256 形状 fixture，不是真实内容摘要。
fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn money(currency: &str, micros: i128) -> Money {
    Money::new(currency, micros).expect("money")
}

fn receipt(value: &str) -> ProviderReceiptRef {
    ProviderReceiptRef::new(value.to_owned()).expect("receipt ref")
}

fn import_digest(import: &ProviderInvoiceImport) -> String {
    kiana_domain::json_digest(&json!({
        "schema": import.schema,
        "provider_account_ref": import.provider_account_ref,
        "invoice_id": import.invoice_id,
        "period_start_unix_ms": import.period_start_unix_ms,
        "period_end_unix_ms": import.period_end_unix_ms,
        "model": import.model,
        "usage_digest": import.usage_digest,
        "amount": import.amount,
        "receipt_ref": import.receipt_ref,
        "receipt_digest": import.receipt_digest,
        "authentication_ref": import.authentication_ref,
        "authenticated": import.authenticated,
        "source_cursor": import.source_cursor,
        "import_key": import.import_key,
    }))
}

fn reseal(import: &mut ProviderInvoiceImport) {
    import.import_digest = import_digest(import);
}

fn invoice(usage_digest: Option<&str>) -> ProviderInvoiceImport {
    let mut invoice = ProviderInvoiceImport {
        schema: PROVIDER_INVOICE_SCHEMA.to_owned(),
        provider_account_ref: "account:provider-1".to_owned(),
        invoice_id: "invoice:2026-09".to_owned(),
        period_start_unix_ms: 1_000,
        period_end_unix_ms: 2_000,
        model: Some("model-a".to_owned()),
        usage_digest: usage_digest.map(str::to_owned),
        amount: money("USD", 100),
        receipt_ref: ProviderReceiptRef::new("receipt:provider-1").expect("receipt ref"),
        receipt_digest: D3.to_owned(),
        authentication_ref: "verified:provider-1".to_owned(),
        authenticated: true,
        source_cursor: 12,
        import_key: "import:provider-1:2026-09".to_owned(),
        import_digest: String::new(),
    };
    invoice.import_digest = import_digest(&invoice);
    invoice
}

/// 同一 provider 账户下、不同 invoice/账期/游标的一份合法导入。
fn invoice_at(invoice_id: &str, amount_micros: i128, cursor: u64) -> ProviderInvoiceImport {
    let mut import = invoice(Some(D1));
    import.invoice_id = invoice_id.to_owned();
    import.amount = money("USD", amount_micros);
    import.source_cursor = cursor;
    import.import_key = format!("import:provider-1:{invoice_id}");
    reseal(&mut import);
    import
}

fn evidence(
    import: &ProviderInvoiceImport,
    verified_at: u64,
    epoch: u64,
) -> InvoiceAuthenticationEvidence {
    InvoiceAuthenticationEvidence::new(
        import.provider_account_ref.clone(),
        import.invoice_id.clone(),
        import.period_start_unix_ms,
        import.period_end_unix_ms,
        import.receipt_digest.clone(),
        "verifier:provider-1",
        verified_at,
        epoch,
    )
    .expect("evidence")
}

/// 与 `invoice(Some(D1))` 完全一致的账本期望：period 1000..2000、model-a、用量 D1、金额 100。
fn expectation(
    ledger_entry_digest: &str,
    ledger_entry_ref: &str,
    usage_digest: Option<&str>,
    amount: Option<Money>,
    cursor: u64,
) -> InvoiceLedgerExpectation {
    expectation_between(
        ledger_entry_digest,
        ledger_entry_ref,
        1_000,
        2_000,
        usage_digest,
        amount,
        cursor,
    )
}

/// 与 `invoice(Some(D1))` 同一 provider receipt 的账本期望；账期可显式指定。
#[allow(clippy::too_many_arguments)]
fn expectation_between(
    ledger_entry_digest: &str,
    ledger_entry_ref: &str,
    period_start_unix_ms: u64,
    period_end_unix_ms: u64,
    usage_digest: Option<&str>,
    amount: Option<Money>,
    cursor: u64,
) -> InvoiceLedgerExpectation {
    let mut expectation = InvoiceLedgerExpectation::new(
        ledger_entry_digest,
        ledger_entry_ref,
        period_start_unix_ms,
        period_end_unix_ms,
        "model-a",
        usage_digest.map(str::to_owned),
        amount,
        cursor,
    )
    .expect("expectation");
    expectation.receipt_ref = Some(receipt("receipt:provider-1"));
    expectation
}

fn matched_expectation() -> InvoiceLedgerExpectation {
    expectation(D2, "entry:matched", Some(D1), Some(money("USD", 100)), 9)
}

fn comparison(state: InvoiceComparisonState) -> InvoiceComparison {
    let (invoice_end, invoice_usage, invoice_amount, codes, correction_ref) = match state {
        InvoiceComparisonState::Matched => (2_000, Some(D1.to_owned()), 100, vec![], None),
        InvoiceComparisonState::CorrectionRequired => (
            2_001,
            Some(D1.to_owned()),
            120,
            vec!["period_mismatch".to_owned(), "amount_mismatch".to_owned()],
            Some("correction:invoice-1".to_owned()),
        ),
        InvoiceComparisonState::ReviewRequired => {
            (2_000, None, 100, vec!["usage_unknown".to_owned()], None)
        }
    };
    let mut comparison = InvoiceComparison {
        schema: INVOICE_COMPARISON_SCHEMA.to_owned(),
        invoice_digest: D1.to_owned(),
        ledger_entry_digest: D2.to_owned(),
        expected_period_start_unix_ms: 1_000,
        expected_period_end_unix_ms: 2_000,
        invoice_period_start_unix_ms: 1_000,
        invoice_period_end_unix_ms: invoice_end,
        expected_model: "model-a".to_owned(),
        invoice_model: Some("model-a".to_owned()),
        expected_usage_digest: Some(D1.to_owned()),
        invoice_usage_digest: invoice_usage,
        expected_amount: Some(money("USD", 100)),
        invoice_amount: money("USD", invoice_amount),
        expected_receipt_ref: Some(receipt("receipt:provider-1")),
        invoice_receipt_ref: Some(receipt("receipt:provider-1")),
        state,
        discrepancy_codes: codes,
        correction_ref,
        comparison_digest: String::new(),
    };
    comparison.comparison_digest = comparison.digest();
    comparison
}

/// 一份已封口的账本目标：BQ-14 draft 命令要求目标可验证且带 attempt。
fn ledger_entry(estimate_micros: i128) -> CostLedgerEntry {
    let mut entry = CostLedgerEntry::new(
        LedgerEntryId::new(),
        CostLedgerEntryKind::Consumption,
        RunId::new(),
        Some(AttemptId::new()),
        None,
        "USD",
        10,
        EventId::new(),
        1,
        1,
        digest('f'),
    )
    .expect("entry")
    .with_rate_card(RateCardId::new(), 7)
    .expect("rate card")
    .with_estimated_cost(money("USD", estimate_micros))
    .expect("estimate");
    entry.amount = Some(money("USD", estimate_micros));
    entry.entry_digest = entry.digest();
    entry.validate().expect("sealed entry");
    entry
}

// ---------------------------------------------------------------------------
// 先拒绝：认证
// ---------------------------------------------------------------------------

#[test]
fn authenticated_import_requires_server_evidence() {
    let import = invoice(Some(D1));
    assert!(import
        .validate_against_authentication(&evidence(&import, NOW, EPOCH), NOW, EPOCH)
        .is_ok());

    // wire 自报 authenticated=false 的收据在形状层就被拒绝。
    let mut unauthenticated = invoice(Some(D1));
    unauthenticated.authenticated = false;
    reseal(&mut unauthenticated);
    assert_eq!(
        unauthenticated.validate(),
        Err("provider_invoice_import_invalid")
    );
    assert_eq!(
        unauthenticated.validate_against_authentication(
            &evidence(&unauthenticated, NOW, EPOCH),
            NOW,
            EPOCH
        ),
        Err("provider_invoice_import_invalid")
    );

    // 收据摘要必须存在且形状合法。
    let mut missing_receipt_digest = invoice(Some(D1));
    missing_receipt_digest.receipt_digest = "receipt-bytes".to_owned();
    reseal(&mut missing_receipt_digest);
    assert_eq!(
        missing_receipt_digest.validate(),
        Err("provider_invoice_import_invalid")
    );

    // 证据与导入逐字段绑定：账户、invoice、账期两端、收据摘要任一漂移都拒绝。
    let baseline = evidence(&import, NOW, EPOCH);
    let mutators: [fn(&mut InvoiceAuthenticationEvidence); 5] = [
        |value: &mut InvoiceAuthenticationEvidence| {
            value.provider_account_ref = "account:provider-2".to_owned()
        },
        |value: &mut InvoiceAuthenticationEvidence| value.invoice_id = "invoice:2026-10".to_owned(),
        |value: &mut InvoiceAuthenticationEvidence| value.period_start_unix_ms = 1_001,
        |value: &mut InvoiceAuthenticationEvidence| value.period_end_unix_ms = 2_001,
        |value: &mut InvoiceAuthenticationEvidence| value.receipt_digest = D2.to_owned(),
    ];
    for mutate in mutators {
        let mut drifted = baseline.clone();
        mutate(&mut drifted);
        drifted.evidence_digest = drifted.digest();
        assert_eq!(
            import.validate_against_authentication(&drifted, NOW, EPOCH),
            Err("provider_invoice_authentication_binding_mismatch")
        );
    }

    // 证据自身的摘要与形状必须自洽：篡改字段但不重签 -> 形状拒绝。
    let mut tampered = baseline.clone();
    tampered.invoice_id = "invoice:2026-10".to_owned();
    assert_eq!(
        import.validate_against_authentication(&tampered, NOW, EPOCH),
        Err("provider_invoice_authentication_evidence_invalid")
    );
    let mut zero_verified = baseline.clone();
    zero_verified.verified_at_unix_ms = 0;
    zero_verified.evidence_digest = zero_verified.digest();
    assert_eq!(
        import.validate_against_authentication(&zero_verified, NOW, EPOCH),
        Err("provider_invoice_authentication_evidence_invalid")
    );

    // 授权世代：超前与落后都拒绝，只有与当前世代一致才通过。
    assert_eq!(
        import.validate_against_authentication(&evidence(&import, NOW, EPOCH + 1), NOW, EPOCH),
        Err("provider_invoice_authentication_epoch_ahead")
    );
    assert_eq!(
        import.validate_against_authentication(&evidence(&import, NOW, EPOCH - 1), NOW, EPOCH),
        Err("provider_invoice_authentication_epoch_stale")
    );

    // 验证时刻：未来时间拒绝；超过最大年龄拒绝；恰好到期边界仍可用。
    assert_eq!(
        import.validate_against_authentication(&evidence(&import, NOW + 1, EPOCH), NOW, EPOCH),
        Err("provider_invoice_authentication_timestamp_invalid")
    );
    assert_eq!(
        import.validate_against_authentication(&evidence(&import, 1, EPOCH), NOW, EPOCH),
        Err("provider_invoice_authentication_expired")
    );
    let boundary = NOW - INVOICE_AUTHENTICATION_MAX_AGE_UNIX_MS;
    assert!(import
        .validate_against_authentication(&evidence(&import, boundary, EPOCH), NOW, EPOCH)
        .is_ok());
    assert_eq!(
        import.validate_against_authentication(&evidence(&import, boundary - 1, EPOCH), NOW, EPOCH),
        Err("provider_invoice_authentication_expired")
    );
}

// ---------------------------------------------------------------------------
// 先拒绝：重复 invoice 与来源游标
// ---------------------------------------------------------------------------

#[test]
fn duplicate_invoice_identity_and_source_cursor_are_fenced() {
    let first = invoice(Some(D1));
    let mut ledger = ProviderInvoiceLedger::new();
    assert_eq!(ledger.append(&first), Ok(InvoiceImportOutcome::Imported));
    assert_eq!(ledger.last_source_cursor(), 12);
    assert_eq!(ledger.imports().count(), 1);

    // 同摘要重放幂等：不新增事实、不推进游标。
    assert_eq!(ledger.append(&first), Ok(InvoiceImportOutcome::Replayed));
    assert_eq!(ledger.imports().count(), 1);
    assert_eq!(ledger.last_source_cursor(), 12);

    // 同一身份换内容（金额变化 -> 摘要变化）不能新增第二条事实。
    let mut replayed_with_new_amount = invoice(Some(D1));
    replayed_with_new_amount.amount = money("USD", 120);
    reseal(&mut replayed_with_new_amount);
    assert_eq!(
        ledger.append(&replayed_with_new_amount),
        Err("provider_invoice_duplicate".to_owned())
    );
    assert_eq!(ledger.imports().count(), 1);
    assert_eq!(ledger.last_source_cursor(), 12);

    // 不同身份复用 import_key 是冲突，不是新事实。
    let mut shared_key = invoice_at("invoice:2026-10", 100, 13);
    shared_key.import_key = first.import_key.clone();
    reseal(&mut shared_key);
    assert_eq!(
        ledger.append(&shared_key),
        Err("provider_invoice_import_key_conflict".to_owned())
    );
    assert_eq!(ledger.imports().count(), 1);

    // 来源游标回退/持平拒绝，且不改变账本。
    let stale_cursor = invoice_at("invoice:2026-11", 100, 12);
    assert_eq!(
        ledger.append(&stale_cursor),
        Err("provider_invoice_source_cursor_regression".to_owned())
    );
    assert_eq!(ledger.imports().count(), 1);
    assert_eq!(ledger.last_source_cursor(), 12);

    // 形状非法不进入账本。
    let mut malformed = invoice_at("invoice:2026-12", 100, 14);
    malformed.period_end_unix_ms = malformed.period_start_unix_ms;
    reseal(&mut malformed);
    assert_eq!(
        ledger.append(&malformed),
        Err("provider_invoice_import_invalid".to_owned())
    );
    assert_eq!(ledger.imports().count(), 1);

    // 前进的游标才被接受。
    let accepted = invoice_at("invoice:2026-13", 100, 13);
    assert_eq!(ledger.append(&accepted), Ok(InvoiceImportOutcome::Imported));
    assert_eq!(ledger.imports().count(), 2);
    assert_eq!(ledger.last_source_cursor(), 13);

    // 既有静态重复围栏与账本规则一致。
    assert_eq!(
        kiana_domain::reject_duplicate_provider_invoices(&[first.clone(), first.clone()]),
        Err("provider_invoice_duplicate")
    );
}

// ---------------------------------------------------------------------------
// 先拒绝：比较状态由服务端派生
// ---------------------------------------------------------------------------

#[test]
fn server_side_comparison_derives_state_and_discrepancy_codes() {
    let import = invoice(Some(D1));

    let matched = compare_provider_invoice(&import, &matched_expectation(), None).expect("matched");
    assert_eq!(matched.state, InvoiceComparisonState::Matched);
    assert!(matched.discrepancy_codes.is_empty());
    assert!(matched.correction_ref.is_none());
    assert!(!matched.requires_review());

    // period / model / usage / amount 漂移都必须落到 CorrectionRequired 且带精确差异码。
    let period = expectation_between(
        D2,
        "entry:period",
        1_000,
        2_001,
        Some(D1),
        Some(money("USD", 100)),
        9,
    );
    let refused = compare_provider_invoice(&import, &period, None).unwrap_err();
    assert_eq!(refused, "provider_invoice_correction_ref_required");
    let period = compare_provider_invoice(&import, &period, Some("correction:1")).expect("period");
    assert_eq!(period.state, InvoiceComparisonState::CorrectionRequired);
    assert_eq!(period.discrepancy_codes, vec!["period_mismatch".to_owned()]);

    let model = expectation(D2, "entry:model", Some(D1), Some(money("USD", 100)), 9);
    let mut model_named = model.clone();
    model_named.model = "model-b".to_owned();
    let model =
        compare_provider_invoice(&import, &model_named, Some("correction:1")).expect("model");
    assert_eq!(model.discrepancy_codes, vec!["model_mismatch".to_owned()]);

    let usage = expectation_between(
        D2,
        "entry:usage",
        1_000,
        2_000,
        Some(D2),
        Some(money("USD", 100)),
        9,
    );
    let usage = compare_provider_invoice(&import, &usage, Some("correction:1")).expect("usage");
    assert_eq!(usage.discrepancy_codes, vec!["usage_mismatch".to_owned()]);

    let amount = expectation(D2, "entry:amount", Some(D1), Some(money("USD", 120)), 9);
    let amount = compare_provider_invoice(&import, &amount, Some("correction:1")).expect("amount");
    assert_eq!(amount.discrepancy_codes, vec!["amount_mismatch".to_owned()]);

    // 未知（账本侧或账单一侧）只能是 ReviewRequired，且绝不能带修正引用或实测金额。
    let ledger_unknown_usage =
        expectation(D2, "entry:unknown-usage", None, Some(money("USD", 100)), 9);
    let review =
        compare_provider_invoice(&import, &ledger_unknown_usage, None).expect("review usage");
    assert_eq!(review.state, InvoiceComparisonState::ReviewRequired);
    assert_eq!(review.discrepancy_codes, vec!["usage_unknown".to_owned()]);
    assert!(review.requires_review());
    assert!(review.correction_ref.is_none());
    assert_eq!(
        compare_provider_invoice(&import, &ledger_unknown_usage, Some("correction:1")).unwrap_err(),
        "provider_invoice_correction_ref_unexpected"
    );

    let ledger_unknown_amount = expectation(D2, "entry:unknown-amount", Some(D1), None, 9);
    let review =
        compare_provider_invoice(&import, &ledger_unknown_amount, None).expect("review amount");
    assert_eq!(review.state, InvoiceComparisonState::ReviewRequired);
    assert_eq!(review.discrepancy_codes, vec!["amount_unknown".to_owned()]);

    let invoice_missing_usage = invoice(None);
    let review = compare_provider_invoice(&invoice_missing_usage, &matched_expectation(), None)
        .expect("invoice usage unknown");
    assert_eq!(review.state, InvoiceComparisonState::ReviewRequired);
    assert_eq!(review.discrepancy_codes, vec!["usage_unknown".to_owned()]);

    // 币种漂移在构造比较事实前就被拒绝。
    let other_currency = expectation(D2, "entry:currency", Some(D1), Some(money("EUR", 100)), 9);
    assert_eq!(
        compare_provider_invoice(&import, &other_currency, None).unwrap_err(),
        "provider_invoice_currency_mismatch"
    );

    // 修正引用只能出现在 CorrectionRequired。
    assert_eq!(
        compare_provider_invoice(&import, &matched_expectation(), Some("correction:1"))
            .unwrap_err(),
        "provider_invoice_correction_ref_unexpected"
    );

    // 账本期望自身形状不合法时先拒绝。
    let mut invalid = matched_expectation();
    invalid.source_cursor = 0;
    assert_eq!(
        compare_provider_invoice(&import, &invalid, None).unwrap_err(),
        "provider_invoice_ledger_expectation_invalid"
    );
    let mut invalid = matched_expectation();
    invalid.usage_digest = Some("not-a-digest".to_owned());
    assert_eq!(
        compare_provider_invoice(&import, &invalid, None).unwrap_err(),
        "provider_invoice_ledger_expectation_invalid"
    );
}

#[test]
fn reported_comparisons_are_recomputed_and_cannot_claim_a_match() {
    let import = invoice(Some(D1));
    // 事实是 period 漂移，wire 却自报 Matched 且差异码为空：即使用自洽摘要重签也必须被拒。
    let period = expectation_between(
        D2,
        "entry:period",
        1_000,
        2_001,
        Some(D1),
        Some(money("USD", 100)),
        9,
    );

    let mut forged = comparison(InvoiceComparisonState::Matched);
    forged.invoice_digest = import.import_digest.clone();
    forged.invoice_period_end_unix_ms = 2_001;
    forged.comparison_digest = forged.digest();
    assert_eq!(forged.validate(), Err("invoice_comparison_match_invalid"));

    let recomputed =
        compare_provider_invoice(&import, &period, Some("correction:1")).expect("real");
    assert_ne!(recomputed, forged);
    assert_ne!(recomputed.comparison_digest, forged.comparison_digest);

    // 少写差异码 / 多带修正引用 / 状态与事实不符都在比较事实层拒绝。
    let mut missing_codes = comparison(InvoiceComparisonState::CorrectionRequired);
    missing_codes.invoice_period_end_unix_ms = 2_000;
    missing_codes.comparison_digest = missing_codes.digest();
    assert_eq!(
        missing_codes.validate(),
        Err("invoice_comparison_correction_required")
    );

    let mut review_with_ref = comparison(InvoiceComparisonState::ReviewRequired);
    review_with_ref.correction_ref = Some("correction:1".to_owned());
    review_with_ref.comparison_digest = review_with_ref.digest();
    assert_eq!(
        review_with_ref.validate(),
        Err("invoice_comparison_review_required")
    );

    let mut tampered = comparison(InvoiceComparisonState::Matched);
    tampered.comparison_digest = D3.to_owned();
    assert_eq!(tampered.validate(), Err("invoice_comparison_invalid"));

    // 服务端比较与自报值逐字段一致时才通过（摘要一致即事实一致）。
    let reported = compare_provider_invoice(&import, &matched_expectation(), None).expect("real");
    let recomputed = compare_provider_invoice(&import, &matched_expectation(), None).expect("re");
    assert_eq!(reported, recomputed);
}

// ---------------------------------------------------------------------------
// 先拒绝：观测事实
// ---------------------------------------------------------------------------

#[test]
fn observations_generate_measured_correction_and_keep_unknown_traceable() {
    let import = invoice(Some(D1));

    let matched =
        compare_provider_invoice(&import, &matched_expectation(), None).expect("matched compare");
    let measured = ProviderInvoiceObservation::from_import_comparison(&import, &matched, None)
        .expect("measured");
    assert_eq!(measured.kind, InvoiceObservationKind::Measured);
    assert_eq!(measured.measured_amount, Some(money("USD", 100)));
    assert_eq!(measured.usage_digest.as_deref(), Some(D1));
    assert!(measured.correction_ref.is_none());
    assert!(measured.review_ref.is_none());
    assert_eq!(measured.invoice_digest, import.import_digest);
    assert_eq!(measured.source_cursor, import.source_cursor);

    let mut correction = comparison(InvoiceComparisonState::CorrectionRequired);
    correction.invoice_digest = import.import_digest.clone();
    correction.comparison_digest = correction.digest();
    let observation =
        ProviderInvoiceObservation::from_import_comparison(&import, &correction, Some("review:1"))
            .unwrap_err();
    assert_eq!(observation, "provider_invoice_review_ref_unexpected");
    let correction_observation =
        ProviderInvoiceObservation::from_import_comparison(&import, &correction, None)
            .expect("correction observation");
    assert_eq!(
        correction_observation.kind,
        InvoiceObservationKind::CorrectionRequired
    );
    assert!(correction_observation.measured_amount.is_none());
    assert_eq!(
        correction_observation.correction_ref.as_deref(),
        Some("correction:invoice-1")
    );

    let review = compare_provider_invoice(
        &import,
        &expectation(D2, "entry:unknown", None, Some(money("USD", 100)), 9),
        None,
    )
    .expect("review compare");
    assert_eq!(
        ProviderInvoiceObservation::from_import_comparison(&import, &review, None).unwrap_err(),
        "provider_invoice_review_ref_required"
    );
    let review_observation =
        ProviderInvoiceObservation::from_import_comparison(&import, &review, Some("review:1"))
            .expect("review observation");
    assert_eq!(
        review_observation.kind,
        InvoiceObservationKind::ReviewRequired
    );
    assert!(review_observation.measured_amount.is_none());
    assert!(review_observation.usage_digest.is_none());
    assert_eq!(review_observation.review_ref.as_deref(), Some("review:1"));
    assert!(review_observation
        .discrepancy_codes
        .iter()
        .any(|code| code.ends_with("_unknown")));

    // 观测必须绑定同一条导入摘要。
    let other_import = invoice_at("invoice:2026-10", 100, 13);
    assert_eq!(
        ProviderInvoiceObservation::from_import_comparison(&other_import, &matched, None)
            .unwrap_err(),
        "provider_invoice_observation_invoice_mismatch"
    );

    // 零额是显式实测观测，不是“未知”的替代品。
    let mut zero_import = invoice(Some(D1));
    zero_import.amount = money("USD", 0);
    reseal(&mut zero_import);
    let zero_expected = expectation(D2, "entry:zero", Some(D1), Some(money("USD", 0)), 9);
    let zero_comparison =
        compare_provider_invoice(&zero_import, &zero_expected, None).expect("zero");
    let zero_observation =
        ProviderInvoiceObservation::from_import_comparison(&zero_import, &zero_comparison, None)
            .expect("zero observation");
    assert_eq!(zero_observation.measured_amount, Some(money("USD", 0)));

    // 负额、摘要篡改、未知被标实测都在事实层拒绝。
    let mut negative = measured.clone();
    negative.measured_amount = Some(money("USD", -1));
    negative.observation_digest = negative.digest();
    assert_eq!(
        negative.validate(),
        Err("provider_invoice_observation_amount_negative")
    );

    let mut tampered = measured.clone();
    tampered.observation_digest = D3.to_owned();
    assert_eq!(
        tampered.validate(),
        Err("provider_invoice_observation_invalid")
    );

    let mut forged_measured = review_observation.clone();
    forged_measured.kind = InvoiceObservationKind::Measured;
    forged_measured.measured_amount = Some(money("USD", 100));
    forged_measured.usage_digest = Some(D1.to_owned());
    forged_measured.review_ref = None;
    forged_measured.discrepancy_codes = vec![];
    // Bind the forged fact to a *different* committed entry digest so it is not byte-identical
    // to the recorded measured observation; the ledger must still refuse the overwrite.
    forged_measured.ledger_entry_digest = D3.to_owned();
    forged_measured.observation_digest = forged_measured.digest();
    // 形状上自洽，但它改变的是 invoice 摘要对应的既有事实：账本按发票唯一性拒绝第二次观测。
    let mut ledger = ProviderInvoiceLedger::new();
    ledger.append(&import).expect("import");
    ledger
        .record_observation(&measured)
        .expect("first observation");
    assert_eq!(
        ledger.record_observation(&forged_measured),
        Err("provider_invoice_measured_overwrite_refused".to_owned())
    );
}

#[test]
fn measured_observations_are_append_only_and_overwrites_are_refused() {
    let import = invoice(Some(D1));
    let matched =
        compare_provider_invoice(&import, &matched_expectation(), None).expect("matched compare");
    let measured = ProviderInvoiceObservation::from_import_comparison(&import, &matched, None)
        .expect("measured");
    let review = compare_provider_invoice(
        &import,
        &expectation(D2, "entry:unknown", None, Some(money("USD", 100)), 9),
        None,
    )
    .expect("review compare");
    let review_observation =
        ProviderInvoiceObservation::from_import_comparison(&import, &review, Some("review:1"))
            .expect("review observation");

    let mut ledger = ProviderInvoiceLedger::new();
    assert_eq!(
        ledger.record_observation(&measured),
        Err("provider_invoice_observation_unknown_import".to_owned())
    );
    assert_eq!(ledger.observations().count(), 0);

    assert_eq!(ledger.append(&import), Ok(InvoiceImportOutcome::Imported));
    assert_eq!(
        ledger.record_observation(&measured),
        Ok(InvoiceObservationOutcome::Recorded)
    );
    assert_eq!(
        ledger.record_observation(&measured),
        Ok(InvoiceObservationOutcome::Replayed)
    );
    assert_eq!(ledger.observations().count(), 1);

    // 第二次实测即使金额不同也不能覆盖原值。
    let mut overwrite = measured.clone();
    overwrite.measured_amount = Some(money("USD", 120));
    overwrite.ledger_entry_digest = D3.to_owned();
    overwrite.observation_digest = overwrite.digest();
    assert_eq!(
        ledger.record_observation(&overwrite),
        Err("provider_invoice_measured_overwrite_refused".to_owned())
    );
    assert_eq!(ledger.observations().count(), 1);
    assert_eq!(
        ledger
            .observation(&import.import_digest)
            .map(|value| value.measured_amount.clone()),
        Some(Some(money("USD", 100)))
    );

    // 已实测的 invoice 也不能被后来的 review 观测改写（没有降级/替换路径）。
    assert_eq!(
        ledger.record_observation(&review_observation),
        Err("provider_invoice_observation_replay_conflict".to_owned())
    );
    assert_eq!(ledger.observations().count(), 1);

    // 未知观测进入另一个 invoice：可追踪且不被当成实测。
    let unknown_import = invoice_at("invoice:2026-10", 100, 13);
    let mut unknown_expectation =
        expectation(D2, "entry:unknown", None, Some(money("USD", 100)), 9);
    unknown_expectation.usage_digest = None;
    let unknown_compare = compare_provider_invoice(&unknown_import, &unknown_expectation, None)
        .expect("unknown compare");
    let unknown_observation = ProviderInvoiceObservation::from_import_comparison(
        &unknown_import,
        &unknown_compare,
        Some("review:2"),
    )
    .expect("unknown observation");
    assert_eq!(
        ledger.append(&unknown_import),
        Ok(InvoiceImportOutcome::Imported)
    );
    assert_eq!(
        ledger.record_observation(&unknown_observation),
        Ok(InvoiceObservationOutcome::Recorded)
    );
    assert_eq!(ledger.observations().count(), 2);
    assert_eq!(ledger.pending_reviews().len(), 1);
    assert!(ledger.pending_reviews()[0].measured_amount.is_none());
}

// ---------------------------------------------------------------------------
// 先拒绝：注册事件与模型文本
// ---------------------------------------------------------------------------

#[test]
fn import_and_observation_facts_are_registered_cost_events() {
    let import = invoice(Some(D1));
    let import_event = import
        .to_runtime_event(RequestId::new(), 1, 1)
        .expect("import event");
    assert_eq!(import_event.kind, INVOICE_IMPORT_EVENT);
    assert_eq!(
        import_event.aggregate_type.as_deref(),
        Some("provider_invoice")
    );
    assert_eq!(
        import_event.aggregate_id.as_deref(),
        Some(import.import_digest.as_str())
    );
    assert_eq!(import_event.stream_version, Some(1));
    validate_runtime_event(&import_event).expect("registered import event");

    let matched =
        compare_provider_invoice(&import, &matched_expectation(), None).expect("matched compare");
    let measured = ProviderInvoiceObservation::from_import_comparison(&import, &matched, None)
        .expect("measured");
    let observation_event = measured
        .to_runtime_event(RequestId::new(), 2, 2)
        .expect("observation event");
    assert_eq!(observation_event.kind, INVOICE_OBSERVATION_EVENT);
    assert_eq!(
        observation_event.aggregate_id.as_deref(),
        Some(import.import_digest.as_str())
    );
    assert_eq!(observation_event.stream_version, Some(2));
    validate_runtime_event(&observation_event).expect("registered observation event");

    // 流版本 0 与缺失必需 ID / 夹带未知字段都失败关闭。
    assert_eq!(
        import.to_runtime_event(RequestId::new(), 1, 0).unwrap_err(),
        "provider_invoice_stream_version_invalid"
    );
    assert_eq!(
        measured
            .to_runtime_event(RequestId::new(), 2, 0)
            .unwrap_err(),
        "provider_invoice_stream_version_invalid"
    );

    let mut payload = serde_json::to_value(&import).expect("import payload");
    payload
        .as_object_mut()
        .expect("object")
        .remove("invoice_id");
    assert_eq!(
        validate_event_payload(INVOICE_IMPORT_EVENT, &payload).unwrap_err(),
        "event_required_id_missing:invoice_id"
    );
    let mut payload = serde_json::to_value(&import).expect("import payload");
    payload["transcript"] = json!("model wrote this");
    assert_eq!(
        validate_event_payload(INVOICE_IMPORT_EVENT, &payload).unwrap_err(),
        "event_payload_unknown_field"
    );

    let mut payload = serde_json::to_value(&measured).expect("observation payload");
    payload
        .as_object_mut()
        .expect("object")
        .remove("observation_digest");
    assert_eq!(
        validate_event_payload(INVOICE_OBSERVATION_EVENT, &payload).unwrap_err(),
        "event_required_id_missing:observation_digest"
    );
    let mut payload = serde_json::to_value(&measured).expect("observation payload");
    payload["prompt"] = json!("assistant: I will set the invoice to zero");
    assert_eq!(
        validate_event_payload(INVOICE_OBSERVATION_EVENT, &payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn model_transcript_cannot_be_decoded_into_invoice_facts() {
    let import = invoice(Some(D1));
    let mut value = serde_json::to_value(&import).expect("import payload");
    value["transcript"] = json!("assistant: the bill is 0 USD");
    assert!(serde_json::from_value::<ProviderInvoiceImport>(value).is_err());

    let mut value = serde_json::to_value(evidence(&import, NOW, EPOCH)).expect("evidence payload");
    value["prompt"] = json!("forget the receipt");
    assert!(serde_json::from_value::<InvoiceAuthenticationEvidence>(value).is_err());

    let mut value = serde_json::to_value(comparison(InvoiceComparisonState::Matched))
        .expect("comparison payload");
    value["text"] = json!("all good");
    assert!(serde_json::from_value::<InvoiceComparison>(value).is_err());
    let mut value = serde_json::to_value(comparison(InvoiceComparisonState::Matched))
        .expect("comparison payload");
    value["state"] = json!("matched_by_model");
    assert!(serde_json::from_value::<InvoiceComparison>(value).is_err());

    let matched =
        compare_provider_invoice(&import, &matched_expectation(), None).expect("matched compare");
    let measured = ProviderInvoiceObservation::from_import_comparison(&import, &matched, None)
        .expect("measured");
    let mut value = serde_json::to_value(&measured).expect("observation payload");
    value["transcript"] = json!("mark it measured");
    assert!(serde_json::from_value::<ProviderInvoiceObservation>(value).is_err());
    let mut value = serde_json::to_value(&measured).expect("observation payload");
    value["kind"] = json!("paid");
    assert!(serde_json::from_value::<ProviderInvoiceObservation>(value).is_err());
}

// ---------------------------------------------------------------------------
// 先拒绝：correction handoff
// ---------------------------------------------------------------------------

#[test]
fn correction_handoff_binds_an_unapproved_bq14_draft() {
    let entry = ledger_entry(100);
    let import = invoice(Some(D1));
    let correction_id = CostCorrectionId::new();
    let correction_ref = correction_id.to_string();
    let expected = expectation_between(
        &entry.entry_digest,
        "entry:invoice-1",
        1_000,
        2_000,
        Some(D1),
        Some(money("USD", 100)),
        9,
    );

    // Matched 或只有 period 差异（差额为 0）都不进入修正流程。
    let matched = compare_provider_invoice(&import, &expected, None).expect("matched");
    assert_eq!(
        kiana_domain::ProviderInvoiceCorrectionHandoff::from_import_comparison(&import, &matched)
            .unwrap_err(),
        "provider_invoice_handoff_requires_correction"
    );

    let period_only = expectation_between(
        &entry.entry_digest,
        "entry:period",
        1_000,
        2_001,
        Some(D1),
        Some(money("USD", 100)),
        9,
    );
    let period_only =
        compare_provider_invoice(&import, &period_only, Some(correction_ref.as_str()))
            .expect("period");
    assert_eq!(
        kiana_domain::ProviderInvoiceCorrectionHandoff::from_import_comparison(
            &import,
            &period_only
        )
        .unwrap_err(),
        "provider_invoice_handoff_amount_mismatch_required"
    );

    // 金额差异：差额是 invoice - expected 的显式签名值。
    let mut invoice_amount = invoice(Some(D1));
    invoice_amount.amount = money("USD", 120);
    reseal(&mut invoice_amount);
    let compared =
        compare_provider_invoice(&invoice_amount, &expected, Some(correction_ref.as_str()))
            .expect("correction compare");
    assert_eq!(
        compared.discrepancy_codes,
        vec!["amount_mismatch".to_owned()]
    );
    let handoff = kiana_domain::ProviderInvoiceCorrectionHandoff::from_import_comparison(
        &invoice_amount,
        &compared,
    )
    .expect("handoff");
    assert_eq!(handoff.delta_measured, money("USD", 20));
    assert!(handoff.validate().is_ok());

    // 零额 handoff 不能通过事实校验。
    let mut zero = handoff.clone();
    zero.delta_measured = money("USD", 0);
    zero.handoff_digest = zero.digest();
    assert_eq!(zero.validate(), Err("provider_invoice_handoff_invalid"));

    // 未批准的 BQ-14 draft：目标摘要、correction id、差额、证据与差异码全部一致时通过。
    let draft = CostCorrectionCommand::draft(
        RequestId::new(),
        correction_id,
        &entry,
        None,
        Some(money("USD", 20)),
        "invoice amount_mismatch: provider receipt reconciliation",
        handoff.evidence_refs.clone(),
        "requester",
        "bq22-handoff",
        NOW,
    )
    .expect("draft");
    assert!(handoff.validate_draft_command(&draft).is_ok());

    // 已批准命令不是 handoff 的出口。
    let approval = CostCorrectionApproval::new(
        ApprovalId::new(),
        draft.command_id,
        draft.command_digest.clone(),
        entry.entry_digest.clone(),
        "approver",
        NOW + 1,
    )
    .expect("approval");
    let approved = draft.clone().with_approval(approval).expect("approved");
    assert_eq!(
        handoff.validate_draft_command(&approved).unwrap_err(),
        "provider_invoice_handoff_command_approved"
    );

    // 目标摘要漂移。
    let mut wrong_target = draft.clone();
    wrong_target.target_entry_digest = D1.to_owned();
    wrong_target.command_digest = wrong_target.digest();
    assert_eq!(
        handoff.validate_draft_command(&wrong_target).unwrap_err(),
        "provider_invoice_handoff_command_binding_mismatch"
    );

    // correction id 漂移。
    let other_id = CostCorrectionId::new();
    let mut wrong_reference = CostCorrectionCommand::draft(
        RequestId::new(),
        other_id,
        &entry,
        None,
        Some(money("USD", 20)),
        "invoice amount_mismatch",
        handoff.evidence_refs.clone(),
        "requester",
        "bq22-handoff-2",
        NOW,
    )
    .expect("draft");
    wrong_reference.command_digest = wrong_reference.digest();
    assert_eq!(
        handoff
            .validate_draft_command(&wrong_reference)
            .unwrap_err(),
        "provider_invoice_handoff_command_reference_mismatch"
    );

    // 差额（含 estimated 旁路）与证据、理由漂移。
    let mut wrong_delta = draft.clone();
    wrong_delta.delta_measured = Some(money("USD", 21));
    wrong_delta.command_digest = wrong_delta.digest();
    assert_eq!(
        handoff.validate_draft_command(&wrong_delta).unwrap_err(),
        "provider_invoice_handoff_command_delta_mismatch"
    );
    let mut estimated_bypass = draft.clone();
    estimated_bypass.delta_estimated = Some(money("USD", 20));
    estimated_bypass.command_digest = estimated_bypass.digest();
    assert_eq!(
        handoff
            .validate_draft_command(&estimated_bypass)
            .unwrap_err(),
        "provider_invoice_handoff_command_delta_mismatch"
    );
    let mut missing_evidence = draft.clone();
    missing_evidence.evidence_refs = vec!["receipt:other".to_owned()];
    missing_evidence.command_digest = missing_evidence.digest();
    assert_eq!(
        handoff
            .validate_draft_command(&missing_evidence)
            .unwrap_err(),
        "provider_invoice_handoff_command_evidence_missing"
    );
    let mut missing_reason = draft.clone();
    missing_reason.reason = "invoice reconciliation".to_owned();
    missing_reason.command_digest = missing_reason.digest();
    assert_eq!(
        handoff.validate_draft_command(&missing_reason).unwrap_err(),
        "provider_invoice_handoff_command_reason_missing"
    );

    // handoff 自身的摘要篡改。
    let mut tampered = handoff.clone();
    tampered.handoff_digest = D2.to_owned();
    assert_eq!(tampered.validate(), Err("provider_invoice_handoff_invalid"));
}

// ---------------------------------------------------------------------------
// 成功列：可追踪
// ---------------------------------------------------------------------------

#[test]
fn invoice_ledger_traceability_keeps_measured_corrections_and_reviews_visible() {
    let entry = ledger_entry(100);
    let matched_import = invoice(Some(D1));
    let matched_expectation = expectation_between(
        &entry.entry_digest,
        "entry:matched",
        1_000,
        2_000,
        Some(D1),
        Some(money("USD", 100)),
        9,
    );

    let correction_import = invoice_at("invoice:2026-10", 120, 13);
    let correction_expectation = expectation_between(
        &entry.entry_digest,
        "entry:correction",
        1_000,
        2_000,
        Some(D1),
        Some(money("USD", 100)),
        9,
    );

    let review_import = invoice_at("invoice:2026-11", 100, 14);
    let review_expectation = expectation_between(
        &entry.entry_digest,
        "entry:review",
        1_000,
        2_000,
        None,
        Some(money("USD", 100)),
        9,
    );

    let mut ledger = ProviderInvoiceLedger::new();
    assert_eq!(
        ledger.append(&matched_import),
        Ok(InvoiceImportOutcome::Imported)
    );
    assert_eq!(
        ledger.append(&correction_import),
        Ok(InvoiceImportOutcome::Imported)
    );
    assert_eq!(
        ledger.append(&review_import),
        Ok(InvoiceImportOutcome::Imported)
    );
    assert_eq!(ledger.last_source_cursor(), 14);

    let matched_compare =
        compare_provider_invoice(&matched_import, &matched_expectation, None).expect("matched");
    let measured =
        ProviderInvoiceObservation::from_import_comparison(&matched_import, &matched_compare, None)
            .expect("measured");
    let correction_id = CostCorrectionId::new();
    let correction_compare = compare_provider_invoice(
        &correction_import,
        &correction_expectation,
        Some(correction_id.to_string().as_str()),
    )
    .expect("correction");
    let correction_observation = ProviderInvoiceObservation::from_import_comparison(
        &correction_import,
        &correction_compare,
        None,
    )
    .expect("correction observation");
    let review_compare =
        compare_provider_invoice(&review_import, &review_expectation, None).expect("review");
    let review_observation = ProviderInvoiceObservation::from_import_comparison(
        &review_import,
        &review_compare,
        Some("review:invoice-11"),
    )
    .expect("review observation");

    assert_eq!(
        ledger.record_observation(&measured),
        Ok(InvoiceObservationOutcome::Recorded)
    );
    assert_eq!(
        ledger.record_observation(&correction_observation),
        Ok(InvoiceObservationOutcome::Recorded)
    );
    assert_eq!(
        ledger.record_observation(&review_observation),
        Ok(InvoiceObservationOutcome::Recorded)
    );

    // 三类判定各自可追踪，且原始导入事实保持不变（只追加，无 update/delete）。
    assert_eq!(ledger.imports().count(), 3);
    assert_eq!(ledger.observations().count(), 3);
    assert_eq!(ledger.measured_observations().len(), 1);
    assert_eq!(ledger.open_corrections().len(), 1);
    assert_eq!(ledger.pending_reviews().len(), 1);

    let measured = ledger.measured_observations()[0];
    assert_eq!(measured.measured_amount, Some(money("USD", 100)));
    assert_eq!(measured.usage_digest.as_deref(), Some(D1));
    assert_eq!(measured.source_cursor, matched_import.source_cursor);

    let correction = ledger.open_corrections()[0];
    assert_eq!(
        correction.correction_ref.as_deref(),
        Some(correction_id.to_string().as_str())
    );
    assert_eq!(
        correction.discrepancy_codes,
        vec!["amount_mismatch".to_owned()]
    );
    assert!(correction.measured_amount.is_none());

    let review = ledger.pending_reviews()[0];
    assert_eq!(review.review_ref.as_deref(), Some("review:invoice-11"));
    assert!(review.measured_amount.is_none());
    assert_eq!(review.discrepancy_codes, vec!["usage_unknown".to_owned()]);

    let handoff = kiana_domain::ProviderInvoiceCorrectionHandoff::from_import_comparison(
        &correction_import,
        &correction_compare,
    )
    .expect("handoff");
    assert_eq!(handoff.delta_measured, money("USD", 20));
    assert_eq!(handoff.correction_ref, correction_id.to_string());

    assert_eq!(
        ledger
            .import(&matched_import.import_digest)
            .map(|value| value.amount.clone()),
        Some(money("USD", 100))
    );
    assert_eq!(
        ledger.append(&matched_import),
        Ok(InvoiceImportOutcome::Replayed)
    );
    assert_eq!(ledger.imports().count(), 3);
}

// ---------------------------------------------------------------------------
// 兼容：既有单条夹具（保留原断言）
// ---------------------------------------------------------------------------

#[test]
fn authenticated_invoice_import_is_deterministic_and_duplicates_fail_closed() {
    let imported = invoice(Some(D1));
    assert!(imported.validate().is_ok());
    assert!(kiana_domain::reject_duplicate_provider_invoices(&[imported.clone()]).is_ok());
    assert_eq!(
        kiana_domain::reject_duplicate_provider_invoices(&[imported.clone(), imported]),
        Err("provider_invoice_duplicate")
    );
}

#[test]
fn unauthenticated_or_malformed_receipts_are_rejected() {
    let mut invalid = invoice(Some(D1));
    invalid.authenticated = false;
    reseal(&mut invalid);
    assert_eq!(invalid.validate(), Err("provider_invoice_import_invalid"));

    let mut invalid = invoice(Some(D1));
    invalid.period_end_unix_ms = invalid.period_start_unix_ms;
    reseal(&mut invalid);
    assert_eq!(invalid.validate(), Err("provider_invoice_import_invalid"));

    let mut value = serde_json::to_value(invoice(Some(D1))).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ProviderInvoiceImport>(value).is_err());
}

#[test]
fn comparison_tracks_match_correction_and_unknown_review() {
    assert!(comparison(InvoiceComparisonState::Matched)
        .validate()
        .is_ok());
    assert!(comparison(InvoiceComparisonState::CorrectionRequired)
        .validate()
        .is_ok());
    assert!(comparison(InvoiceComparisonState::ReviewRequired)
        .validate()
        .is_ok());

    let mut forged = comparison(InvoiceComparisonState::CorrectionRequired);
    forged.correction_ref = None;
    forged.comparison_digest = forged.digest();
    assert_eq!(
        forged.validate(),
        Err("invoice_comparison_correction_required")
    );
}

// ---------------------------------------------------------------------------
// 先拒绝：账本绑定（TryFrom<CostLedgerEntry>）与收据身份
// ---------------------------------------------------------------------------

/// 一份带账期、模型和 provider 收据的 measured 账本事实；source_digest 同时是用量摘要。
fn measured_ledger_entry(cost_micros: i128) -> CostLedgerEntry {
    CostLedgerEntry::new(
        LedgerEntryId::new(),
        CostLedgerEntryKind::Consumption,
        RunId::new(),
        Some(AttemptId::new()),
        Some(money("USD", cost_micros)),
        "USD",
        10,
        EventId::new(),
        1,
        1,
        D1,
    )
    .expect("entry")
    .with_measured_cost(money("USD", cost_micros), receipt("receipt:provider-1"))
    .expect("measured")
    .with_billing_period(1_000, 2_000)
    .expect("period")
    .with_model("model-a")
    .expect("model")
}

#[test]
fn ledger_expectation_is_derived_from_the_authoritative_entry_not_from_wire_text() {
    let entry = measured_ledger_entry(100);
    let expectation = InvoiceLedgerExpectation::try_from(&entry).expect("expectation");
    assert_eq!(expectation.ledger_entry_digest, entry.entry_digest);
    assert_eq!(
        expectation.ledger_entry_ref,
        format!("ledger-entry:{}", entry.entry_id)
    );
    assert_eq!(expectation.usage_digest.as_deref(), Some(D1));
    assert_eq!(expectation.amount, Some(money("USD", 100)));
    assert_eq!(expectation.source_cursor, entry.source_cursor);
    assert_eq!(
        expectation
            .receipt_ref
            .as_ref()
            .map(ProviderReceiptRef::as_str),
        Some("receipt:provider-1")
    );

    // 成功列：同一份导入对账为 Matched（收据、账期、模型、用量、金额全部来自事实）。
    let import = invoice(Some(D1));
    let matched = compare_provider_invoice(&import, &expectation, None).expect("matched");
    assert_eq!(matched.state, InvoiceComparisonState::Matched);
    assert_eq!(
        matched
            .expected_receipt_ref
            .as_ref()
            .map(ProviderReceiptRef::as_str),
        Some("receipt:provider-1")
    );

    // 估计事实没有 measured 观测：用量/金额必须保持未知，比较只能是 review。
    let estimated = ledger_entry(100)
        .with_billing_period(1_000, 2_000)
        .expect("period")
        .with_model("model-a")
        .expect("model");
    let expectation = InvoiceLedgerExpectation::try_from(&estimated).expect("estimated");
    assert!(expectation.usage_digest.is_none());
    assert!(expectation.amount.is_none());
    let review = compare_provider_invoice(&import, &expectation, None).expect("review");
    assert_eq!(review.state, InvoiceComparisonState::ReviewRequired);
    assert_eq!(
        review.discrepancy_codes,
        vec![
            "usage_unknown".to_owned(),
            "amount_unknown".to_owned(),
            "receipt_unknown".to_owned()
        ]
    );
}

#[test]
fn forged_ledger_digest_receipts_and_missing_comparison_fields_are_refused() {
    // 伪造/篡改的账本摘要不能变成期望：entry 自证摘要不成立时构造直接失败。
    let mut forged_entry = measured_ledger_entry(100);
    forged_entry.entry_digest = D3.to_owned();
    assert_eq!(
        InvoiceLedgerExpectation::try_from(&forged_entry).unwrap_err(),
        "cost_ledger_entry_digest_mismatch"
    );

    // 手工构造的期望若声明了与权威 entry 不同的摘要，比较事实虽然自洽，但 handoff 的
    // ledger_entry_digest 就不是真实 entry 摘要（core 侧必须据此拒绝）。
    let entry = measured_ledger_entry(100);
    let mut forged_expectation =
        InvoiceLedgerExpectation::try_from(&entry).expect("authoritative expectation");
    forged_expectation.ledger_entry_digest = D3.to_owned();
    let mut import = invoice(Some(D1));
    import.amount = money("USD", 120);
    reseal(&mut import);
    let compared =
        compare_provider_invoice(&import, &forged_expectation, Some("correction:forged"))
            .expect("comparison");
    assert_eq!(compared.ledger_entry_digest, D3);
    assert_ne!(compared.ledger_entry_digest, entry.entry_digest);

    // 收据身份不同：在构造任何比较事实之前硬拒绝。
    let mut wrong_receipt =
        InvoiceLedgerExpectation::try_from(&measured_ledger_entry(100)).expect("expectation");
    wrong_receipt.receipt_ref = Some(receipt("receipt:provider-2"));
    assert_eq!(
        compare_provider_invoice(&invoice(Some(D1)), &wrong_receipt, None).unwrap_err(),
        "provider_invoice_receipt_mismatch"
    );

    // 账本事实没写收据：比较只能是 receipt_unknown + review，绝不是 Matched。
    let mut no_receipt =
        InvoiceLedgerExpectation::try_from(&measured_ledger_entry(100)).expect("expectation");
    no_receipt.receipt_ref = None;
    let review = compare_provider_invoice(&invoice(Some(D1)), &no_receipt, None).expect("review");
    assert_eq!(review.state, InvoiceComparisonState::ReviewRequired);
    assert!(review
        .discrepancy_codes
        .iter()
        .any(|code| code == "receipt_unknown"));

    // 缺账期或模型的事实不能补默认值来形成 matched 期望。
    let no_period = CostLedgerEntry::new(
        LedgerEntryId::new(),
        CostLedgerEntryKind::Consumption,
        RunId::new(),
        Some(AttemptId::new()),
        Some(money("USD", 100)),
        "USD",
        10,
        EventId::new(),
        1,
        1,
        D1,
    )
    .expect("entry")
    .with_measured_cost(money("USD", 100), receipt("receipt:provider-1"))
    .expect("measured");
    assert_eq!(
        InvoiceLedgerExpectation::try_from(&no_period).unwrap_err(),
        "provider_invoice_ledger_entry_period_missing"
    );
    let no_model = no_period
        .clone()
        .with_billing_period(1_000, 2_000)
        .expect("period");
    assert_eq!(
        InvoiceLedgerExpectation::try_from(&no_model).unwrap_err(),
        "provider_invoice_ledger_entry_model_missing"
    );

    // 半对账期与零宽账期在账本事实层就拒绝（建造器与解码都要 fail closed）。
    let mut half = no_period
        .clone()
        .with_billing_period(1_000, 2_000)
        .expect("period");
    half.period_end_unix_ms = None;
    half.entry_digest = half.digest();
    assert_eq!(
        half.validate(),
        Err("cost_ledger_entry_billing_period_pair_required".to_owned())
    );
    assert_eq!(
        no_period.with_billing_period(2_000, 2_000).unwrap_err(),
        "cost_ledger_entry_billing_period_invalid"
    );
}

// ---------------------------------------------------------------------------
// 先拒绝：本地 invoice document 解析器
// ---------------------------------------------------------------------------

fn invoice_document() -> serde_json::Value {
    json!({
        "schema": PROVIDER_INVOICE_DOCUMENT_SCHEMA,
        "provider_account_ref": "account:provider-1",
        "invoice_id": "invoice:2026-09",
        "period_start_unix_ms": 1_000,
        "period_end_unix_ms": 2_000,
        "model": "model-a",
        "usage_digest": D1,
        "amount": {"schema": "kiana.money.v1", "currency": "USD", "micros": 100},
        "receipt_ref": "receipt:provider-1",
        "receipt_digest": D3,
        "verifier_ref": "verifier:provider-1",
        "verified_at_unix_ms": NOW,
        "authority_epoch": EPOCH,
        "source_cursor": 12,
        "import_key": "import:provider-1:2026-09"
    })
}

fn parse_document(
    document: &serde_json::Value,
) -> Result<kiana_domain::ParsedProviderInvoiceDocument, String> {
    parse_provider_invoice_document(document.to_string().as_bytes())
}

#[test]
fn invoice_document_parser_refuses_unknown_fields_bad_shapes_and_bad_money() {
    // 空/超限/非 UTF-8 在 JSON 解析前就拒绝。
    assert_eq!(
        parse_provider_invoice_document(b"").unwrap_err(),
        "provider_invoice_document_empty"
    );
    assert_eq!(
        parse_provider_invoice_document(&vec![b' '; MAX_PROVIDER_INVOICE_DOCUMENT_BYTES + 1])
            .unwrap_err(),
        "provider_invoice_document_too_large"
    );
    assert_eq!(
        parse_provider_invoice_document(&[0xff, 0xfe]).unwrap_err(),
        "provider_invoice_document_not_utf8"
    );

    // 未知字段（包括自报摘要）必须整体拒绝，绝不部分解释。
    let mut unknown = invoice_document();
    unknown["unexpected_column"] = json!("value");
    assert!(parse_document(&unknown)
        .unwrap_err()
        .starts_with("provider_invoice_document_invalid_json:"));
    let mut self_reported = invoice_document();
    self_reported["import_digest"] = json!(D1);
    assert!(parse_document(&self_reported)
        .unwrap_err()
        .starts_with("provider_invoice_document_invalid_json:"));

    let mut wrong_schema = invoice_document();
    wrong_schema["schema"] = json!("kiana.provider-invoice.v1");
    assert_eq!(
        parse_document(&wrong_schema).unwrap_err(),
        "provider_invoice_document_schema_invalid"
    );

    let mut negative = invoice_document();
    negative["amount"]["micros"] = json!(-1);
    assert_eq!(
        parse_document(&negative).unwrap_err(),
        "provider_invoice_document_amount_negative"
    );
    let mut bad_money = invoice_document();
    bad_money["amount"]["currency"] = json!("usd");
    assert_eq!(
        parse_document(&bad_money).unwrap_err(),
        "provider_invoice_document_money_invalid"
    );

    let mut bad_receipt = invoice_document();
    bad_receipt["receipt_ref"] = json!("secret:receipt");
    assert_eq!(
        parse_document(&bad_receipt).unwrap_err(),
        "provider_invoice_document_receipt_invalid"
    );
    let mut bad_digest = invoice_document();
    bad_digest["receipt_digest"] = json!("not-a-digest");
    assert_eq!(
        parse_document(&bad_digest).unwrap_err(),
        "provider_invoice_document_receipt_digest_invalid"
    );
    let mut bad_usage = invoice_document();
    bad_usage["usage_digest"] = json!("not-a-digest");
    assert_eq!(
        parse_document(&bad_usage).unwrap_err(),
        "provider_invoice_document_import_invalid:provider_invoice_import_invalid"
    );
    let mut inverted_period = invoice_document();
    inverted_period["period_end_unix_ms"] = json!(1_000);
    assert_eq!(
        parse_document(&inverted_period).unwrap_err(),
        "provider_invoice_document_import_invalid:provider_invoice_import_invalid"
    );

    let mut zero_verified = invoice_document();
    zero_verified["verified_at_unix_ms"] = json!(0);
    assert_eq!(
        parse_document(&zero_verified).unwrap_err(),
        "provider_invoice_document_verified_at_invalid"
    );
    let mut zero_epoch = invoice_document();
    zero_epoch["authority_epoch"] = json!(0);
    assert_eq!(
        parse_document(&zero_epoch).unwrap_err(),
        "provider_invoice_document_authority_epoch_invalid"
    );
    let mut zero_cursor = invoice_document();
    zero_cursor["source_cursor"] = json!(0);
    assert_eq!(
        parse_document(&zero_cursor).unwrap_err(),
        "provider_invoice_document_source_cursor_invalid"
    );
    let mut blank_key = invoice_document();
    blank_key["import_key"] = json!("  ");
    assert_eq!(
        parse_document(&blank_key).unwrap_err(),
        "provider_invoice_document_import_key_invalid"
    );
    let mut blank_verifier = invoice_document();
    blank_verifier["verifier_ref"] = json!(" ");
    assert!(parse_document(&blank_verifier)
        .unwrap_err()
        .starts_with("provider_invoice_document_evidence_invalid:"));
}

#[test]
fn local_invoice_document_parses_into_a_bound_import_and_evidence() {
    let parsed = parse_document(&invoice_document()).expect("document");
    assert_eq!(parsed.import.schema, PROVIDER_INVOICE_SCHEMA);
    assert_eq!(parsed.import.invoice_id, "invoice:2026-09");
    assert_eq!(parsed.import.amount, money("USD", 100));
    assert_eq!(parsed.import.usage_digest.as_deref(), Some(D1));
    assert_eq!(
        parsed.import.authentication_ref,
        "document:verifier:provider-1"
    );
    assert!(parsed.import.validate().is_ok());
    assert!(parsed.authentication.validate().is_ok());
    // 导入与证据互相绑定，并且仍需调用方给出当前时刻与授权世代。
    assert!(parsed
        .import
        .validate_against_authentication(&parsed.authentication, NOW, EPOCH)
        .is_ok());

    // 同一份字节解析两次得到同一摘要（确定性、无隐藏时钟/随机）。
    let again = parse_document(&invoice_document()).expect("document");
    assert_eq!(parsed.import.import_digest, again.import.import_digest);
    assert_eq!(
        parsed.authentication.evidence_digest,
        again.authentication.evidence_digest
    );

    // 解析出的导入可以只追加进账本，重复导入幂等。
    let mut ledger = ProviderInvoiceLedger::new();
    assert_eq!(
        ledger.append(&parsed.import),
        Ok(InvoiceImportOutcome::Imported)
    );
    assert_eq!(
        ledger.append(&parsed.import),
        Ok(InvoiceImportOutcome::Replayed)
    );
    assert_eq!(ledger.imports().count(), 1);
}

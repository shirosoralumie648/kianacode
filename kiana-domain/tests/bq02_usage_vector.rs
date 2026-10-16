//! BQ-02 单条归一化用量合同的 Domain fixture。
//!
//! 本文件通过直接构造 UsageVector / NormalizedUsage 验证 absent 与显式零、来源与 observation 标签、Delta
//! 序号和 confidence/reason 组合。测试不启动 Provider、账本或持久事件写入；伪 digest 只用于格式检查。

use kiana_domain::{
    AttemptId, BillingUnknownReason, NormalizedUsage, UsageConfidence, UsageObservation,
    UsagePresence, UsageSource, UsageVector,
};

/// 生成 sha256: 加 64 位 hex 外形的伪摘要，不是对原始内容进行真正的 SHA-256 运算。
fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

#[test]
/// None 表示没有报告、Some(0) 表示来源明确报零、Some(n) 表示报了非零值。
/// 测试分别用向量方法和枚举 helper 检查三个状态没有被合并。
fn usage_vector_distinguishes_absent_zero_and_present() {
    let mut vector = UsageVector::zero();
    vector.input_tokens = None;
    vector.output_tokens = Some(0);
    vector.cache_read_tokens = Some(12);
    vector.validate().unwrap();
    assert_eq!(vector.input_presence(), UsagePresence::Absent);
    assert_eq!(vector.output_presence(), UsagePresence::ExplicitZero);
    assert_eq!(
        UsagePresence::of(vector.cache_read_tokens),
        UsagePresence::Present
    );
}

#[test]
/// Known provider Final 与 Partial local Delta 都必须保留来源、basis、sequence 和未知原因标签。
/// 这里分别验证单条 NormalizedUsage 可构造/校验，不把两条记录送进 accumulator。
fn normalized_usage_binds_source_basis_sequence_and_unknown_reason() {
    let usage = NormalizedUsage::new(
        kiana_domain::UsageId::new(),
        AttemptId::new(),
        kiana_domain::RunId::new(),
        Some("provider:test".to_owned()),
        "model:test",
        "route:v1",
        UsageVector::zero(),
        UsageSource::Provider,
        UsageObservation::Final,
        Some(2),
        UsageConfidence::Known,
        None,
        "provider_reported_final",
        digest('a'),
    )
    .unwrap();
    usage.validate().unwrap();

    let partial = NormalizedUsage::new(
        kiana_domain::UsageId::new(),
        AttemptId::new(),
        kiana_domain::RunId::new(),
        None,
        "local-model",
        "route:local",
        UsageVector::zero(),
        UsageSource::LocalExecutor,
        UsageObservation::Delta,
        Some(1),
        UsageConfidence::Partial,
        Some(BillingUnknownReason::ProviderUnreported),
        "local_partial",
        digest('b'),
    )
    .unwrap();
    assert_eq!(partial.sequence, Some(1));
    assert_eq!(
        partial.unknown_reason,
        Some(BillingUnknownReason::ProviderUnreported)
    );
}

#[test]
/// 保护 BQ-02 的三条拒绝路径：Delta 无 sequence、Unknown 无原因、向量被篡改后原摘要失配。
/// 字段篡改后返回 header 错误是因为 digest mismatch 与其他 header 条件在 validator 的同一分支检查。
fn normalized_usage_rejects_missing_sequence_reason_and_digest_drift() {
    let missing_sequence = NormalizedUsage::new(
        kiana_domain::UsageId::new(),
        AttemptId::new(),
        kiana_domain::RunId::new(),
        None,
        "model",
        "route",
        UsageVector::zero(),
        UsageSource::Derived,
        UsageObservation::Delta,
        None,
        UsageConfidence::Partial,
        Some(BillingUnknownReason::Partial),
        "derived",
        digest('c'),
    );
    assert_eq!(
        missing_sequence.unwrap_err(),
        "normalized_usage_delta_sequence_required"
    );

    let missing_reason = NormalizedUsage::new(
        kiana_domain::UsageId::new(),
        AttemptId::new(),
        kiana_domain::RunId::new(),
        None,
        "model",
        "route",
        UsageVector::zero(),
        UsageSource::Derived,
        UsageObservation::Final,
        None,
        UsageConfidence::Unknown,
        None,
        "unknown",
        digest('d'),
    );
    assert_eq!(
        missing_reason.unwrap_err(),
        "normalized_usage_unknown_reason_required"
    );

    let mut tampered = NormalizedUsage::new(
        kiana_domain::UsageId::new(),
        AttemptId::new(),
        kiana_domain::RunId::new(),
        None,
        "model",
        "route",
        UsageVector::zero(),
        UsageSource::Derived,
        UsageObservation::Final,
        None,
        UsageConfidence::Known,
        None,
        "known",
        digest('e'),
    )
    .unwrap();
    tampered.vector.tool_calls = 1;
    assert_eq!(
        tampered.validate().unwrap_err(),
        "normalized_usage_header_invalid"
    );
}

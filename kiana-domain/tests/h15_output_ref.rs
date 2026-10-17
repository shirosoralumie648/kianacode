use kiana_domain::{
    ExecutionOutputBudget, ExecutionOutputRef, InvocationId, RequestId, RunId,
    EXECUTION_OUTPUT_REF_SCHEMA, MAX_EXECUTION_OUTPUT_REF_BYTES,
};

fn digest(ch: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (ch as u32) & 0xffff).repeat(16)
    )
}

/// 输出预算在采集前就必须自洽：采集、预览、持久化、观察与行数都是正数，持久化/观察不小于
/// 采集，且不越过引用可承载的绝对值。任一违反都以同一个稳定错误码 fail-closed。
#[test]
fn output_budget_bounds_are_enforced_before_capture() {
    let budget = ExecutionOutputBudget::default();
    budget.validate().expect("default budget is bounded");

    let invalid = [
        ExecutionOutputBudget {
            collect_max_bytes: 0,
            ..budget.clone()
        },
        ExecutionOutputBudget {
            preview_max_bytes: 0,
            ..budget.clone()
        },
        ExecutionOutputBudget {
            persist_max_bytes: budget.collect_max_bytes - 1,
            ..budget.clone()
        },
        ExecutionOutputBudget {
            observed_max_bytes: budget.collect_max_bytes - 1,
            ..budget.clone()
        },
        ExecutionOutputBudget {
            max_lines: 0,
            ..budget.clone()
        },
        ExecutionOutputBudget {
            persist_max_bytes: MAX_EXECUTION_OUTPUT_REF_BYTES as usize + 1,
            ..budget.clone()
        },
        ExecutionOutputBudget {
            schema: "kiana.execution-output-budget.v2".to_owned(),
            ..budget.clone()
        },
    ];
    for candidate in invalid {
        let error = candidate.validate().unwrap_err();
        assert_eq!(error, "execution_output_budget_invalid");
    }
    // 预算结构是封闭的：注入未知字段会在反序列化时被拒绝，而不是被静默忽略。
    let mut forged = serde_json::to_value(&budget).expect("budget json");
    forged["unbounded"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ExecutionOutputBudget>(forged).is_err());
}

/// 引用的身份、大小与过期边界在构造期即拒绝：空过期、超绝对上限、nil 或与 output_id
/// 不一致的 invocation 都不会生成一份“看起来合法”的引用。
#[test]
fn output_reference_rejects_empty_expiry_oversize_and_identity_drift() {
    let output_id = RequestId::new();
    let run_id = RunId::new();
    let invocation = InvocationId::from_uuid(output_id.as_uuid());

    let empty_expiry = ExecutionOutputRef::new(
        output_id,
        Some(run_id),
        invocation,
        digest('c'),
        512,
        digest('s'),
        0,
    )
    .unwrap_err();
    assert_eq!(empty_expiry, "execution_output_ref_header_invalid");

    let oversize = ExecutionOutputRef::new(
        output_id,
        Some(run_id),
        invocation,
        digest('c'),
        MAX_EXECUTION_OUTPUT_REF_BYTES + 1,
        digest('s'),
        9_999,
    )
    .unwrap_err();
    assert_eq!(oversize, "execution_output_ref_header_invalid");

    let drifted = ExecutionOutputRef::new(
        output_id,
        Some(run_id),
        InvocationId::new(),
        digest('c'),
        512,
        digest('s'),
        9_999,
    )
    .unwrap_err();
    assert_eq!(drifted, "execution_output_ref_header_invalid");

    // 成功对照：字段自洽的引用生成后能自我校验，且 wire 名称为当前版本。
    let reference = ExecutionOutputRef::new(
        output_id,
        Some(run_id),
        invocation,
        digest('c'),
        512,
        digest('s'),
        9_999,
    )
    .expect("consistent reference");
    reference.validate().expect("valid reference");
    assert_eq!(reference.schema, EXECUTION_OUTPUT_REF_SCHEMA);
}

#[test]
fn output_reference_round_trips_with_invocation_binding() {
    let output_id = RequestId::new();
    let reference = ExecutionOutputRef::new(
        output_id,
        Some(RunId::new()),
        InvocationId::from_uuid(output_id.as_uuid()),
        digest('c'),
        512,
        digest('s'),
        9_999,
    )
    .unwrap();
    reference.validate().unwrap();
    let encoded = serde_json::to_value(&reference).unwrap();
    assert_eq!(
        serde_json::from_value::<ExecutionOutputRef>(encoded).unwrap(),
        reference
    );
}

#[test]
fn output_reference_tampering_is_rejected() {
    let output_id = RequestId::new();
    let mut reference = ExecutionOutputRef::new(
        output_id,
        Some(RunId::new()),
        InvocationId::from_uuid(output_id.as_uuid()),
        digest('c'),
        512,
        digest('s'),
        9_999,
    )
    .unwrap();
    reference.size_bytes = 513;
    assert_eq!(
        reference.validate().unwrap_err(),
        "execution_output_reference_digest_mismatch"
    );
}

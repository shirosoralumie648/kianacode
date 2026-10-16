//! DEP-15 运维诊断 Domain 合同的集成测试。
//!
//! 测试直接用合成 cursor、时间、摘要和四轴事实构造 DTO，覆盖序列化 schema、Ready 输出、Unknown/Missing 的非健康归并、
//! 路径/秘密文本拒绝和 report 篡改检测；它们不读取真实配置、启动状态、健康探针、观测存储或系统时钟。
//! 因而这些用例证明的是当前纯 reducer 对给定输入的行为，不证明生产环境已经采集事实或接通 DaemonHost 路由。

use kiana_domain::*;

// 固定格式的合成 SHA-256 字符串，仅用于满足 DTO 的摘要外形校验；不对应真实证据内容。
const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// 构造按 `OpsDiagnosticAxis::ALL` 顺序排列的四条合成事实。
///
/// 只有 Health 轴使用参数状态，其余轴固定为 Ready；这让单项测试可以改变一个轴观察总状态归并。
/// 非 Ready fact 沿用测试用原因和 remediation 文本，并不代表真实健康检查结果。
fn facts(cursor: u64, status: OpsAxisStatus) -> Vec<OpsDiagnosticFact> {
    OpsDiagnosticAxis::ALL
        .into_iter()
        .map(|axis| {
            let axis_status = if axis == OpsDiagnosticAxis::Health {
                status
            } else {
                OpsAxisStatus::Ready
            };
            OpsDiagnosticFact::new(
                axis,
                axis_status,
                cursor,
                (axis_status == OpsAxisStatus::Ready).then(|| D.to_owned()),
                if axis_status == OpsAxisStatus::Ready {
                    "ok"
                } else {
                    "health_unknown"
                },
                if axis_status == OpsAxisStatus::Ready {
                    "none"
                } else {
                    "inspect the redacted health evidence"
                },
            )
            .unwrap()
        })
        .collect()
}

/// 用 Doctor 模式、固定非零 cursor/时间和一条适配器来源限制，生成测试 input。
///
/// 这里的 `9` 和 `100` 只是稳定 fixture 值；input 校验会检查其非零和内部一致性，但不会将它们与外部事件或时钟对照。
fn input(status: OpsAxisStatus) -> OpsDiagnosticsInput {
    OpsDiagnosticsInput::new(
        OpsDiagnosticsMode::Doctor,
        9,
        100,
        facts(9, status),
        vec!["source facts are adapter supplied".to_owned()],
    )
    .unwrap()
}

/// 验证 schema 注册、默认 Ready 结果和两种渲染都保留安全/脱敏约束。
///
/// 本用例同时固定报告退出码及复现命令的可见文本，避免 wire 名称和面向操作者的输出意外漂移。
#[test]
fn diagnostics_schema_and_ready_outputs_are_stable_and_redacted() {
    for schema in [
        OPS_DIAGNOSTIC_FACT_SCHEMA,
        OPS_DIAGNOSTICS_INPUT_SCHEMA,
        OPS_DIAGNOSTICS_REPORT_SCHEMA,
    ] {
        let contract = schema_contract(schema).unwrap();
        assert_eq!(contract.owner_crate, "kiana-domain");
        assert!(!contract.allow_unknown_fields);
    }
    let input = input(OpsAxisStatus::Ready);
    let report = OpsDiagnosticsReport::evaluate(&input).unwrap();
    assert_eq!(report.status, OpsDiagnosticsStatus::Ready);
    assert!(report.healthy);
    assert_eq!(report.exit_code, 0);
    assert_eq!(
        report.reproduction.render().unwrap(),
        "kiana ops doctor --json"
    );
    assert!(report
        .render_json()
        .unwrap()
        .contains(OPS_DIAGNOSTICS_REPORT_SCHEMA));
    let human = report.render_human().unwrap();
    assert!(human.contains("status: ready"));
    assert!(human.contains("reproduction: kiana ops doctor --json"));
    assert!(!human.contains("/home/") && !human.contains("api_key="));
}

/// 确认 Unknown 和 Missing 都不会被整体报告归并为健康。
///
/// Unknown 直接成为总状态 Unknown；Config 轴 Missing 按 reducer 规则成为总状态 Blocked。
/// 这里只覆盖这两个拒绝健康的边界，不是所有状态组合的穷举测试。
#[test]
fn unknown_and_missing_evidence_never_become_healthy() {
    let unknown = OpsDiagnosticsReport::evaluate(&input(OpsAxisStatus::Unknown)).unwrap();
    assert_eq!(unknown.status, OpsDiagnosticsStatus::Unknown);
    assert!(!unknown.healthy);
    assert_eq!(unknown.exit_code, 30);

    let mut missing_facts = facts(9, OpsAxisStatus::Ready);
    missing_facts[0] = OpsDiagnosticFact::new(
        OpsDiagnosticAxis::Config,
        OpsAxisStatus::Missing,
        9,
        None,
        "config_evidence_missing",
        "load a trusted redacted config snapshot",
    )
    .unwrap();
    let missing = OpsDiagnosticsInput::new(
        OpsDiagnosticsMode::Preflight,
        9,
        100,
        missing_facts,
        Vec::new(),
    )
    .unwrap();
    let report = OpsDiagnosticsReport::evaluate(&missing).unwrap();
    assert_eq!(report.status, OpsDiagnosticsStatus::Blocked);
    assert!(!report.healthy);
    assert_eq!(report.exit_code, 20);
}

/// 验证文本安全拒绝以及 report 字段发生变化后的绑定失败。
///
/// ⚠ 函数名还包含 `wrong_mode`，但当前函数没有调用 `evaluate_ops_mode()`，也没有断言 mode mismatch 错误；
/// 因此“错误模式会被拒绝”尚未由这个 Domain 测试证明。保留原测试名和逻辑，只记录实际覆盖范围。
#[test]
fn diagnostics_reject_secret_paths_tamper_and_wrong_mode() {
    assert!(OpsDiagnosticFact::new(
        OpsDiagnosticAxis::Health,
        OpsAxisStatus::Blocked,
        9,
        Some(D.to_owned()),
        "provider failed at /home/operator",
        "use token=raw",
    )
    .is_err());

    let input = input(OpsAxisStatus::Ready);
    let mut report = OpsDiagnosticsReport::evaluate(&input).unwrap();
    report.healthy = false;
    assert_eq!(
        report.validate_against(&input).unwrap_err(),
        "ops_diagnostics_report_binding_invalid"
    );
}

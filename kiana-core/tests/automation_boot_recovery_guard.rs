//! AUT-21 Core 源守卫 + resume 推导行为守卫（两个测试）。
//!
//! 源守卫检查恢复门面没有第二条执行循环；行为守卫证明「自证 Ready」不被接受，只有清空所有
//! 屏障并由 Core 自己推导出的 Ready 才能作为 resume 结论。

use kiana_core::{
    derive_boot_recovery_resume, validate_boot_recovery, AUTOMATION_BOOT_RECOVERY_RESUME_COMMAND,
};
use kiana_domain::{
    AutomationBootJournal, AutomationBootLease, AutomationBootRecoveryFact,
    AutomationBootRecoveryState, AutomationReconcileCase, AUTOMATION_BOOT_RECOVERY_SCHEMA,
    JOURNAL_HEADER_SCHEMA,
};

#[test]
fn boot_recovery_facade_has_no_second_execution_loop() {
    let domain = include_str!("../../kiana-domain/src/automation_boot_recovery.rs");
    let core = include_str!("../src/automation_boot_recovery.rs");
    let daemon = include_str!("../../kiana-daemon/src/automation_boot_recovery.rs");
    for marker in [
        "AutomationBootRecoveryFact",
        "source_cursor",
        "projection_cursor",
        "pending_unknown_count",
        "ReconcileRequired",
        "AutomationReconcileCase",
        "AutomationBootLease",
        "AutomationBootJournal",
        "derive_boot_recovery_resume",
        "validate_boot_recovery",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker) || daemon.contains(marker),
            "AUT-21 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "auto_success",
    ] {
        for (name, source) in [("domain", domain), ("core", core), ("daemon", daemon)] {
            assert!(
                !source.contains(forbidden),
                "AUT-21 {name} boundary executes effects: {forbidden}"
            );
        }
    }
    // 显式 resume 必须经过唯一命令路由，而不是 daemon 自己另开入口。
    let commands = include_str!("../src/commands.rs");
    assert!(commands.contains("AUTOMATION_BOOT_RECOVERY_RESUME_COMMAND"));
    assert!(commands.contains("handle_boot_recovery_resume"));
    // 两个 effect 服务启动入口都必须先过恢复 fence，不能绕过 operator resume。
    let daemon_host = include_str!("../../kiana-daemon/src/lib.rs");
    assert!(daemon_host.contains("ensure_boot_recovery_admits_dispatch"));
    assert_eq!(
        daemon_host
            .matches("ensure_boot_recovery_admits_dispatch")
            .count(),
        2
    );
    assert_eq!(
        AUTOMATION_BOOT_RECOVERY_RESUME_COMMAND,
        "automation.boot_recovery.resume.v1"
    );
}

fn fact(state: AutomationBootRecoveryState) -> AutomationBootRecoveryFact {
    AutomationBootRecoveryFact {
        schema: AUTOMATION_BOOT_RECOVERY_SCHEMA.to_owned(),
        source_cursor: 10,
        projection_cursor: 10,
        projection_generation: 1,
        authority_epoch: 4,
        observed_authority_epoch: 4,
        pending_unknown_count: 0,
        stale_index: false,
        lease: AutomationBootLease::absent(),
        journal: AutomationBootJournal::healthy(),
        state,
        reconcile_cases: Vec::new(),
        recovery_digest: String::new(),
    }
    .sealed()
}

#[test]
fn resume_refuses_self_certified_ready_and_unreconciled_facts() {
    // 自证 Ready：即使字段全部干净也拒绝，resume 结论必须由 Core 推导。
    assert_eq!(
        derive_boot_recovery_resume(&fact(AutomationBootRecoveryState::Ready)),
        Err("automation_boot_recovery_resume_already_ready")
    );

    // ReconcileRequired 必须先走既有对账路径，resume 不是 reconcile。
    let mut reconcile = fact(AutomationBootRecoveryState::ReconcileRequired);
    reconcile.pending_unknown_count = 2;
    reconcile.projection_cursor = 9;
    reconcile.reconcile_cases = vec![AutomationReconcileCase {
        source_cursor: 9,
        projection_generation: 1,
        authority_epoch: 4,
        pending_unknown_count: 2,
    }];
    let reconcile = reconcile.sealed();
    assert!(validate_boot_recovery(&reconcile).is_ok());
    assert_eq!(
        derive_boot_recovery_resume(&reconcile),
        Err("automation_boot_recovery_reconcile_required")
    );

    // Blocked 证据不能通过 resume 洗白。
    assert_eq!(
        derive_boot_recovery_resume(&fact(AutomationBootRecoveryState::Blocked)),
        Err("automation_boot_recovery_blocked")
    );

    // Rebuilding 但投影仍落后：resume 必须返回 Domain 的稳定屏障码。
    let mut lagging = fact(AutomationBootRecoveryState::Rebuilding);
    lagging.projection_cursor = 9;
    assert_eq!(
        derive_boot_recovery_resume(&lagging.sealed()),
        Err("automation_boot_recovery_projection_lagging")
    );

    // 过期租约同样拒绝。
    let mut stale_lease = fact(AutomationBootRecoveryState::Rebuilding);
    stale_lease.lease = AutomationBootLease {
        lease_id: Some("instance-a".to_owned()),
        expires_at_unix_ms: 100,
        observed_at_unix_ms: 200,
    };
    assert_eq!(
        derive_boot_recovery_resume(&stale_lease.sealed()),
        Err("automation_boot_recovery_stale_lease")
    );

    // journal schema 漂移拒绝（重启路径不得把旧 writer 当成可恢复）。
    let mut drift = fact(AutomationBootRecoveryState::Rebuilding);
    drift.journal = AutomationBootJournal {
        schema: JOURNAL_HEADER_SCHEMA.replace("v2", "v1"),
        healthy: true,
    };
    assert_eq!(
        derive_boot_recovery_resume(&drift.sealed()),
        Err("automation_boot_recovery_journal_schema_mismatch")
    );

    // 成功夹具在最后：干净 Rebuilding 推导出 Ready，且摘要被重新计算。
    let ready = derive_boot_recovery_resume(&fact(AutomationBootRecoveryState::Rebuilding))
        .expect("clean restart resumes");
    assert_eq!(ready.state, AutomationBootRecoveryState::Ready);
    assert_eq!(ready.recovery_digest, ready.digest());
    assert!(ready.validate().is_ok());
}

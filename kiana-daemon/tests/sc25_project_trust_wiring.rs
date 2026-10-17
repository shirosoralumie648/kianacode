//! SC-25 daemon wiring: the production [`StoredProjectTrustAuthority`] resolves project
//! trust through `kiana-policy::ProjectTrustResolution`, and the `DaemonHost` gate exposes
//! the policy-level load audit. Deny paths are proven before the allow path.
//!
//! `KIANA_HOME` is process-global and read on every trust lookup, so the fixtures below
//! serialize on a file-local lock and restore the variable on drop.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use kiana_daemon::{DaemonHost, ProjectTrustAuthority, StoredProjectTrustAuthority};
use kiana_ports::PortError;

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct IsolatedTrustHome {
    _lock: MutexGuard<'static, ()>,
    original: Option<OsString>,
    root: PathBuf,
    project: PathBuf,
    other_project: PathBuf,
}

impl IsolatedTrustHome {
    fn new(label: &str) -> Self {
        let lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("kiana-sc25-{label}-{}-{stamp}", std::process::id()));
        let home = root.join("kiana-home");
        fs::create_dir_all(&home).expect("kiana home");
        let project = root.join("project");
        fs::create_dir_all(project.join(".git")).expect("project fixture");
        let other_project = root.join("other-project");
        fs::create_dir_all(other_project.join(".git")).expect("other project fixture");

        let original = std::env::var_os("KIANA_HOME");
        std::env::set_var("KIANA_HOME", &home);
        Self {
            _lock: lock,
            original,
            root,
            project,
            other_project,
        }
    }
}

impl Drop for IsolatedTrustHome {
    fn drop(&mut self) {
        match self.original.as_ref() {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn daemon_host(authority: Arc<dyn ProjectTrustAuthority>) -> DaemonHost {
    DaemonHost::with_harness_and_project_authority(
        kiana_runner::KianaHarness::new(Arc::new(kiana_runner::UnavailableModel::default())),
        authority,
    )
    .expect("fixture daemon host")
}

fn port_reason(error: PortError) -> String {
    match error {
        PortError::Failed(reason) => reason,
        other => panic!("unexpected port error: {other}"),
    }
}

#[test]
fn stored_authority_deny_first_reasons_and_bool_compatibility() {
    let env = IsolatedTrustHome::new("reasons");
    let authority = StoredProjectTrustAuthority;
    let project = env.project.as_path();

    // 缺失记录：旧布尔语义是 false；策略拒绝原因是 missing，而不是伪造的 unknown。
    assert!(!authority.project_trusted(project).unwrap());
    let missing = authority
        .project_trust_resolution(project)
        .unwrap()
        .expect("stored authority always resolves");
    assert_eq!(missing.reason, "project_trust_root_missing");
    assert!(missing.candidates.is_empty());
    missing.validate().unwrap();

    // 明确不可信：稳定原因切换为 untrusted。
    kiana_types::write_project_trust(project, kiana_types::ProjectTrust::Untrusted).unwrap();
    assert!(!authority.project_trusted(project).unwrap());
    let untrusted = authority
        .project_trust_resolution(project)
        .unwrap()
        .unwrap();
    assert_eq!(untrusted.reason, "project_trust_untrusted");

    // 另一个项目没有记录：一个项目的判定不能外溢到别的项目。
    assert!(!authority.project_trusted(&env.other_project).unwrap());
    let other = authority
        .project_trust_resolution(&env.other_project)
        .unwrap()
        .unwrap();
    assert_eq!(other.reason, "project_trust_root_missing");

    // 改为可信：Allow，且命中记录带稳定 scope 与自校验摘要。
    kiana_types::write_project_trust(project, kiana_types::ProjectTrust::Trusted).unwrap();
    assert!(authority.project_trusted(project).unwrap());
    let trusted = authority
        .project_trust_resolution(project)
        .unwrap()
        .unwrap();
    assert_eq!(trusted.reason, "project_trust_allowed");
    assert_eq!(
        trusted.selected_scope,
        Some(kiana_policy::ProjectTrustScope::Project)
    );
    trusted.validate().unwrap();
}

#[test]
fn daemon_gate_denies_before_allow_and_keeps_policy_reasons() {
    let env = IsolatedTrustHome::new("host-gate");
    let host = daemon_host(Arc::new(StoredProjectTrustAuthority));
    let project_root = env.project.to_str().expect("utf-8 project root");

    // 缺失记录：放行视图拒绝，原因来自策略层。
    let error = host.authorize_project_trust_load(project_root).unwrap_err();
    assert_eq!(port_reason(error), "project_trust_root_missing");

    // 审计视图仍产出记录：Deny 也是"决定已作出"。
    let deny_audit = host.project_trust_load_audit(project_root).unwrap();
    assert_eq!(
        deny_audit.decision,
        kiana_policy::ProjectTrustDecision::Deny
    );
    assert_eq!(deny_audit.reason, "project_trust_root_missing");
    assert_eq!(
        deny_audit.requested_scope,
        kiana_policy::ProjectTrustScope::Project
    );
    deny_audit.validate().unwrap();

    // 明确不可信：原因切换为 untrusted。
    kiana_types::write_project_trust(&env.project, kiana_types::ProjectTrust::Untrusted).unwrap();
    let error = host.authorize_project_trust_load(project_root).unwrap_err();
    assert_eq!(port_reason(error), "project_trust_untrusted");

    // 可信：放行并返回 Allow 审计（命中 scope 就是请求 scope）。
    kiana_types::write_project_trust(&env.project, kiana_types::ProjectTrust::Trusted).unwrap();
    let allow_audit = host.authorize_project_trust_load(project_root).unwrap();
    assert_eq!(
        allow_audit.decision,
        kiana_policy::ProjectTrustDecision::Allow
    );
    assert_eq!(
        allow_audit.selected_scope,
        Some(kiana_policy::ProjectTrustScope::Project)
    );
    assert_eq!(allow_audit.reason, "project_trust_allowed");
    allow_audit.validate().unwrap();
    // 审计同时绑定项目摘要与资源根摘要，两者都是 sha256 摘要且互不相同。
    assert!(allow_audit.project_root_digest.starts_with("sha256:"));
    assert!(allow_audit.resource_root_digest.starts_with("sha256:"));
    assert_ne!(
        allow_audit.resource_root_digest,
        allow_audit.project_root_digest
    );
}

struct BoolOnlyAuthority {
    trusted: bool,
}

impl ProjectTrustAuthority for BoolOnlyAuthority {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(self.trusted)
    }
}

#[test]
fn daemon_gate_refuses_to_fallback_when_the_authority_has_no_policy_resolution() {
    let env = IsolatedTrustHome::new("no-policy");
    let host = daemon_host(Arc::new(BoolOnlyAuthority { trusted: true }));
    let project_root = env.project.to_str().expect("utf-8 project root");

    // 旧布尔闸门仍可用……
    let snapshot = host.project_trust_snapshot(project_root, 1).unwrap();
    assert!(snapshot.trusted);
    // ……但新装载闸门不拿布尔值猜策略结论：没有判定就 fail-closed。
    let error = host.project_trust_load_audit(project_root).unwrap_err();
    assert_eq!(port_reason(error), "project_trust_policy_unavailable");
}

#[test]
fn sc25_daemon_wiring_and_lane_are_pinned() {
    let daemon = include_str!("../src/lib.rs");
    for marker in [
        "StoredProjectTrustAuthority",
        "ProjectTrustResolution",
        "ProjectTrustLoadAudit",
        "project_trust_resolution",
        "project_trust_load_audit",
        "authorize_project_trust_load",
        "PROJECT_TRUST_AUDIT_REF",
        "PROJECT_TRUST_RECORD_REVISION",
        "ProjectTrustScope::Project",
        "allows_loading",
        "project_trust_policy_unavailable",
        "Ok(self.resolve_project_trust(project_root)?.allows_loading())",
    ] {
        assert!(
            daemon.contains(marker),
            "missing SC-25 daemon marker: {marker}"
        );
    }
    // 生产布尔闸门必须经策略判定，不得退回旧的直接布尔读取。
    assert!(!daemon.contains("is_some_and(kiana_types::ProjectTrust::as_bool)"));

    let fixtures = [
        "stored_authority_deny_first_reasons_and_bool_compatibility",
        "daemon_gate_denies_before_allow_and_keeps_policy_reasons",
        "daemon_gate_refuses_to_fallback_when_the_authority_has_no_policy_resolution",
    ];
    let workflow = include_str!("../../.github/workflows/sc25-project-trust.yml");
    let baseline = include_str!("../../docs/roadmap/sc25-project-trust-baseline.md");
    for fixture in fixtures {
        assert!(include_str!("sc25_project_trust_wiring.rs").contains(fixture));
        assert!(
            workflow.contains(fixture),
            "lane must run fixture: {fixture}"
        );
        assert!(
            baseline.contains(fixture),
            "baseline must name fixture: {fixture}"
        );
    }
    assert!(workflow.contains("cargo test -p kiana-daemon --test sc25_project_trust_wiring"));
    assert!(workflow.contains("cargo test -p kiana-policy --test sc25_project_trust"));
    assert!(workflow.contains("--test-threads=1"));
    assert!(!workflow.contains("continue-on-error"));
    assert!(!workflow.contains("ignore"));
}

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use kiana_daemon::LocalModelConfig;
use kiana_provider::{ProviderConfig, ProviderGateway, WorkspaceConfigTrust};

#[test]
fn config_resolver_is_the_only_product_parser() {
    let daemon = include_str!("../src/model_client.rs");
    let provider = include_str!("../../kiana-provider/src/resolver.rs");
    let gateway = include_str!("../../kiana-provider/src/lib.rs");
    let baseline = include_str!("../../docs/roadmap/config-credentials-identity-baseline.md");
    let fixture = include_str!("../../kiana-provider/tests/ci06_config_resolver.rs");
    for marker in [
        "ConfigResolver",
        "WORKSPACE_CONFIG_SCHEMA",
        "parse_workspace",
        "workspace_snapshot",
        "ConfigSnapshot",
        "ProviderConfigSnapshot",
        "configuration_revision",
        "contains_forbidden_provider_config_control",
        "config_workspace_untrusted",
        "config_workspace_schema_unsupported",
        "config_workspace_invalid",
        "config_endpoint_credentials_or_query_denied",
        "config_endpoint_requires_tls_or_loopback",
        "config_profile_invalid",
        "MAX_WORKSPACE_CONFIG_BYTES",
        "deny_unknown_fields",
        "legacy_fixtures",
        "config_resolver_produces_a_canonical_secret_free_snapshot",
    ] {
        assert!(
            daemon.contains(marker)
                || provider.contains(marker)
                || gateway.contains(marker)
                || baseline.contains(marker)
                || fixture.contains(marker),
            "config resolver marker missing: {marker}"
        );
    }
    assert!(gateway.contains("resolver::ConfigResolver::resolve(config)"));
    assert!(daemon.contains("#[cfg(test)]\nmod legacy_fixtures"));
    assert!(provider.contains("project_trusted"));
    assert!(provider.contains("parse_bounded_json(raw.as_bytes())"));
    assert!(provider.contains("serde_json::from_value(value)"));
    assert!(provider.contains("contains_forbidden_provider_config_control"));
    assert!(!gateway.contains("config::connections(config)"));
    assert!(!provider.contains("api_key: String"));

    let project_config = include_str!("../src/project_provider_config.rs");
    let lib = include_str!("../src/lib.rs");
    for marker in [
        "PROJECT_PROVIDER_CONFIG_RELATIVE_PATH",
        "project_provider_config_path",
        "gateway_from_project_config",
        "config_workspace_untrusted",
        "config_workspace_too_large",
        "config_workspace_not_regular_file",
        "ProviderGateway::from_workspace",
        "read_bounded_project_config",
    ] {
        assert!(
            project_config.contains(marker),
            "daemon project provider config marker missing: {marker}"
        );
    }
    assert!(lib.contains("pub mod project_provider_config;"));
    assert!(!project_config.contains("serde_json::from"));
    assert!(!project_config.contains("config::connections"));
}

#[test]
fn local_model_config_debug_redacts_key_and_endpoint_credentials() {
    let config = kiana_daemon::LocalModelConfig {
        provider: Some("ollama".to_owned()),
        model: Some("local-diagnostic-model".to_owned()),
        api_key: Some("CI06_DEBUG_KEY_SENTINEL".to_owned()),
        base_url: Some(
            "https://CI06_USERINFO_SENTINEL@example.invalid/api?token=CI06_QUERY_SENTINEL#CI06_FRAGMENT_SENTINEL"
                .to_owned(),
        ),
    };

    for formatted in [format!("{config:?}"), format!("{config:#?}")] {
        for sentinel in [
            "CI06_DEBUG_KEY_SENTINEL",
            "CI06_USERINFO_SENTINEL",
            "CI06_QUERY_SENTINEL",
            "CI06_FRAGMENT_SENTINEL",
        ] {
            assert!(
                !formatted.contains(sentinel),
                "Debug must not expose a configured credential"
            );
        }
        assert!(formatted.contains("ollama"));
        assert!(formatted.contains("local-diagnostic-model"));
        assert!(formatted.contains("api_key"));
        assert!(formatted.contains("base_url"));
        assert!(formatted.contains("[REDACTED]"));
        assert!(!formatted.contains("https://"));
    }
}

#[test]
fn local_model_config_debug_preserves_unconfigured_options() {
    let config = kiana_daemon::LocalModelConfig::default();

    for formatted in [format!("{config:?}"), format!("{config:#?}")] {
        for field in ["provider", "model", "api_key", "base_url"] {
            assert!(
                formatted.contains(&format!("{field}: None")),
                "Debug must distinguish an unconfigured option"
            );
        }
        assert!(!formatted.contains("[REDACTED]"));
    }
}

#[test]
fn local_model_config_debug_marks_empty_sensitive_values_as_configured() {
    let config = kiana_daemon::LocalModelConfig {
        api_key: Some(String::new()),
        base_url: Some(String::new()),
        ..kiana_daemon::LocalModelConfig::default()
    };

    for formatted in [format!("{config:?}"), format!("{config:#?}")] {
        assert!(!formatted.contains("api_key: None"));
        assert!(!formatted.contains("base_url: None"));
        assert_eq!(formatted.matches("[REDACTED]").count(), 2);
    }
}

// ---------------------------------------------------------------------------
// Daemon project provider config wiring (CI-06)
//
// The daemon resolves `<project_root>/.kiana/provider-config.json` through the single
// `ConfigResolver` (via `ProviderGateway::from_workspace`). Trust gates the file read: an
// untrusted project must deny before any content is opened, and the same input must produce
// the same canonical snapshot as the direct provider entrypoint.
// ---------------------------------------------------------------------------

fn temp_project(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-ci06-project-{label}-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir_all(root.join(".kiana")).expect("project fixture");
    root
}

fn project_overlay(model: &str) -> String {
    serde_json::json!({
        "schema": "kiana.provider-config.v1",
        "version": 1,
        "provider": "ollama",
        "model": model,
        "base_url": "http://127.0.0.1:22114"
    })
    .to_string()
}

fn project_revision(trusted: bool) -> String {
    // Same derivation the daemon authority boundary uses for ProjectIdentity.trust_revision.
    kiana_domain::json_digest(&serde_json::json!({"trusted": trusted}))
}

fn trust_facts(trusted: bool, revision: &str) -> WorkspaceConfigTrust<'_> {
    WorkspaceConfigTrust {
        project_trusted: trusted,
        admitted_revision: revision,
        current_revision: revision,
    }
}

fn resolve_project(
    root: &Path,
    trust: WorkspaceConfigTrust<'_>,
) -> Result<Option<ProviderGateway>, String> {
    kiana_daemon::project_provider_config::gateway_from_project_config(
        LocalModelConfig::default(),
        root,
        trust,
    )
}

fn resolve_error(root: &Path, trust: WorkspaceConfigTrust<'_>) -> String {
    match resolve_project(root, trust) {
        Ok(_) => panic!("project provider config must fail closed"),
        Err(error) => error,
    }
}

/// Provider-affecting environment variables. The resolver reads process environment, so these
/// fixtures clear them like the provider CI-06 fixture does (the target runs with one test
/// thread); a dirty host environment must not flip precedence or fail construction.
const PROVIDER_ENV_VARS: [&str; 14] = [
    "KIANA_PROVIDER",
    "KIANA_MODEL",
    "KIANA_BASE_URL",
    "KIANA_MODEL_PROFILES_JSON",
    "KIANA_OLLAMA_MODEL",
    "KIANA_OLLAMA_BASE_URL",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_API_KEY",
    "OPENAI_MODEL",
    "OPENAI_BASE_URL",
    "OPENAI_API_KEY",
    "KIANA_MODEL_MAX_CONCURRENCY",
    "KIANA_MODEL_QUEUE_LIMIT",
];

struct EnvRestore(Vec<(&'static str, Option<OsString>)>);

impl EnvRestore {
    fn new() -> Self {
        let values = PROVIDER_ENV_VARS
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect::<Vec<_>>();
        for (key, _) in &values {
            std::env::remove_var(key);
        }
        Self(values)
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

#[test]
fn project_provider_config_denies_untrusted_before_reading_content() {
    let _env = EnvRestore::new();
    let root = temp_project("untrusted");
    let path = kiana_daemon::project_provider_config::project_provider_config_path(&root);
    assert_eq!(path, root.join(".kiana/provider-config.json"));
    fs::write(&path, project_overlay("ci06-project-model")).expect("project overlay");

    // The same bytes are admitted once ProjectTrust is established...
    let trusted = resolve_project(&root, trust_facts(true, &project_revision(true)))
        .expect("trusted overlay")
        .expect("project overlay exists");
    let snapshot = trusted.configuration_snapshot().expect("snapshot");
    let default = snapshot
        .profiles
        .iter()
        .find(|profile| profile.profile == "default")
        .expect("default profile");
    assert_eq!(default.route.model_id, "ci06-project-model");

    // ...while an untrusted project denies before any read. A directory or an oversized file at
    // the overlay path would produce an open/read/size error if the gate ran after opening.
    fs::remove_file(&path).expect("remove overlay file");
    fs::create_dir(&path).expect("directory at overlay path");
    let error = resolve_error(&root, trust_facts(false, &project_revision(false)));
    assert_eq!(error, "config_workspace_untrusted");
    fs::remove_dir(&path).expect("remove overlay directory");
    fs::write(&path, "x".repeat(kiana_provider::MAX_WORKSPACE_CONFIG_BYTES + 1))
        .expect("oversized overlay");
    let error = resolve_error(&root, trust_facts(false, &project_revision(false)));
    assert_eq!(error, "config_workspace_untrusted");
}

#[test]
fn project_provider_config_keeps_environment_path_when_overlay_is_absent() {
    let root = temp_project("absent");
    for trusted in [true, false] {
        let resolution = resolve_project(&root, trust_facts(trusted, &project_revision(trusted)))
            .expect("missing overlay is not a failure");
        assert!(resolution.is_none(), "no project resource exists to load");
    }
}

#[test]
fn project_provider_config_bounds_reads_before_the_resolver() {
    let root = temp_project("oversized");
    let path = kiana_daemon::project_provider_config::project_provider_config_path(&root);
    fs::write(&path, "x".repeat(kiana_provider::MAX_WORKSPACE_CONFIG_BYTES + 1))
        .expect("oversized overlay");
    let error = resolve_error(&root, trust_facts(true, &project_revision(true)));
    assert_eq!(error, "config_workspace_too_large");
}

#[test]
fn project_provider_config_rejects_revision_drift_before_parsing() {
    let root = temp_project("revision-drift");
    let path = kiana_daemon::project_provider_config::project_provider_config_path(&root);
    fs::write(&path, "not JSON at all").expect("invalid overlay");
    let admitted = project_revision(true);
    let current = project_revision(false);
    let error = resolve_error(
        &root,
        WorkspaceConfigTrust {
            project_trusted: true,
            admitted_revision: &admitted,
            current_revision: &current,
        },
    );
    assert_eq!(error, "config_project_trust_revision_changed");
}

#[test]
fn project_provider_config_matches_the_provider_entrypoint_snapshot() {
    let _env = EnvRestore::new();
    let root = temp_project("canonical");
    let raw = project_overlay("ci06-project-model");
    let path = kiana_daemon::project_provider_config::project_provider_config_path(&root);
    fs::write(&path, &raw).expect("project overlay");
    let revision = project_revision(true);

    let daemon_gateway = resolve_project(&root, trust_facts(true, &revision))
        .expect("trusted overlay")
        .expect("project overlay exists");
    let provider_gateway = ProviderGateway::from_workspace(
        ProviderConfig::default(),
        &raw,
        trust_facts(true, &revision),
    )
    .expect("provider entrypoint");
    assert_eq!(
        daemon_gateway.configuration_snapshot().unwrap(),
        provider_gateway.configuration_snapshot().unwrap(),
        "same input must produce one canonical snapshot on every entrypoint"
    );
    assert_eq!(
        daemon_gateway.workspace_configuration_snapshot().unwrap(),
        provider_gateway.workspace_configuration_snapshot().unwrap()
    );
    let workspace = daemon_gateway
        .workspace_configuration_snapshot()
        .unwrap()
        .expect("secret-free workspace snapshot");
    assert_eq!(workspace.project_trust_revision, revision);

    let repeated = resolve_project(&root, trust_facts(true, &revision))
        .expect("trusted overlay")
        .expect("project overlay exists");
    assert_eq!(
        repeated.configuration_snapshot().unwrap(),
        daemon_gateway.configuration_snapshot().unwrap()
    );
}

use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const LICENSE_SCHEMA: &str = "kiana.license-status.v1";

pub struct LicenseCommand;

#[async_trait]
impl Command for LicenseCommand {
    fn name(&self) -> &str {
        "license"
    }

    fn description(&self) -> &str {
        "Inspect enterprise license status"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let (command, rest) = split_word(context.args.trim());
        match command.unwrap_or("status") {
            "" | "status" => license_status(rest),
            "help" | "--help" | "-h" if rest.is_empty() => Ok(CommandResult::text(usage())),
            other => Err(anyhow!(
                "unknown license command '{}'\n\n{}",
                other,
                usage()
            )),
        }
    }
}

fn license_status(rest: &str) -> anyhow::Result<CommandResult> {
    match rest.trim() {
        "" | "--text" => Ok(CommandResult::text(license_status_text()?)),
        "--json" => Ok(CommandResult::text(license_status_json()?)),
        "help" | "--help" | "-h" => Ok(CommandResult::text(status_usage())),
        other => Err(anyhow!(
            "unknown license status option '{}'\n\n{}",
            other,
            status_usage()
        )),
    }
}

fn license_status_text() -> anyhow::Result<String> {
    let status = license_status_report();
    let mut lines = vec![
        "License status".to_string(),
        format!("schema: {}", status.schema),
        format!("status: {}", status.status),
        format!("source: {}", status.source),
        format!("license_key: {}", status.license_key),
        format!(
            "account_id: {}",
            status.account_id.as_deref().unwrap_or("none")
        ),
        format!("plan: {}", status.plan.as_deref().unwrap_or("none")),
        format!("subject: {}", status.subject.as_deref().unwrap_or("none")),
        format!("issuer: {}", status.issuer.as_deref().unwrap_or("none")),
        format!(
            "expires_at: {}",
            status.expires_at.as_deref().unwrap_or("none")
        ),
        format!("offline: {}", yes_no(status.offline)),
        format!(
            "entitlements: {}",
            if status.entitlements.is_empty() {
                "none".to_string()
            } else {
                status.entitlements.join(",")
            }
        ),
        format!(
            "managed_policy_file: {}",
            status.managed_policy_file.as_deref().unwrap_or("none")
        ),
        format!(
            "support_contact: {}",
            status.support_contact.as_deref().unwrap_or("none")
        ),
    ];
    if !status.issues.is_empty() {
        lines.push(format!("issues: {}", status.issues.join("; ")));
    }
    lines.push("usage: kiana license status [--json|--text]".to_string());
    Ok(lines.join("\n"))
}

fn license_status_json() -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(&license_status_report())?)
}

fn license_status_report() -> LicenseStatusReport {
    let mut report = LicenseStatusReport {
        schema: LICENSE_SCHEMA.to_string(),
        status: "missing".to_string(),
        source: "none".to_string(),
        license_key: "missing".to_string(),
        license_key_preview: None,
        account_id: None,
        plan: None,
        subject: None,
        issuer: None,
        expires_at: None,
        offline: false,
        entitlements: Vec::new(),
        managed_policy_file: managed_policy_file_label(),
        support_contact: std::env::var("KIANA_SUPPORT_CONTACT")
            .ok()
            .and_then(|value| non_empty(Some(value))),
        issues: vec!["no enterprise license configured".to_string()],
    };

    if let Some(path) = std::env::var_os("KIANA_LICENSE_FILE").map(PathBuf::from) {
        apply_license_file(&mut report, &path);
    }
    apply_env_license_overrides(&mut report);
    finalize_report(&mut report);
    report
}

#[derive(Debug, Clone, Serialize)]
struct LicenseStatusReport {
    schema: String,
    status: String,
    source: String,
    license_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    license_key_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plan: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    issuer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<String>,
    offline: bool,
    entitlements: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    managed_policy_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    support_contact: Option<String>,
    issues: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct LicenseFile {
    #[serde(default, alias = "licenseKey", alias = "key")]
    license_key: Option<String>,
    #[serde(default, alias = "accountId", alias = "account")]
    account_id: Option<String>,
    #[serde(default)]
    plan: Option<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    issuer: Option<String>,
    #[serde(default, alias = "expiresAt", alias = "expiry")]
    expires_at: Option<String>,
    #[serde(default)]
    offline: Option<bool>,
    #[serde(default)]
    entitlements: Vec<String>,
    #[serde(default, alias = "managedPolicyFile")]
    managed_policy_file: Option<String>,
    #[serde(default, alias = "supportContact")]
    support_contact: Option<String>,
}

fn apply_license_file(report: &mut LicenseStatusReport, path: &Path) {
    report.source = format!("file:{}", path.display());
    match std::fs::read_to_string(path) {
        Ok(contents) => match serde_json::from_str::<LicenseFile>(&contents) {
            Ok(file) => apply_license_file_values(report, file),
            Err(error) => {
                report.status = "invalid".to_string();
                report.issues = vec![format!("failed to parse license file: {error}")];
            }
        },
        Err(error) => {
            report.status = "invalid".to_string();
            report.issues = vec![format!("failed to read license file: {error}")];
        }
    }
}

fn apply_license_file_values(report: &mut LicenseStatusReport, file: LicenseFile) {
    if let Some(key) = non_empty(file.license_key) {
        set_license_key(report, key, "file");
    }
    if let Some(account_id) = non_empty(file.account_id) {
        report.account_id = Some(account_id);
    }
    if let Some(plan) = non_empty(file.plan) {
        report.plan = Some(plan);
    }
    if let Some(subject) = non_empty(file.subject) {
        report.subject = Some(subject);
    }
    if let Some(issuer) = non_empty(file.issuer) {
        report.issuer = Some(issuer);
    }
    if let Some(expires_at) = non_empty(file.expires_at) {
        report.expires_at = Some(expires_at);
    }
    if let Some(offline) = file.offline {
        report.offline = offline;
    }
    report.entitlements = clean_list(file.entitlements);
    if let Some(managed_policy_file) = non_empty(file.managed_policy_file) {
        report.managed_policy_file = Some(managed_policy_file);
    }
    if let Some(support_contact) = non_empty(file.support_contact) {
        report.support_contact = Some(support_contact);
    }
}

fn apply_env_license_overrides(report: &mut LicenseStatusReport) {
    if let Ok(key) = std::env::var("KIANA_LICENSE_KEY") {
        if let Some(key) = non_empty(Some(key)) {
            set_license_key(report, key, "KIANA_LICENSE_KEY");
        }
    }
    if let Ok(account_id) = std::env::var("KIANA_ENTERPRISE_ACCOUNT_ID") {
        if let Some(account_id) = non_empty(Some(account_id)) {
            report.account_id = Some(account_id);
        }
    }
    if let Ok(plan) = std::env::var("KIANA_LICENSE_PLAN") {
        if let Some(plan) = non_empty(Some(plan)) {
            report.plan = Some(plan);
        }
    }
    if let Ok(subject) = std::env::var("KIANA_LICENSE_SUBJECT") {
        if let Some(subject) = non_empty(Some(subject)) {
            report.subject = Some(subject);
        }
    }
    if let Ok(issuer) = std::env::var("KIANA_LICENSE_ISSUER") {
        if let Some(issuer) = non_empty(Some(issuer)) {
            report.issuer = Some(issuer);
        }
    }
    if let Ok(expires_at) = std::env::var("KIANA_LICENSE_EXPIRES_AT") {
        if let Some(expires_at) = non_empty(Some(expires_at)) {
            report.expires_at = Some(expires_at);
        }
    }
    if let Ok(offline) = std::env::var("KIANA_LICENSE_OFFLINE") {
        report.offline = matches!(
            offline.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        );
    }
    if let Ok(entitlements) = std::env::var("KIANA_LICENSE_ENTITLEMENTS") {
        report.entitlements = clean_list(split_csv(&entitlements));
    }
}

fn set_license_key(report: &mut LicenseStatusReport, key: String, source: &str) {
    report.license_key = "set".to_string();
    report.license_key_preview = Some(redacted_preview(&key));
    if report.source == "none" || source == "KIANA_LICENSE_KEY" {
        report.source = source.to_string();
    }
}

fn finalize_report(report: &mut LicenseStatusReport) {
    if report.status == "invalid" {
        return;
    }
    if report.license_key == "missing" {
        report.status = "missing".to_string();
        report.issues = vec!["no enterprise license configured".to_string()];
        return;
    }

    let mut issues = Vec::new();
    if report.account_id.is_none() {
        issues.push("enterprise account id is not configured".to_string());
    }
    if report.plan.is_none() {
        issues.push("license plan is not configured".to_string());
    }
    if report.entitlements.is_empty() {
        issues.push("license entitlements are not configured".to_string());
    }
    report.status = if issues.is_empty() {
        "configured".to_string()
    } else {
        "partial".to_string()
    };
    report.issues = issues;
}

fn managed_policy_file_label() -> Option<String> {
    std::env::var_os("KIANA_MANAGED_POLICY_FILE")
        .or_else(|| std::env::var_os("KIANA_MANAGED_SETTINGS_FILE"))
        .map(PathBuf::from)
        .map(|path| {
            format!(
                "{} ({})",
                path.display(),
                if path.is_file() { "found" } else { "missing" }
            )
        })
}

fn redacted_preview(secret: &str) -> String {
    let trimmed = secret.trim();
    let suffix: String = trimmed
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if suffix.is_empty() {
        "redacted".to_string()
    } else {
        format!("redacted-{}", suffix)
    }
}

fn split_csv(input: &str) -> Vec<String> {
    input.split(',').map(str::to_string).collect()
}

fn clean_list(values: Vec<String>) -> Vec<String> {
    let mut cleaned = values
        .into_iter()
        .filter_map(|value| non_empty(Some(value)))
        .collect::<Vec<_>>();
    cleaned.sort();
    cleaned.dedup();
    cleaned
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn usage() -> &'static str {
    "Usage: kiana license [status]\n       kiana license status [--json|--text]"
}

fn status_usage() -> &'static str {
    "Usage: kiana license status [--json|--text]"
}

fn split_word(input: &str) -> (Option<&str>, &str) {
    let input = input.trim();
    if input.is_empty() {
        return (None, "");
    }
    match input.find(char::is_whitespace) {
        Some(index) => (Some(&input[..index]), input[index..].trim()),
        None => (Some(input), ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_state::env_lock;
    use serde_json::json;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct EnvGuard {
        values: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn set(values: &[(&'static str, Option<&str>)]) -> Self {
            let previous = values
                .iter()
                .map(|(key, _)| (*key, std::env::var(key).ok()))
                .collect::<Vec<_>>();
            for (key, value) in values {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
            Self { values: previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.values {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    fn clear_license_env() -> EnvGuard {
        EnvGuard::set(&[
            ("KIANA_LICENSE_FILE", None),
            ("KIANA_LICENSE_KEY", None),
            ("KIANA_LICENSE_PLAN", None),
            ("KIANA_LICENSE_SUBJECT", None),
            ("KIANA_LICENSE_ISSUER", None),
            ("KIANA_LICENSE_EXPIRES_AT", None),
            ("KIANA_LICENSE_OFFLINE", None),
            ("KIANA_LICENSE_ENTITLEMENTS", None),
            ("KIANA_ENTERPRISE_ACCOUNT_ID", None),
            ("KIANA_SUPPORT_CONTACT", None),
            ("KIANA_MANAGED_POLICY_FILE", None),
            ("KIANA_MANAGED_SETTINGS_FILE", None),
        ])
    }

    fn temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-license-{name}-{}-{unique}.json",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn license_status_json_reports_missing_by_default() {
        let _lock = env_lock().lock().unwrap();
        let _guard = clear_license_env();

        let result = LicenseCommand
            .execute(CommandContext {
                args: "status --json".to_string(),
                app_state: Default::default(),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], LICENSE_SCHEMA);
        assert_eq!(value["status"], "missing");
        assert_eq!(value["source"], "none");
        assert_eq!(value["license_key"], "missing");
        assert!(value["issues"]
            .as_array()
            .unwrap()
            .contains(&json!("no enterprise license configured")));
    }

    #[tokio::test]
    async fn license_status_json_reports_file_without_leaking_key() {
        let _lock = env_lock().lock().unwrap();
        let path = temp_path("configured");
        fs::write(
            &path,
            json!({
                "licenseKey": "kiana-enterprise-secret-1234",
                "accountId": "acct_123",
                "plan": "enterprise",
                "subject": "Acme",
                "issuer": "Kiana",
                "offline": true,
                "entitlements": ["managed-policy", "offline", "managed-policy"],
                "supportContact": "security@example.test"
            })
            .to_string(),
        )
        .unwrap();
        let path_str = path.to_string_lossy().to_string();
        let _guard = EnvGuard::set(&[
            ("KIANA_LICENSE_FILE", Some(&path_str)),
            ("KIANA_LICENSE_KEY", None),
            ("KIANA_LICENSE_PLAN", None),
            ("KIANA_LICENSE_SUBJECT", None),
            ("KIANA_LICENSE_ISSUER", None),
            ("KIANA_LICENSE_EXPIRES_AT", None),
            ("KIANA_LICENSE_OFFLINE", None),
            ("KIANA_LICENSE_ENTITLEMENTS", None),
            ("KIANA_ENTERPRISE_ACCOUNT_ID", None),
            ("KIANA_SUPPORT_CONTACT", None),
            ("KIANA_MANAGED_POLICY_FILE", None),
            ("KIANA_MANAGED_SETTINGS_FILE", None),
        ]);

        let output = license_status_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(value["schema"], LICENSE_SCHEMA);
        assert_eq!(value["status"], "configured");
        assert_eq!(value["source"], format!("file:{path_str}"));
        assert_eq!(value["license_key"], "set");
        assert_eq!(value["license_key_preview"], "redacted-1234");
        assert_eq!(value["account_id"], "acct_123");
        assert_eq!(value["plan"], "enterprise");
        assert_eq!(value["offline"], true);
        assert_eq!(value["entitlements"], json!(["managed-policy", "offline"]));
        assert_eq!(value["support_contact"], "security@example.test");
        assert_eq!(value["issues"], json!([]));
        assert!(!output.contains("kiana-enterprise-secret"));

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn license_status_text_reports_partial_env_license() {
        let _lock = env_lock().lock().unwrap();
        let _guard = EnvGuard::set(&[
            ("KIANA_LICENSE_FILE", None),
            ("KIANA_LICENSE_KEY", Some("env-secret-9999")),
            ("KIANA_LICENSE_PLAN", Some("team")),
            ("KIANA_LICENSE_SUBJECT", None),
            ("KIANA_LICENSE_ISSUER", None),
            ("KIANA_LICENSE_EXPIRES_AT", None),
            ("KIANA_LICENSE_OFFLINE", Some("true")),
            ("KIANA_LICENSE_ENTITLEMENTS", Some("audit,policy")),
            ("KIANA_ENTERPRISE_ACCOUNT_ID", None),
            ("KIANA_SUPPORT_CONTACT", None),
            ("KIANA_MANAGED_POLICY_FILE", None),
            ("KIANA_MANAGED_SETTINGS_FILE", None),
        ]);

        let output = license_status_text().unwrap();

        assert!(output.contains("License status"));
        assert!(output.contains("status: partial"));
        assert!(output.contains("source: KIANA_LICENSE_KEY"));
        assert!(output.contains("license_key: set"));
        assert!(output.contains("offline: yes"));
        assert!(output.contains("entitlements: audit,policy"));
        assert!(output.contains("enterprise account id is not configured"));
        assert!(!output.contains("env-secret-9999"));
    }

    #[tokio::test]
    async fn license_status_json_reports_invalid_file() {
        let _lock = env_lock().lock().unwrap();
        let path = temp_path("invalid");
        fs::write(&path, "{not-json").unwrap();
        let path_str = path.to_string_lossy().to_string();
        let _guard = EnvGuard::set(&[
            ("KIANA_LICENSE_FILE", Some(&path_str)),
            ("KIANA_LICENSE_KEY", None),
            ("KIANA_LICENSE_PLAN", None),
            ("KIANA_LICENSE_SUBJECT", None),
            ("KIANA_LICENSE_ISSUER", None),
            ("KIANA_LICENSE_EXPIRES_AT", None),
            ("KIANA_LICENSE_OFFLINE", None),
            ("KIANA_LICENSE_ENTITLEMENTS", None),
            ("KIANA_ENTERPRISE_ACCOUNT_ID", None),
            ("KIANA_SUPPORT_CONTACT", None),
            ("KIANA_MANAGED_POLICY_FILE", None),
            ("KIANA_MANAGED_SETTINGS_FILE", None),
        ]);

        let output = license_status_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(value["status"], "invalid");
        assert_eq!(value["license_key"], "missing");
        assert!(value["issues"][0]
            .as_str()
            .unwrap()
            .contains("failed to parse license file"));

        let _ = fs::remove_file(path);
    }
}

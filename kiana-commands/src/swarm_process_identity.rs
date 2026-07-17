use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const PROCESS_IDENTITY_SCHEMA: &str = "kiana.swarm-process-identity.v1";
const PROCESS_IDENTITY_BACKEND_SCHEMA: &str = "kiana.swarm-process-identity-backend.v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct WorkerProcessIdentity {
    pub schema: String,
    pub platform: String,
    pub pid: u32,
    pub process_group_id: u32,
    pub start_time_ticks: u64,
    pub command_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProcessIdentityStatus {
    Verified,
    Exited,
    Missing,
    Mismatch(Vec<String>),
    Unavailable(String),
}

pub(crate) fn process_identity_backend_capability() -> Value {
    #[cfg(target_os = "linux")]
    {
        return json!({
            "schema": PROCESS_IDENTITY_BACKEND_SCHEMA,
            "platform": std::env::consts::OS,
            "backend": "linux_procfs",
            "supported": true,
            "safe_to_start_workers": true,
            "safe_to_monitor_workers": true,
            "safe_to_cancel_workers": true,
            "identity_fields": [
                "pid",
                "process_group_id",
                "start_time_ticks",
                "command_sha256"
            ],
            "continuity_fields": [
                "pid",
                "process_group_id",
                "start_time_ticks"
            ],
            "provenance_fields": ["command_sha256"],
            "unsupported_reason": Value::Null,
            "notes": [
                "command_sha256 is provenance only and is not used as process continuity evidence",
                "workers must be their own process group leaders before cancellation is allowed"
            ]
        });
    }
    #[cfg(not(target_os = "linux"))]
    {
        json!({
            "schema": PROCESS_IDENTITY_BACKEND_SCHEMA,
            "platform": std::env::consts::OS,
            "backend": "unsupported_platform",
            "supported": false,
            "safe_to_start_workers": false,
            "safe_to_monitor_workers": false,
            "safe_to_cancel_workers": false,
            "identity_fields": [],
            "continuity_fields": [],
            "provenance_fields": [],
            "unsupported_reason": "process_identity_backend_not_implemented",
            "notes": [
                "bounded swarm worker lifecycle fails closed until a platform-specific identity backend is implemented"
            ]
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LinuxProcStat {
    pid: u32,
    process_group_id: u32,
    start_time_ticks: u64,
}

fn parse_linux_proc_stat(contents: &str) -> anyhow::Result<LinuxProcStat> {
    let contents = contents.trim();
    let comm_start = contents
        .find('(')
        .ok_or_else(|| anyhow::anyhow!("missing comm opening delimiter"))?;
    let comm_end = contents
        .rfind(')')
        .filter(|index| *index > comm_start)
        .ok_or_else(|| anyhow::anyhow!("missing comm closing delimiter"))?;

    let pid = contents[..comm_start]
        .trim()
        .parse::<u32>()
        .map_err(|_| anyhow::anyhow!("invalid pid"))?;
    if pid == 0 {
        return Err(anyhow::anyhow!("invalid pid: zero"));
    }

    let fields = contents[comm_end + 1..]
        .split_whitespace()
        .collect::<Vec<_>>();
    if fields.len() < 20 {
        return Err(anyhow::anyhow!("missing field 22 (start time)"));
    }

    let process_group_id = fields[2]
        .parse::<u32>()
        .map_err(|_| anyhow::anyhow!("invalid process group id"))?;
    if process_group_id == 0 {
        return Err(anyhow::anyhow!("invalid process group id: zero"));
    }

    let start_time_ticks = fields[19]
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("invalid start time"))?;
    if start_time_ticks == 0 {
        return Err(anyhow::anyhow!("invalid start time: zero"));
    }

    Ok(LinuxProcStat {
        pid,
        process_group_id,
        start_time_ticks,
    })
}

pub(crate) fn identity_digest(identity: &WorkerProcessIdentity) -> String {
    let mut digest = Sha256::new();
    digest.update(b"kiana.swarm-process-identity.digest.v1\0");
    update_digest_field(&mut digest, identity.schema.as_bytes());
    update_digest_field(&mut digest, identity.platform.as_bytes());
    update_digest_field(&mut digest, &identity.pid.to_be_bytes());
    update_digest_field(&mut digest, &identity.process_group_id.to_be_bytes());
    update_digest_field(&mut digest, &identity.start_time_ticks.to_be_bytes());
    update_digest_field(&mut digest, identity.command_sha256.as_bytes());
    format!("sha256:{:x}", digest.finalize())
}

fn update_digest_field(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
}

fn command_sha256(command: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(command))
}

fn compare_worker_process_identity(
    expected: &WorkerProcessIdentity,
    actual: &WorkerProcessIdentity,
) -> ProcessIdentityStatus {
    let mut reasons = Vec::new();
    if expected.schema != actual.schema {
        reasons.push("schema".to_string());
    }
    if expected.platform != actual.platform {
        reasons.push("platform".to_string());
    }
    if expected.pid != actual.pid {
        reasons.push("pid".to_string());
    }
    if expected.process_group_id != actual.process_group_id {
        reasons.push("process_group_id".to_string());
    }
    if expected.start_time_ticks != actual.start_time_ticks {
        reasons.push("start_time_ticks".to_string());
    }
    if reasons.is_empty() {
        ProcessIdentityStatus::Verified
    } else {
        ProcessIdentityStatus::Mismatch(reasons)
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn capture_worker_process_identity(
    pid: u32,
) -> Result<WorkerProcessIdentity, ProcessIdentityStatus> {
    if pid == 0 {
        return Err(ProcessIdentityStatus::Missing);
    }

    let stat_path = format!("/proc/{pid}/stat");
    let cmdline_path = format!("/proc/{pid}/cmdline");
    let first_stat = read_linux_proc_stat(&stat_path)?;
    if first_stat.pid != pid {
        return Err(ProcessIdentityStatus::Mismatch(vec!["pid".to_string()]));
    }

    let command = match std::fs::read(&cmdline_path) {
        Ok(command) => command,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ProcessIdentityStatus::Exited);
        }
        Err(error) => {
            return Err(ProcessIdentityStatus::Unavailable(format!(
                "proc_cmdline_read_failed:{}",
                error.kind()
            )));
        }
    };

    let second_stat = read_linux_proc_stat(&stat_path)?;
    if first_stat != second_stat {
        return Err(ProcessIdentityStatus::Mismatch(vec![
            "process_changed_during_capture".to_string(),
        ]));
    }

    Ok(WorkerProcessIdentity {
        schema: PROCESS_IDENTITY_SCHEMA.to_string(),
        platform: "linux_procfs".to_string(),
        pid,
        process_group_id: first_stat.process_group_id,
        start_time_ticks: first_stat.start_time_ticks,
        command_sha256: command_sha256(&command),
    })
}

#[cfg(target_os = "linux")]
fn read_linux_proc_stat(path: &str) -> Result<LinuxProcStat, ProcessIdentityStatus> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ProcessIdentityStatus::Missing);
        }
        Err(error) => {
            return Err(ProcessIdentityStatus::Unavailable(format!(
                "proc_stat_read_failed:{}",
                error.kind()
            )));
        }
    };

    parse_linux_proc_stat(&contents).map_err(|error| {
        ProcessIdentityStatus::Unavailable(format!("proc_stat_parse_failed:{error}"))
    })
}

#[cfg(target_os = "linux")]
pub(crate) fn check_worker_process_identity(
    expected: &WorkerProcessIdentity,
) -> ProcessIdentityStatus {
    match capture_worker_process_identity(expected.pid) {
        Ok(actual) => compare_worker_process_identity(expected, &actual),
        Err(ProcessIdentityStatus::Missing) => ProcessIdentityStatus::Exited,
        Err(status) => status,
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn capture_worker_process_identity(
    _pid: u32,
) -> Result<WorkerProcessIdentity, ProcessIdentityStatus> {
    Err(ProcessIdentityStatus::Unavailable(format!(
        "process_identity_unavailable_on_{}",
        std::env::consts::OS
    )))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn check_worker_process_identity(
    _expected: &WorkerProcessIdentity,
) -> ProcessIdentityStatus {
    ProcessIdentityStatus::Unavailable(format!(
        "process_identity_unavailable_on_{}",
        std::env::consts::OS
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc_stat(pid: &str, comm: &str, process_group_id: &str, start_time_ticks: &str) -> String {
        format!(
            "{pid} ({comm}) S 1 {process_group_id} 1 0 -1 0 1 0 0 0 0 0 0 20 0 1 0 0 {start_time_ticks}"
        )
    }

    fn identity(
        schema: &str,
        platform: &str,
        pid: u32,
        process_group_id: u32,
        start_time_ticks: u64,
        command_sha256: &str,
    ) -> WorkerProcessIdentity {
        WorkerProcessIdentity {
            schema: schema.to_string(),
            platform: platform.to_string(),
            pid,
            process_group_id,
            start_time_ticks,
            command_sha256: command_sha256.to_string(),
        }
    }

    #[test]
    fn parse_linux_proc_stat_accepts_normal_comm() {
        let parsed = parse_linux_proc_stat(&proc_stat("123", "kiana-worker", "123", "98765"))
            .expect("valid proc stat");

        assert_eq!(parsed.pid, 123);
        assert_eq!(parsed.process_group_id, 123);
        assert_eq!(parsed.start_time_ticks, 98_765);
    }

    #[test]
    fn parse_linux_proc_stat_accepts_spaces_and_right_parentheses_in_comm() {
        let parsed =
            parse_linux_proc_stat(&proc_stat("321", "kiana worker) phase 2)", "654", "123456"))
                .expect("comm may contain spaces and right parentheses");

        assert_eq!(parsed.pid, 321);
        assert_eq!(parsed.process_group_id, 654);
        assert_eq!(parsed.start_time_ticks, 123_456);
    }

    #[test]
    fn parse_linux_proc_stat_rejects_missing_fields() {
        let error = parse_linux_proc_stat("123 (worker) S 1 123")
            .expect_err("truncated proc stat must be rejected");

        assert!(error.to_string().contains("missing field 22"));
    }

    #[test]
    fn parse_linux_proc_stat_rejects_invalid_or_zero_pid() {
        for pid in ["invalid", "0"] {
            let error = parse_linux_proc_stat(&proc_stat(pid, "worker", "123", "98765"))
                .expect_err("invalid pid must be rejected");
            assert!(error.to_string().contains("invalid pid"), "{error}");
        }
    }

    #[test]
    fn parse_linux_proc_stat_rejects_zero_process_group_id() {
        let error = parse_linux_proc_stat(&proc_stat("123", "worker", "0", "98765"))
            .expect_err("zero process group id must be rejected");

        assert!(error.to_string().contains("invalid process group id"));
    }

    #[test]
    fn parse_linux_proc_stat_rejects_zero_start_time() {
        let error = parse_linux_proc_stat(&proc_stat("123", "worker", "123", "0"))
            .expect_err("zero start time must be rejected");

        assert!(error.to_string().contains("invalid start time"));
    }

    #[test]
    fn identity_mismatch_reasons_have_stable_field_order() {
        let expected = identity("schema-a", "linux", 11, 12, 13, "sha256:expected");
        let actual = identity("schema-b", "other", 21, 22, 23, "sha256:actual");

        assert_eq!(
            compare_worker_process_identity(&expected, &actual),
            ProcessIdentityStatus::Mismatch(vec![
                "schema".to_string(),
                "platform".to_string(),
                "pid".to_string(),
                "process_group_id".to_string(),
                "start_time_ticks".to_string(),
            ])
        );
    }

    #[test]
    fn command_digest_is_provenance_not_process_continuity() {
        let expected = identity("schema", "linux_procfs", 11, 11, 13, "sha256:before");
        let actual = identity("schema", "linux_procfs", 11, 11, 13, "sha256:after");

        assert_eq!(
            compare_worker_process_identity(&expected, &actual),
            ProcessIdentityStatus::Verified
        );
    }

    #[test]
    fn process_identity_backend_capability_reports_platform_boundary() {
        let capability = process_identity_backend_capability();

        assert_eq!(
            capability["schema"],
            "kiana.swarm-process-identity-backend.v1"
        );
        assert_eq!(capability["platform"], std::env::consts::OS);

        #[cfg(target_os = "linux")]
        {
            assert_eq!(capability["backend"], "linux_procfs");
            assert_eq!(capability["supported"], true);
            assert_eq!(capability["safe_to_start_workers"], true);
            assert_eq!(capability["safe_to_monitor_workers"], true);
            assert_eq!(capability["safe_to_cancel_workers"], true);
            assert_eq!(
                capability["continuity_fields"],
                serde_json::json!(["pid", "process_group_id", "start_time_ticks"])
            );
            assert_eq!(
                capability["provenance_fields"],
                serde_json::json!(["command_sha256"])
            );
            assert!(capability["unsupported_reason"].is_null());
        }

        #[cfg(not(target_os = "linux"))]
        {
            assert_eq!(capability["backend"], "unsupported_platform");
            assert_eq!(capability["supported"], false);
            assert_eq!(capability["safe_to_start_workers"], false);
            assert_eq!(capability["safe_to_monitor_workers"], false);
            assert_eq!(capability["safe_to_cancel_workers"], false);
            assert_eq!(
                capability["unsupported_reason"],
                "process_identity_backend_not_implemented"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn capture_and_check_current_linux_process_identity() {
        let captured = capture_worker_process_identity(std::process::id())
            .expect("current process identity must be capturable");

        assert_eq!(captured.schema, "kiana.swarm-process-identity.v1");
        assert_eq!(captured.platform, "linux_procfs");
        assert_eq!(captured.pid, std::process::id());
        assert!(captured.process_group_id > 0);
        assert!(captured.start_time_ticks > 0);
        assert!(captured.command_sha256.starts_with("sha256:"));
        assert_eq!(
            check_worker_process_identity(&captured),
            ProcessIdentityStatus::Verified
        );
    }

    #[test]
    fn identity_digest_is_stable_and_sensitive_to_identity_fields() {
        let first = identity("schema", "linux", 11, 12, 13, "sha256:command");
        let mut changed = first.clone();
        changed.start_time_ticks += 1;

        let first_digest = identity_digest(&first);
        assert!(first_digest.starts_with("sha256:"));
        assert_eq!(first_digest, identity_digest(&first));
        assert_ne!(first_digest, identity_digest(&changed));
    }
}

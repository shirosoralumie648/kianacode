//! Typed shell command preparation shared by short and long-running process adapters.
//!
//! A string command is deliberately opaque: it is executed by the fixed non-login `/bin/sh -c`
//! boundary and receives its own digest. An argv command is never joined into a shell string.
//! This module does not attempt to infer side effects from shell text.

use kiana_ports::PortError;
use serde_json::{json, Value};

const MAX_COMMAND_BYTES: usize = 1024 * 1024;
const MAX_ARG_COUNT: usize = 256;
const MAX_ARG_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShellCommandKind {
    OpaqueString,
    Argv,
}

#[derive(Clone, Debug)]
pub(crate) struct ShellCommandPlan {
    pub(crate) argv: Vec<String>,
    pub(crate) kind: ShellCommandKind,
    pub(crate) command_digest: String,
}

impl ShellCommandPlan {
    pub(crate) fn from_value(command: Option<&Value>) -> Result<Self, PortError> {
        let command = command.ok_or_else(|| failed("harness_command_required"))?;
        let (argv, kind, digest_input) = if let Some(value) = command.as_str() {
            let value = value.trim();
            if value.is_empty() {
                return Err(failed("harness_command_required"));
            }
            if value.len() > MAX_COMMAND_BYTES || value.contains('\0') {
                return Err(failed("harness_command_invalid"));
            }
            (
                vec!["/bin/sh".to_owned(), "-c".to_owned(), value.to_owned()],
                ShellCommandKind::OpaqueString,
                json!({"kind":"opaque_string","shell":"/bin/sh","login":false,"text":value}),
            )
        } else if let Some(items) = command.as_array() {
            if items.is_empty() || items.len() > MAX_ARG_COUNT {
                return Err(failed("harness_command_invalid"));
            }
            let mut argv = Vec::with_capacity(items.len());
            let mut total = 0usize;
            for item in items {
                let value = item
                    .as_str()
                    .ok_or_else(|| failed("harness_command_invalid"))?;
                if value.is_empty() || value.len() > MAX_ARG_BYTES || value.contains('\0') {
                    return Err(failed("harness_command_invalid"));
                }
                total = total
                    .checked_add(value.len())
                    .filter(|size| *size <= MAX_COMMAND_BYTES)
                    .ok_or_else(|| failed("harness_command_invalid"))?;
                argv.push(value.to_owned());
            }
            if argv[0].trim().is_empty() {
                return Err(failed("harness_command_invalid"));
            }
            let digest_input = json!({"kind":"argv","argv":argv.clone()});
            (argv, ShellCommandKind::Argv, digest_input)
        } else {
            return Err(failed("harness_command_invalid"));
        };
        Ok(Self {
            argv,
            kind,
            command_digest: kiana_domain::json_digest(&digest_input),
        })
    }

    pub(crate) fn metadata(&self) -> Value {
        json!({
            "kind": match self.kind { ShellCommandKind::OpaqueString => "opaque_shell_string", ShellCommandKind::Argv => "argv" },
            "executable": self.argv.first(),
            "shell": self.kind == ShellCommandKind::OpaqueString,
            "login": false,
            "command_digest": self.command_digest,
            "side_effect_analysis": "not_performed",
        })
    }
}

fn failed(reason: &str) -> PortError {
    PortError::Failed(reason.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn string_command_is_opaque_fixed_non_login_shell() {
        let plan = ShellCommandPlan::from_value(Some(&json!("echo hello | tr a-z A-Z"))).unwrap();
        assert_eq!(plan.kind, ShellCommandKind::OpaqueString);
        assert_eq!(plan.argv[0], "/bin/sh");
        assert_eq!(plan.argv[1], "-c");
        assert_eq!(plan.metadata()["shell"], true);
        assert_eq!(plan.metadata()["login"], false);
        assert_eq!(plan.metadata()["side_effect_analysis"], "not_performed");
    }

    #[test]
    fn argv_command_never_becomes_a_shell_string() {
        let plan = ShellCommandPlan::from_value(Some(&json!(["printf", "%s", "a b"]))).unwrap();
        assert_eq!(plan.kind, ShellCommandKind::Argv);
        assert_eq!(plan.argv, ["printf", "%s", "a b"]);
        assert_eq!(plan.metadata()["shell"], false);
    }

    #[test]
    fn command_bounds_reject_nul_empty_and_oversized_inputs() {
        assert!(ShellCommandPlan::from_value(Some(&json!(["", "x"]))).is_err());
        assert!(ShellCommandPlan::from_value(Some(&json!("echo\u{0}"))).is_err());
        assert!(
            ShellCommandPlan::from_value(Some(&json!(vec!["x".repeat(MAX_ARG_BYTES + 1)])))
                .is_err()
        );
    }
}

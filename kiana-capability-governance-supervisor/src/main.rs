use std::env;
use std::ffi::OsString;
use std::process::ExitCode;

use kiana_capability_governance_supervisor::{
    parse_invocation, run_public, trace_logger_main, worker_launcher_main, Invocation,
    LinuxProcessLauncher, SupervisorConfig, UsageError, DEFAULT_RUNNER,
};

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().collect();
    let internal = args
        .get(1)
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.starts_with("__"));
    let invocation = match parse_invocation(&args) {
        Ok(invocation) => invocation,
        Err(UsageError) => {
            eprintln!(
                "{}",
                if internal {
                    "supervisor_unavailable"
                } else {
                    "supervisor_usage"
                }
            );
            return ExitCode::from(if internal { 1 } else { 2 });
        }
    };

    match invocation {
        Invocation::Public(slice) => run_public_invocation(slice),
        Invocation::TraceLogger => internal_result(trace_logger_main()),
        Invocation::WorkerLauncher { runner, slice } => {
            internal_result(worker_launcher_main(&runner, slice))
        }
    }
}

fn internal_result(
    result: Result<(), kiana_capability_governance_supervisor::SupervisorError>,
) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", error.diagnostic());
            ExitCode::from(1)
        }
    }
}

fn run_public_invocation(slice: kiana_capability_governance_supervisor::Slice) -> ExitCode {
    let repo_root = match env::current_dir() {
        Ok(path) => path,
        Err(_) => {
            eprintln!("supervisor_unavailable");
            return ExitCode::from(1);
        }
    };
    let runner = repo_root.join(DEFAULT_RUNNER);
    let supervisor_bin = match env::current_exe() {
        Ok(path) => path,
        Err(_) => {
            eprintln!("supervisor_unavailable");
            return ExitCode::from(1);
        }
    };
    let config = SupervisorConfig::new(repo_root, runner, supervisor_bin);
    let outcome = run_public(&config, slice, &LinuxProcessLauncher);
    if !outcome.public_stdout.is_empty() {
        print!("{}", String::from_utf8_lossy(&outcome.public_stdout));
    }
    if !outcome.public_stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&outcome.public_stderr));
    }
    ExitCode::from(outcome.exit_code as u8)
}

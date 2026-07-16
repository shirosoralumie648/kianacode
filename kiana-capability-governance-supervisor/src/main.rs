#[cfg(target_os = "linux")]
use std::env;
#[cfg(target_os = "linux")]
use std::ffi::OsString;
use std::process::ExitCode;

#[cfg(target_os = "linux")]
use kiana_capability_governance_supervisor::{
    embedded_repo_root, parse_invocation, run_public, trace_logger_main, worker_launcher_main,
    Invocation, LinuxProcessLauncher, SupervisorConfig, UsageError, DEFAULT_RUNNER,
};

#[cfg(target_os = "linux")]
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
            worker_launcher_result(worker_launcher_main(&runner, slice))
        }
    }
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
fn worker_launcher_result(
    result: Result<(), kiana_capability_governance_supervisor::SupervisorError>,
) -> ExitCode {
    match result {
        Ok(()) => ExitCode::from(
            kiana_capability_governance_supervisor::SUPERVISOR_WORKER_EXIT_CODE as u8,
        ),
        Err(error) => {
            eprintln!("{}", error.diagnostic());
            ExitCode::from(1)
        }
    }
}

#[cfg(target_os = "linux")]
fn run_public_invocation(slice: kiana_capability_governance_supervisor::Slice) -> ExitCode {
    let repo_root = match embedded_repo_root() {
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

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!("supervisor_unavailable");
    ExitCode::from(1)
}

//! Deterministic, isolated evaluation runtime boundary.
//!
//! This module is an adapter for CI/evaluation fixtures only.  It creates an owned temporary
//! workspace and KIANA_HOME, supplies a fixed clock observation and deterministic bytes, and does
//! not start a runner, scheduler, provider or capability loop.  Callers must still route any
//! target through the normal DaemonHost/ControlPlane composition.

use kiana_domain::ClockObservation;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const EVAL_RUNTIME_SCHEMA: &str = "kiana.eval-runtime.v1";
pub const EVAL_RUNTIME_ENV_KIANA_HOME: &str = "KIANA_HOME";
pub const EVAL_RUNTIME_ENV_HOME: &str = "HOME";

static PROCESS_ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn env_lock() -> &'static Mutex<()> {
    PROCESS_ENV_LOCK.get_or_init(|| Mutex::new(()))
}

fn bounded_label(label: &str) -> bool {
    !label.trim().is_empty()
        && label.len() <= 64
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// Owned evaluation sandbox.  The root is created with `create_dir`, never reused or recursively
/// cleaned unless this instance created it successfully.
pub struct EvalRuntimeSandbox {
    root: PathBuf,
    workspace: PathBuf,
    kiana_home: PathBuf,
    clock: ClockObservation,
    random_seed: u64,
}

impl EvalRuntimeSandbox {
    pub fn create(
        label: &str,
        wall_now_unix_ms: u64,
        monotonic_now_ms: u64,
        random_seed: u64,
    ) -> io::Result<Self> {
        if !bounded_label(label) || wall_now_unix_ms == 0 || monotonic_now_ms == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "eval_runtime_spec_invalid",
            ));
        }
        let root = std::env::temp_dir().join(format!(
            "kiana-eval-{label}-{}-{random_seed:016x}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        let workspace = root.join("workspace");
        let kiana_home = root.join("kiana-home");
        fs::create_dir(&workspace)?;
        fs::create_dir(&kiana_home)?;
        let clock = ClockObservation::observe(
            "eval-fixed",
            u128::from(wall_now_unix_ms),
            u128::from(monotonic_now_ms),
            None,
            1,
        )
        .map_err(io::Error::other)?;
        Ok(Self {
            root,
            workspace,
            kiana_home,
            clock,
            random_seed,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn kiana_home(&self) -> &Path {
        &self.kiana_home
    }

    pub fn clock(&self) -> &ClockObservation {
        &self.clock
    }

    pub fn random_seed(&self) -> u64 {
        self.random_seed
    }

    /// Explicit environment passed to an eval target.  It never reads the operator's home.
    pub fn environment(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            (
                EVAL_RUNTIME_ENV_KIANA_HOME.to_owned(),
                self.kiana_home.display().to_string(),
            ),
            (
                EVAL_RUNTIME_ENV_HOME.to_owned(),
                self.root.join("home").display().to_string(),
            ),
        ])
    }

    /// Run a bounded fixture callback with the process environment redirected to this sandbox.
    /// The mutex prevents concurrent CI fixtures from crossing their KIANA_HOME boundaries, and
    /// every original value is restored even when the callback returns an error.
    pub fn with_process_environment<T, F>(&self, callback: F) -> io::Result<T>
    where
        F: FnOnce() -> io::Result<T>,
    {
        let _guard = env_lock()
            .lock()
            .map_err(|_| io::Error::other("eval_runtime_env_lock_poisoned"))?;
        let home = self.root.join("home");
        fs::create_dir_all(&home)?;
        let previous_kiana_home = std::env::var_os(EVAL_RUNTIME_ENV_KIANA_HOME);
        let previous_home = std::env::var_os(EVAL_RUNTIME_ENV_HOME);
        std::env::set_var(EVAL_RUNTIME_ENV_KIANA_HOME, &self.kiana_home);
        std::env::set_var(EVAL_RUNTIME_ENV_HOME, &home);
        let result = callback();
        match previous_kiana_home {
            Some(value) => std::env::set_var(EVAL_RUNTIME_ENV_KIANA_HOME, value),
            None => std::env::remove_var(EVAL_RUNTIME_ENV_KIANA_HOME),
        }
        match previous_home {
            Some(value) => std::env::set_var(EVAL_RUNTIME_ENV_HOME, value),
            None => std::env::remove_var(EVAL_RUNTIME_ENV_HOME),
        }
        result
    }

    /// Deterministic pseudo-random bytes for fixture IDs/nonces.  This is not a security random
    /// source and is intentionally unsuitable for production credentials.
    pub fn deterministic_bytes(&self, label: &str, length: usize) -> io::Result<Vec<u8>> {
        if !bounded_label(label) || length > 64 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "eval_runtime_seed_request_invalid",
            ));
        }
        let mut state = self.random_seed ^ fnv1a(label.as_bytes());
        let mut bytes = Vec::with_capacity(length);
        while bytes.len() < length {
            state = state.wrapping_add(0x9e37_79b9_7f4a_7c15).rotate_left(17) ^ (state >> 11);
            bytes.extend_from_slice(&state.to_le_bytes());
        }
        bytes.truncate(length);
        Ok(bytes)
    }
}

impl Drop for EvalRuntimeSandbox {
    fn drop(&mut self) {
        // Exact, self-created temp root only; failure is intentionally not turned into a fake
        // evaluation result because cleanup is an infrastructure concern recorded by the caller.
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x1000_0000_01b3)
    })
}

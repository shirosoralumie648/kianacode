use std::env;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

unsafe extern "C" {
    fn getuid() -> u32;
    fn getgid() -> u32;
}

const CANARY: &str = r#"
import jsonschema

schema = {
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "properties": {"items": {"type": "array", "uniqueItems": True}},
    "required": ["items"],
}
jsonschema.Draft202012Validator.check_schema(schema)
validator = jsonschema.Draft202012Validator(schema)
validator.validate({"items": ["one", "two"]})
try:
    validator.validate({"items": "not-an-array"})
except jsonschema.ValidationError:
    pass
else:
    raise SystemExit("negative instance was accepted")
"#;

fn discover_python() -> Result<PathBuf, String> {
    let path = env::var_os("PATH").ok_or_else(|| "PATH is unavailable".to_owned())?;
    for name in ["python3", "python"] {
        for component in env::split_paths(&path) {
            let candidate = component.join(name);
            if candidate.is_file() {
                if let Ok(validated) = validate_python(&candidate) {
                    return Ok(validated);
                }
            }
        }
    }
    Err("no trusted python3/python executable found".to_owned())
}

fn validate_python(path: &Path) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("cannot canonicalize {}: {error}", path.display()))?;
    let metadata = fs::metadata(&canonical)
        .map_err(|error| format!("cannot stat {}: {error}", canonical.display()))?;
    if !metadata.is_file() || metadata.mode() & 0o100 == 0 {
        return Err("python must be a regular executable".to_owned());
    }
    let uid = unsafe { getuid() };
    let gid = unsafe { getgid() };
    if metadata.uid() != 0 && metadata.uid() != uid {
        return Err("python owner is not the build user or root".to_owned());
    }
    if metadata.mode() & 0o002 != 0 {
        return Err("python is writable by other users".to_owned());
    }
    if metadata.mode() & 0o020 != 0 && metadata.gid() != gid {
        return Err("group-writable python has an untrusted group".to_owned());
    }
    Ok(canonical)
}

fn run_jsonschema_canary(path: &Path) -> Result<(), String> {
    let output = Command::new(path)
        .arg("-I")
        .arg("-c")
        .arg(CANARY)
        .env_clear()
        .output()
        .map_err(|error| format!("failed to run python canary: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "python jsonschema canary failed with status {}",
            output.status
        ))
    }
}

fn main() {
    let result = discover_python().and_then(|path| {
        run_jsonschema_canary(&path)?;
        let revalidated = validate_python(&path)?;
        if revalidated != path {
            return Err("python identity changed during build validation".to_owned());
        }
        let metadata = fs::metadata(&revalidated)
            .map_err(|error| format!("cannot stat {}: {error}", path.display()))?;
        Ok((revalidated, metadata.uid(), metadata.gid()))
    });
    match result {
        Ok((path, uid, gid)) => {
            println!("cargo:rerun-if-env-changed=PATH");
            println!("cargo:rustc-env=KIANA_GOVERNANCE_PYTHON={}", path.display());
            println!("cargo:rustc-env=KIANA_GOVERNANCE_PYTHON_UID={uid}");
            println!("cargo:rustc-env=KIANA_GOVERNANCE_PYTHON_GID={gid}");
        }
        Err(error) => panic!("governance supervisor Python prerequisite failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn test_root(name: &str) -> PathBuf {
        let root = env::temp_dir().join(format!(
            "kiana-supervisor-build-test-{}-{name}",
            std::process::id(),
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        root
    }

    #[test]
    fn discover_python_prefers_python3_and_canonicalizes_it() {
        let _guard = ENV_LOCK.lock().unwrap();
        let root = test_root("discover");
        let python3 = root.join("python3");
        let python = root.join("python");
        fs::write(&python3, b"#!/bin/sh\nexit 1\n").unwrap();
        fs::write(&python, b"#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&python3, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&python, fs::Permissions::from_mode(0o755)).unwrap();
        let old = env::var_os("PATH");
        env::set_var("PATH", &root);
        let discovered = discover_python().unwrap();
        match old {
            Some(value) => env::set_var("PATH", value),
            None => env::remove_var("PATH"),
        }
        assert_eq!(discovered, fs::canonicalize(python3).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn validate_python_rejects_other_writable_executable() {
        let root = test_root("other-writable");
        let path = root.join("python3");
        fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o757)).unwrap();
        let error = validate_python(&path).unwrap_err();
        assert!(error.contains("other users"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn jsonschema_canary_requires_positive_accept_and_negative_reject() {
        let _guard = ENV_LOCK.lock().unwrap();
        let root = test_root("canary");
        let fake = root.join("python3");
        fs::write(&fake, b"#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(run_jsonschema_canary(&fake).is_err());
        let path = env::var_os("KIANA_TEST_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| discover_python().expect("test interpreter must be available"));
        run_jsonschema_canary(&path).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}

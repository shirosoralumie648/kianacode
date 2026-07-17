use rand::rngs::OsRng;
use rand::RngCore;
use ring::hmac;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const INTEGRITY_KEY_SCHEMA: &str = "kiana.workflow-integrity-key.v1";
pub const INTEGRITY_KEY_STATUS_SCHEMA: &str = "kiana.workflow-integrity-key-status.v1";
pub const INTEGRITY_ENVELOPE_SCHEMA: &str = "kiana.integrity-envelope.v1";
pub const LOCAL_HMAC_ALGORITHM: &str = "local_hmac_sha256_v1";

#[derive(Debug, thiserror::Error)]
pub enum IntegrityError {
    #[error("integrity_key_missing")]
    KeyMissing,
    #[error("integrity_key_invalid: {0}")]
    KeyInvalid(String),
    #[error("integrity_key_permissions_insecure")]
    InsecurePermissions,
    #[error("integrity_key_id_mismatch")]
    KeyIdMismatch,
    #[error("integrity_payload_mismatch")]
    PayloadMismatch,
    #[error("integrity_auth_mismatch")]
    AuthMismatch,
    #[error("integrity_chain_mismatch")]
    ChainMismatch,
    #[error("integrity_downgrade_detected")]
    DowngradeDetected,
    #[error("integrity_artifact_mismatch: {0}")]
    ArtifactMismatch(String),
    #[error("integrity filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("integrity json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntegrityEnvelope {
    pub schema: String,
    pub algorithm: String,
    pub key_id: String,
    pub payload_sha256: String,
    pub previous_record_sha256: String,
    pub auth: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntegrityKeyStatus {
    pub schema: String,
    pub configured: bool,
    pub backend: String,
    pub algorithm: String,
    pub key_id: Option<String>,
    pub path: String,
    pub permission_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalHmacKeyFile {
    schema: String,
    algorithm: String,
    key_id: String,
    created_at_ms: u64,
    secret_hex: String,
}

#[derive(Clone)]
pub struct LocalHmacKey {
    secret: [u8; 32],
    key_id: String,
}

impl LocalHmacKey {
    #[cfg(test)]
    fn from_secret_for_test(secret: [u8; 32]) -> Self {
        let key_id = key_id_for_secret(&secret);
        Self { secret, key_id }
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn sign_payload(
        &self,
        domain: &str,
        payload: &[u8],
        previous_record_sha256: &str,
    ) -> Result<IntegrityEnvelope, IntegrityError> {
        let payload_sha256 = format!("sha256:{}", sha256_hex(payload));
        let input = auth_input(
            domain,
            &self.key_id,
            &payload_sha256,
            previous_record_sha256,
        );
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.secret);
        let auth = hmac::sign(&key, &input);
        Ok(IntegrityEnvelope {
            schema: INTEGRITY_ENVELOPE_SCHEMA.to_string(),
            algorithm: LOCAL_HMAC_ALGORITHM.to_string(),
            key_id: self.key_id.clone(),
            payload_sha256,
            previous_record_sha256: previous_record_sha256.to_string(),
            auth: format!("hmac-sha256:{}", bytes_to_hex(auth.as_ref())),
        })
    }

    pub fn verify_payload(
        &self,
        domain: &str,
        payload: &[u8],
        envelope: &IntegrityEnvelope,
    ) -> Result<(), IntegrityError> {
        if envelope.schema != INTEGRITY_ENVELOPE_SCHEMA
            || envelope.algorithm != LOCAL_HMAC_ALGORITHM
        {
            return Err(IntegrityError::KeyInvalid(
                "unsupported integrity envelope".to_string(),
            ));
        }
        if envelope.key_id != self.key_id {
            return Err(IntegrityError::KeyIdMismatch);
        }
        let payload_sha256 = format!("sha256:{}", sha256_hex(payload));
        if envelope.payload_sha256 != payload_sha256 {
            return Err(IntegrityError::PayloadMismatch);
        }
        let input = auth_input(
            domain,
            &self.key_id,
            &payload_sha256,
            &envelope.previous_record_sha256,
        );
        let auth = envelope
            .auth
            .strip_prefix("hmac-sha256:")
            .ok_or(IntegrityError::AuthMismatch)
            .and_then(hex_to_bytes)?;
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.secret);
        hmac::verify(&key, &input, &auth).map_err(|_| IntegrityError::AuthMismatch)
    }
}

pub fn workflow_integrity_key_path() -> Result<PathBuf, IntegrityError> {
    if let Some(path) =
        env::var_os("KIANA_WORKFLOW_INTEGRITY_KEY_FILE").filter(|value| !value.is_empty())
    {
        return Ok(PathBuf::from(path));
    }
    if let Some(home) = env::var_os("KIANA_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home)
            .join("trust")
            .join("workflow-integrity-key.json"));
    }
    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home)
            .join(".kiana")
            .join("trust")
            .join("workflow-integrity-key.json"));
    }
    Err(IntegrityError::KeyMissing)
}

pub fn initialize_local_hmac_key() -> Result<IntegrityKeyStatus, IntegrityError> {
    initialize_local_hmac_key_at(&workflow_integrity_key_path()?)
}

pub fn initialize_local_hmac_key_at(path: &Path) -> Result<IntegrityKeyStatus, IntegrityError> {
    if path.exists() {
        let key = load_local_hmac_key_at(path)?;
        return Ok(key_status(path, Some(&key), "secure"));
    }
    let parent = path.parent().ok_or_else(|| {
        IntegrityError::KeyInvalid("integrity key path has no parent".to_string())
    })?;
    fs::create_dir_all(parent)?;
    restrict_directory_permissions(parent)?;

    let mut secret = [0u8; 32];
    OsRng.fill_bytes(&mut secret);
    let key_id = key_id_for_secret(&secret);
    let key_file = LocalHmacKeyFile {
        schema: INTEGRITY_KEY_SCHEMA.to_string(),
        algorithm: LOCAL_HMAC_ALGORITHM.to_string(),
        key_id: key_id.clone(),
        created_at_ms: now_ms(),
        secret_hex: bytes_to_hex(&secret),
    };
    let contents = serde_json::to_vec_pretty(&key_file)?;
    let temp = parent.join(format!(
        ".workflow-integrity-key.{}-{}.tmp",
        std::process::id(),
        now_ms()
    ));
    let write_result = (|| -> Result<(), IntegrityError> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        set_owner_only_create_mode(&mut options);
        let mut file = options.open(&temp)?;
        file.write_all(&contents)?;
        file.flush()?;
        file.sync_all()?;
        restrict_file_permissions(&temp)?;
        match fs::hard_link(&temp, path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        fs::remove_file(&temp)?;
        restrict_file_permissions(path)?;
        sync_directory(parent);
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    write_result?;

    let key = load_local_hmac_key_at(path)?;
    Ok(key_status(path, Some(&key), "secure"))
}

pub fn inspect_local_hmac_key() -> Result<IntegrityKeyStatus, IntegrityError> {
    let path = workflow_integrity_key_path()?;
    if !path.exists() {
        return Ok(key_status(&path, None, "missing"));
    }
    match load_local_hmac_key_at(&path) {
        Ok(key) => Ok(key_status(&path, Some(&key), "secure")),
        Err(IntegrityError::InsecurePermissions) => {
            Ok(key_status(&path, None, "insecure_permissions"))
        }
        Err(error) => Err(error),
    }
}

pub fn load_local_hmac_key() -> Result<Option<LocalHmacKey>, IntegrityError> {
    let path = match workflow_integrity_key_path() {
        Ok(path) => path,
        Err(IntegrityError::KeyMissing) => return Ok(None),
        Err(error) => return Err(error),
    };
    if !path.exists() {
        return Ok(None);
    }
    load_local_hmac_key_at(&path).map(Some)
}

pub fn load_local_hmac_key_at(path: &Path) -> Result<LocalHmacKey, IntegrityError> {
    ensure_secure_key_file(path)?;
    let key_file: LocalHmacKeyFile = serde_json::from_slice(&fs::read(path)?)?;
    if key_file.schema != INTEGRITY_KEY_SCHEMA || key_file.algorithm != LOCAL_HMAC_ALGORITHM {
        return Err(IntegrityError::KeyInvalid(
            "unsupported integrity key schema or algorithm".to_string(),
        ));
    }
    let secret = hex_to_array_32(&key_file.secret_hex)?;
    let key_id = key_id_for_secret(&secret);
    if key_file.key_id != key_id {
        return Err(IntegrityError::KeyIdMismatch);
    }
    Ok(LocalHmacKey { secret, key_id })
}

fn key_status(
    path: &Path,
    key: Option<&LocalHmacKey>,
    permission_state: &str,
) -> IntegrityKeyStatus {
    IntegrityKeyStatus {
        schema: INTEGRITY_KEY_STATUS_SCHEMA.to_string(),
        configured: key.is_some(),
        backend: "file".to_string(),
        algorithm: LOCAL_HMAC_ALGORITHM.to_string(),
        key_id: key.map(|key| key.key_id.clone()),
        path: path.display().to_string(),
        permission_state: permission_state.to_string(),
    }
}

fn key_id_for_secret(secret: &[u8; 32]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"kiana.workflow-integrity-key.v1\0");
    digest.update(secret);
    format!("sha256:{:x}", digest.finalize())
}

fn auth_input(
    domain: &str,
    key_id: &str,
    payload_sha256: &str,
    previous_record_sha256: &str,
) -> Vec<u8> {
    let mut input = Vec::new();
    push_len_prefixed(&mut input, b"kiana.integrity-auth.v1");
    push_len_prefixed(&mut input, domain.as_bytes());
    push_len_prefixed(&mut input, key_id.as_bytes());
    push_len_prefixed(&mut input, payload_sha256.as_bytes());
    push_len_prefixed(&mut input, previous_record_sha256.as_bytes());
    input
}

fn push_len_prefixed(buffer: &mut Vec<u8>, value: &[u8]) {
    buffer.extend_from_slice(&(value.len() as u64).to_be_bytes());
    buffer.extend_from_slice(value);
}

pub fn sha256_prefixed(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(bytes))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn hex_to_bytes(value: &str) -> Result<Vec<u8>, IntegrityError> {
    if !value.len().is_multiple_of(2) {
        return Err(IntegrityError::KeyInvalid(
            "hex value has odd length".to_string(),
        ));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char)
                .to_digit(16)
                .ok_or_else(|| IntegrityError::KeyInvalid("hex value is invalid".to_string()))?;
            let low = (pair[1] as char)
                .to_digit(16)
                .ok_or_else(|| IntegrityError::KeyInvalid("hex value is invalid".to_string()))?;
            Ok(((high << 4) | low) as u8)
        })
        .collect()
}

fn hex_to_array_32(value: &str) -> Result<[u8; 32], IntegrityError> {
    let bytes = hex_to_bytes(value)?;
    bytes.try_into().map_err(|_| {
        IntegrityError::KeyInvalid("integrity secret must be exactly 32 bytes".to_string())
    })
}

fn ensure_secure_key_file(path: &Path) -> Result<(), IntegrityError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(IntegrityError::KeyInvalid(
            "integrity key path must be a regular file".to_string(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(IntegrityError::InsecurePermissions);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_create_mode(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn set_owner_only_create_mode(_options: &mut OpenOptions) {}

#[cfg(unix)]
fn restrict_file_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_file_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn restrict_directory_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_directory_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn sync_directory(path: &Path) {
    #[cfg(unix)]
    if let Ok(directory) = fs::File::open(path) {
        let _ = directory.sync_all();
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "kiana-integrity-test-{}-{unique}-{counter}",
            std::process::id()
        ))
    }

    #[test]
    fn integrity_key_init_is_idempotent_and_owner_only() {
        let root = temp_root();
        let path = root.join("trust/workflow-integrity-key.json");
        let first = initialize_local_hmac_key_at(&path).unwrap();
        let second = initialize_local_hmac_key_at(&path).unwrap();

        assert_eq!(first.key_id, second.key_id);
        assert_eq!(first.algorithm, LOCAL_HMAC_ALGORITHM);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn integrity_key_loader_rejects_key_id_mismatch() {
        let root = temp_root();
        let path = root.join("trust/workflow-integrity-key.json");
        initialize_local_hmac_key_at(&path).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["key_id"] = serde_json::json!("sha256:forged");
        std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        assert!(matches!(
            load_local_hmac_key_at(&path),
            Err(IntegrityError::KeyIdMismatch)
        ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hmac_envelope_detects_payload_mutation() {
        let key = LocalHmacKey::from_secret_for_test([7u8; 32]);
        let envelope = key
            .sign_payload("workflow-event", b"payload", "genesis")
            .unwrap();

        assert!(key
            .verify_payload("workflow-event", b"payload", &envelope)
            .is_ok());
        assert!(matches!(
            key.verify_payload("workflow-event", b"changed", &envelope),
            Err(IntegrityError::PayloadMismatch)
        ));
    }
}

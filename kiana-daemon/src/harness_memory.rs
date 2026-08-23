//! Six-layer memory broker for the owned harness.
//!
//! Collections are partitioned JSONL files. Search and write are tools; the
//! runner never touches the store. Chat is never auto-ingested.

use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{
    AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult, MemoryCollection, RoleSpec,
    MEMORY_LAYER_COMPANY, MEMORY_LAYER_DEPARTMENT, MEMORY_LAYER_INSTANCE_SCRATCH,
    MEMORY_LAYER_PROJECT, MEMORY_LAYER_ROLE, MEMORY_LAYER_USER, MEMORY_RECORD_SCHEMA,
    MEMORY_SEARCH_SCHEMA, MEMORY_WRITE_SCHEMA,
};
use kiana_ports::PortError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SEARCH_OPERATION: &str = "memory.search";
const WRITE_OPERATION: &str = "memory.write";
const KIANA_HOME_ENV: &str = "KIANA_HOME";
const DEFAULT_LIMIT: usize = 10;

pub(crate) fn register(broker: &mut CapabilityBroker) -> Result<(), PortError> {
    broker.register_static(
        CapabilityKind::Query,
        SEARCH_OPERATION,
        std::sync::Arc::new(MemorySearchHandler),
    )?;
    broker.register_static(
        CapabilityKind::Filesystem,
        WRITE_OPERATION,
        std::sync::Arc::new(MemoryWriteHandler),
    )
}

struct MemorySearchHandler;

#[async_trait::async_trait]
impl CapabilityHandler for MemorySearchHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        if request.request.operation != SEARCH_OPERATION {
            return Err(PortError::Failed("harness_operation_mismatch".to_owned()));
        }
        let request_id = request.request.request_id;
        let arguments = request.request.arguments.clone();
        let output = tokio::task::spawn_blocking(move || search_records(&arguments))
            .await
            .map_err(|error| PortError::Failed(format!("memory_search_join_failed:{error}")))??;
        Ok(CapabilityResult::success(request_id, output))
    }
}

struct MemoryWriteHandler;

#[async_trait::async_trait]
impl CapabilityHandler for MemoryWriteHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        if request.request.operation != WRITE_OPERATION {
            return Err(PortError::Failed("harness_operation_mismatch".to_owned()));
        }
        let request_id = request.request.request_id;
        let arguments = request.request.arguments.clone();
        let output = tokio::task::spawn_blocking(move || write_record(&arguments))
            .await
            .map_err(|error| PortError::Failed(format!("memory_write_join_failed:{error}")))??;
        Ok(CapabilityResult::success(request_id, output))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct MemoryRecord {
    schema: String,
    id: String,
    layer: String,
    collection: String,
    text: String,
    source: String,
    role_id: String,
    department_id: String,
    session_id: String,
    created_at_ms: u64,
}

impl MemoryRecord {
    fn verified(&self) -> bool {
        !self.source.trim().is_empty()
    }

    fn hit(&self) -> Value {
        json!({
            "id": self.id,
            "layer": self.layer,
            "collection": self.collection,
            "source": self.source,
            "verified": self.verified(),
            "text": self.text,
        })
    }
}

fn search_records(arguments: &Value) -> Result<Value, PortError> {
    let query = required_string(arguments, "query", "memory_query_required")?;
    let role = RoleSpec::lookup(&optional_string(arguments, "role_id").unwrap_or_default())
        .ok_or_else(|| PortError::Failed("role_unknown".to_owned()))?;
    let collections = requested_collections(arguments, &role)?;
    let session_id = optional_string(arguments, "session_id").unwrap_or_default();
    let project_root = optional_string(arguments, "project_root").unwrap_or_default();
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_LIMIT);
    let mut hits = Vec::new();
    for collection in collections {
        let path = collection_path(&collection, &project_root, &session_id)?;
        for record in read_records(&path)? {
            if record.collection != collection.collection {
                continue;
            }
            if collection.layer == MEMORY_LAYER_INSTANCE_SCRATCH && record.session_id != session_id
            {
                continue;
            }
            if !text_matches(&record.text, &query) {
                continue;
            }
            hits.push(record.hit());
            if hits.len() >= limit {
                break;
            }
        }
        if hits.len() >= limit {
            break;
        }
    }
    Ok(json!({
        "schema": MEMORY_SEARCH_SCHEMA,
        "query": query,
        "role_id": role.role_id,
        "department_id": role.department_id,
        "hits": hits,
    }))
}

fn write_record(arguments: &Value) -> Result<Value, PortError> {
    let collection = required_collection(arguments)?;
    let text = required_string(arguments, "text", "memory_text_required")?;
    let source = required_string(arguments, "source", "memory_source_required")?;
    let role_id = optional_string(arguments, "role_id").unwrap_or_default();
    let department_id = optional_string(arguments, "department_id").unwrap_or_default();
    let session_id = optional_string(arguments, "session_id").unwrap_or_default();
    let project_root = optional_string(arguments, "project_root").unwrap_or_default();
    if collection.layer == MEMORY_LAYER_INSTANCE_SCRATCH && session_id.trim().is_empty() {
        return Err(PortError::Failed("memory_session_required".to_owned()));
    }
    let record = MemoryRecord {
        schema: MEMORY_RECORD_SCHEMA.to_owned(),
        id: format!("mem-{}", now_ms()),
        layer: collection.layer.clone(),
        collection: collection.collection.clone(),
        text,
        source,
        role_id,
        department_id,
        session_id: session_id.clone(),
        created_at_ms: now_ms(),
    };
    let path = collection_path(&collection, &project_root, &session_id)?;
    append_record(&path, &record)?;
    Ok(json!({
        "schema": MEMORY_WRITE_SCHEMA,
        "id": record.id,
        "layer": record.layer,
        "collection": record.collection,
        "source": record.source,
        "path": path.display().to_string(),
        "promoted": false,
    }))
}

fn requested_collections(
    arguments: &Value,
    role: &RoleSpec,
) -> Result<Vec<MemoryCollection>, PortError> {
    match optional_string(arguments, "collection") {
        Some(raw) => {
            let collection = MemoryCollection::parse(&raw)
                .ok_or_else(|| PortError::Failed("memory_collection_unknown".to_owned()))?;
            if !role.allows_knowledge(&collection.collection) {
                return Err(PortError::Failed("role_knowledge_denied".to_owned()));
            }
            Ok(vec![collection])
        }
        None => {
            let mut collections = role.granted_collections();
            collections.dedup();
            Ok(collections)
        }
    }
}

fn required_collection(arguments: &Value) -> Result<MemoryCollection, PortError> {
    let raw = required_string(arguments, "collection", "memory_collection_required")?;
    MemoryCollection::parse(&raw)
        .ok_or_else(|| PortError::Failed("memory_collection_unknown".to_owned()))
}

fn collection_path(
    collection: &MemoryCollection,
    project_root: &str,
    session_id: &str,
) -> Result<PathBuf, PortError> {
    if collection.home_scoped() {
        let home = kiana_home()?;
        let name = match collection.collection.as_str() {
            MEMORY_LAYER_COMPANY => "company.jsonl",
            MEMORY_LAYER_USER => "user.jsonl",
            "user:prefs" => "user-prefs.jsonl",
            "user-private" => "user-private.jsonl",
            other => {
                return Err(PortError::Failed(format!(
                    "memory_collection_unknown:{other}"
                )))
            }
        };
        return Ok(home.join("memory").join(name));
    }
    let root = confined_project_root(project_root)?;
    let base = root.join(".kiana").join("memory");
    let path = match collection.layer.as_str() {
        MEMORY_LAYER_DEPARTMENT => {
            let name = if collection.collection == "planning:unreleased-debate" {
                "planning-unreleased.jsonl".to_owned()
            } else {
                let id = collection
                    .collection
                    .strip_prefix("department:")
                    .unwrap_or(&collection.collection);
                format!("{id}.jsonl")
            };
            base.join("department").join(name)
        }
        MEMORY_LAYER_ROLE => {
            let id = collection
                .collection
                .strip_prefix("role:")
                .unwrap_or(&collection.collection);
            base.join("role").join(format!("{id}.jsonl"))
        }
        MEMORY_LAYER_PROJECT => {
            let name = match collection.collection.as_str() {
                MEMORY_LAYER_PROJECT => "project.jsonl",
                "project:code" => "code.jsonl",
                "project:docs" => "docs.jsonl",
                "project:events" => "events.jsonl",
                other => {
                    return Err(PortError::Failed(format!(
                        "memory_collection_unknown:{other}"
                    )))
                }
            };
            base.join("project").join(name)
        }
        MEMORY_LAYER_INSTANCE_SCRATCH => {
            let session = session_id.trim();
            if session.is_empty() {
                return Err(PortError::Failed("memory_session_required".to_owned()));
            }
            if session.contains('/') || session.contains('\\') || session.contains("..") {
                return Err(PortError::Failed("memory_session_invalid".to_owned()));
            }
            base.join("instance").join(format!("{session}.jsonl"))
        }
        other => {
            return Err(PortError::Failed(format!(
                "memory_collection_unknown:{other}"
            )))
        }
    };
    Ok(path)
}

fn kiana_home() -> Result<PathBuf, PortError> {
    let raw = std::env::var(KIANA_HOME_ENV)
        .map_err(|_| PortError::Failed("memory_home_required".to_owned()))?;
    let path = PathBuf::from(raw.trim());
    if !path.is_absolute() {
        return Err(PortError::Failed("memory_home_required".to_owned()));
    }
    Ok(path)
}

fn confined_project_root(project_root: &str) -> Result<PathBuf, PortError> {
    let root = PathBuf::from(project_root.trim());
    if project_root.trim().is_empty() || !root.is_absolute() {
        return Err(PortError::Failed("memory_project_required".to_owned()));
    }
    Ok(root)
}

fn read_records(path: &Path) -> Result<Vec<MemoryRecord>, PortError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path)
        .map_err(|error| PortError::Failed(format!("memory_read_failed:{error}")))?;
    let mut records = Vec::new();
    for line in BufReader::new(file).lines() {
        let line =
            line.map_err(|error| PortError::Failed(format!("memory_read_failed:{error}")))?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<MemoryRecord>(&line) {
            records.push(record);
        }
    }
    Ok(records)
}

fn append_record(path: &Path, record: &MemoryRecord) -> Result<(), PortError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| PortError::Failed(format!("memory_write_failed:{error}")))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| PortError::Failed(format!("memory_write_failed:{error}")))?;
    let mut encoded = serde_json::to_string(record)
        .map_err(|error| PortError::Failed(format!("memory_write_failed:{error}")))?;
    encoded.push('\n');
    file.write_all(encoded.as_bytes())
        .map_err(|error| PortError::Failed(format!("memory_write_failed:{error}")))?;
    Ok(())
}

fn text_matches(text: &str, query: &str) -> bool {
    let haystack = text.to_ascii_lowercase();
    query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .all(|term| haystack.contains(&term.to_ascii_lowercase()))
}

fn required_string(arguments: &Value, key: &str, error: &str) -> Result<String, PortError> {
    optional_string(arguments, key).ok_or_else(|| PortError::Failed(error.to_owned()))
}

fn optional_string(arguments: &Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::MEMORY_LAYERS;

    #[test]
    fn six_layers_are_catalogued() {
        assert_eq!(
            MEMORY_LAYERS,
            [
                MEMORY_LAYER_COMPANY,
                MEMORY_LAYER_DEPARTMENT,
                MEMORY_LAYER_ROLE,
                MEMORY_LAYER_PROJECT,
                MEMORY_LAYER_USER,
                MEMORY_LAYER_INSTANCE_SCRATCH,
            ]
        );
    }

    #[test]
    fn user_and_project_paths_do_not_mix() {
        let project = PathBuf::from("/tmp/kiana-memory-project");
        let user = MemoryCollection::parse("user-private").unwrap();
        let project_col = MemoryCollection::parse("project").unwrap();
        std::env::set_var(KIANA_HOME_ENV, "/tmp/kiana-memory-home");
        let user_path = collection_path(&user, project.to_str().unwrap(), "session-1").unwrap();
        let project_path =
            collection_path(&project_col, project.to_str().unwrap(), "session-1").unwrap();
        assert!(user_path.starts_with("/tmp/kiana-memory-home/memory"));
        assert!(project_path.starts_with(project.join(".kiana").join("memory")));
        assert_ne!(user_path, project_path);
        std::env::remove_var(KIANA_HOME_ENV);
    }
}

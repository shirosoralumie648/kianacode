use crate::local_state::kiana_home_dir;
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct MemoryCommand;

#[async_trait]
impl Command for MemoryCommand {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Manage memory"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let args = context.args.trim();
        let path = kiana_home_dir().join("memory.md");
        let store_path = structured_memory_path();
        let (command, rest) = split_command(args);
        match command {
            None | Some("status") => {
                let json_output = rest == "--json";
                if !rest.is_empty() && !json_output {
                    return Err(anyhow!(
                        "unknown memory status command '{}'\n\n{}",
                        rest,
                        usage()
                    ));
                }
                let bytes = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
                let records = read_memory_records(&store_path)?;
                let redaction_count: u64 = records.iter().map(|record| record.redaction_count).sum();
                if json_output {
                    return Ok(CommandResult::text(serde_json::to_string_pretty(&json!({
                        "schema": "kiana.memory-status.v1",
                        "legacy_file": path.display().to_string(),
                        "legacy_bytes": bytes,
                        "store_file": store_path.display().to_string(),
                        "record_count": records.len(),
                        "kind_counts": kind_counts(&records),
                        "redaction_count": redaction_count,
                        "latest_created_at_ms": records.iter().map(|record| record.created_at_ms).max(),
                    }))?));
                }
                Ok(CommandResult::text(format!(
                    "Memory\nfile: {}\nbytes: {}\nstructured_file: {}\nrecords: {}\nredactions: {}\nusage: kiana memory show|append <text>|search <query>|clear",
                    path.display(),
                    bytes,
                    store_path.display(),
                    records.len(),
                    redaction_count
                )))
            }
            Some("help") | Some("--help") | Some("-h") => {
                if rest.is_empty() {
                    Ok(CommandResult::text(usage()))
                } else {
                    Err(anyhow!(
                        "unknown memory help command '{}'\n\n{}",
                        rest,
                        usage()
                    ))
                }
            }
            Some("path") => {
                if !rest.is_empty() {
                    return Err(anyhow!(
                        "unknown memory path command '{}'\n\n{}",
                        rest,
                        usage()
                    ));
                }
                Ok(CommandResult::text(path.display().to_string()))
            }
            Some("show") => {
                if !rest.is_empty() {
                    return Err(anyhow!(
                        "unknown memory show command '{}'\n\n{}",
                        rest,
                        usage()
                    ));
                }
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                Ok(CommandResult::text(text))
            }
            Some("append") => {
                let ParsedAppend { kind, source, text } = parse_append_args(rest)?;
                if text.is_empty() {
                    return Err(anyhow!("memory append requires text"));
                }
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let sanitized = redact_memory_text(text);
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)?;
                writeln!(file, "{}", sanitized.text)?;
                append_structured_memory(&store_path, &kind, &source, &sanitized)?;
                Ok(CommandResult::text(format!(
                    "Memory updated\nfile: {}\nstructured_file: {}\nredactions: {}",
                    path.display(),
                    store_path.display(),
                    sanitized.redaction_count
                )))
            }
            Some("search") => {
                let (json_output, query) = parse_search_args(rest)?;
                if query.is_empty() {
                    return Err(anyhow!("memory search requires query"));
                }
                let records = read_memory_records(&store_path)?;
                let hits = search_memory_records(&records, query, 10);
                let report = json!({
                    "schema": "kiana.memory-search.v1",
                    "store_file": store_path.display().to_string(),
                    "query": query,
                    "terms": query_terms(query),
                    "limit": 10,
                    "record_count": records.len(),
                    "hits": hits,
                });
                if json_output {
                    Ok(CommandResult::text(serde_json::to_string_pretty(&report)?))
                } else {
                    Ok(CommandResult::text(render_search_report(&report)))
                }
            }
            Some("clear") => {
                if matches!(rest, "help" | "--help" | "-h") {
                    return Ok(CommandResult::text(usage()));
                }
                if !rest.is_empty() {
                    return Err(anyhow!(
                        "unknown memory clear command '{}'\n\n{}",
                        rest,
                        usage()
                    ));
                }
                if path.exists() {
                    std::fs::remove_file(&path)?;
                }
                if store_path.exists() {
                    std::fs::remove_file(&store_path)?;
                }
                Ok(CommandResult::text(format!(
                    "Memory cleared\nfile: {}\nstructured_file: {}",
                    path.display(),
                    store_path.display()
                )))
            }
            Some(other) => Err(anyhow!(
                "unknown memory command '{}'; expected status, path, show, append, search, or clear",
                other
            )),
        }
    }
}

#[derive(Debug, Clone)]
struct ParsedAppend<'a> {
    kind: String,
    source: String,
    text: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MemoryRecord {
    schema: String,
    id: String,
    kind: String,
    source: String,
    text: String,
    #[serde(default)]
    redaction_count: u64,
    created_at_ms: u128,
}

#[derive(Debug, Clone)]
struct SanitizedMemoryText {
    text: String,
    redaction_count: u64,
}

fn structured_memory_path() -> PathBuf {
    kiana_home_dir().join("memory").join("events.jsonl")
}

fn parse_append_args(value: &str) -> anyhow::Result<ParsedAppend<'_>> {
    let mut kind = "note".to_string();
    let mut source = "manual".to_string();
    let mut remaining = value.trim();
    loop {
        if let Some(rest) = remaining.strip_prefix("--kind ") {
            let (next, tail) = split_command(rest);
            let next = next.ok_or_else(|| anyhow!("memory append --kind requires value"))?;
            validate_memory_token("kind", next)?;
            kind = next.to_string();
            remaining = tail;
            continue;
        }
        if let Some(rest) = remaining.strip_prefix("--source ") {
            let (next, tail) = split_command(rest);
            let next = next.ok_or_else(|| anyhow!("memory append --source requires value"))?;
            validate_memory_token("source", next)?;
            source = next.to_string();
            remaining = tail;
            continue;
        }
        break;
    }
    Ok(ParsedAppend {
        kind,
        source,
        text: remaining,
    })
}

fn parse_search_args(value: &str) -> anyhow::Result<(bool, &str)> {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix("--json ") {
        return Ok((true, rest.trim()));
    }
    if value == "--json" {
        return Ok((true, ""));
    }
    Ok((false, value))
}

fn validate_memory_token(label: &str, value: &str) -> anyhow::Result<()> {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        Ok(())
    } else {
        Err(anyhow!("memory {label} contains unsupported characters"))
    }
}

fn append_structured_memory(
    path: &Path,
    kind: &str,
    source: &str,
    sanitized: &SanitizedMemoryText,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let created_at_ms = now_ms();
    let record = MemoryRecord {
        schema: "kiana.memory-record.v1".to_string(),
        id: format!("mem-{created_at_ms}-{}", stable_text_hash(&sanitized.text)),
        kind: kind.to_string(),
        source: source.to_string(),
        text: sanitized.text.clone(),
        redaction_count: sanitized.redaction_count,
        created_at_ms,
    };
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open structured memory store {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(&record)?)?;
    Ok(())
}

fn redact_memory_text(text: &str) -> SanitizedMemoryText {
    let mut redaction_count = 0u64;
    let redacted = text
        .split_whitespace()
        .map(|token| {
            let lower = token.to_ascii_lowercase();
            let looks_secret = lower.contains("api_key=")
                || lower.contains("apikey=")
                || lower.contains("token=")
                || lower.contains("password=")
                || lower.contains("secret=")
                || token.starts_with("sk-")
                || token.starts_with("xoxb-")
                || token.starts_with("ghp_");
            if looks_secret {
                redaction_count = redaction_count.saturating_add(1);
                "[REDACTED_SECRET]".to_string()
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    SanitizedMemoryText {
        text: redacted,
        redaction_count,
    }
}

fn read_memory_records(path: &Path) -> anyhow::Result<Vec<MemoryRecord>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()))
        }
    };
    let mut records = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record: MemoryRecord = serde_json::from_str(line)
            .with_context(|| format!("invalid memory JSONL record at line {}", index + 1))?;
        if record.schema != "kiana.memory-record.v1" {
            return Err(anyhow!(
                "unsupported memory record schema at line {}: {}",
                index + 1,
                record.schema
            ));
        }
        records.push(record);
    }
    Ok(records)
}

fn kind_counts(records: &[MemoryRecord]) -> serde_json::Map<String, Value> {
    let mut counts = std::collections::BTreeMap::<String, u64>::new();
    for record in records {
        *counts.entry(record.kind.clone()).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(key, value)| (key, json!(value)))
        .collect()
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|term| term.to_ascii_lowercase())
        .filter(|term| !term.is_empty())
        .collect()
}

fn search_memory_records(records: &[MemoryRecord], query: &str, limit: usize) -> Vec<Value> {
    let terms = query_terms(query);
    let mut hits = records
        .iter()
        .filter_map(|record| {
            let haystack =
                format!("{} {} {}", record.kind, record.source, record.text).to_ascii_lowercase();
            let matched_terms = terms
                .iter()
                .filter(|term| haystack.contains(term.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            if matched_terms.is_empty() {
                return None;
            }
            Some(json!({
                "id": record.id,
                "kind": record.kind,
                "source": record.source,
                "score": matched_terms.len(),
                "matched_terms": matched_terms,
                "text": record.text,
                "redaction_count": record.redaction_count,
                "created_at_ms": record.created_at_ms,
            }))
        })
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        right["score"]
            .as_u64()
            .cmp(&left["score"].as_u64())
            .then_with(|| {
                right["created_at_ms"]
                    .as_u64()
                    .cmp(&left["created_at_ms"].as_u64())
            })
    });
    hits.truncate(limit);
    hits
}

fn render_search_report(report: &Value) -> String {
    let query = report["query"].as_str().unwrap_or_default();
    let mut lines = vec![format!("Memory search\nquery: {query}")];
    for hit in report["hits"].as_array().into_iter().flatten() {
        lines.push(format!(
            "- [{}] {} ({})",
            hit["kind"].as_str().unwrap_or("note"),
            hit["text"].as_str().unwrap_or_default(),
            hit["source"].as_str().unwrap_or("manual")
        ));
    }
    lines.join("\n")
}

fn stable_text_hash(value: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn split_command(value: &str) -> (Option<&str>, &str) {
    let mut parts = value.splitn(2, char::is_whitespace);
    let command = parts.next().filter(|value| !value.is_empty());
    let rest = parts.next().unwrap_or_default().trim();
    (command, rest)
}

fn usage() -> &'static str {
    "Usage: kiana memory [status|status --json]\n       kiana memory path\n       kiana memory show\n       kiana memory append [--kind <kind>] [--source <source>] <text>\n       kiana memory search [--json] <query>\n       kiana memory clear"
}

#[cfg(test)]
mod tests {
    use super::MemoryCommand;
    use crate::local_state::env_lock;
    use crate::{Command, CommandContext};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::fs;
    use std::sync::{MutexGuard, PoisonError};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_home() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-memory-command-{}-{unique}",
            std::process::id()
        ))
    }

    fn context(args: &str) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state: HashMap::<String, Value>::new(),
        }
    }

    fn lock_env() -> MutexGuard<'static, ()> {
        env_lock().lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn write_memory(root: &std::path::Path) -> std::path::PathBuf {
        std::env::set_var("KIANA_HOME", root);
        fs::create_dir_all(root).unwrap();
        let path = root.join("memory.md");
        fs::write(&path, "important memory\n").unwrap();
        path
    }

    #[tokio::test]
    async fn memory_clear_rejects_extra_args_without_deleting_file() {
        let _guard = lock_env();
        let root = temp_home();
        let path = write_memory(&root);

        let error = MemoryCommand
            .execute(context("clear status"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("unknown memory clear command"));
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("important memory"));

        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn memory_clear_help_reports_usage_without_deleting_file() {
        let _guard = lock_env();
        let root = temp_home();
        let path = write_memory(&root);

        let result = MemoryCommand
            .execute(context("clear --help"))
            .await
            .unwrap();

        assert!(result.value.contains("Usage: kiana memory"));
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("important memory"));

        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn memory_append_status_and_search_json_use_structured_store() {
        let _guard = lock_env();
        let root = temp_home();
        std::env::set_var("KIANA_HOME", &root);

        MemoryCommand
            .execute(context(
                "append --kind decision --source workflow selected bounded swarm retry policy",
            ))
            .await
            .unwrap();

        let legacy = fs::read_to_string(root.join("memory.md")).unwrap();
        assert!(legacy.contains("selected bounded swarm retry policy"));

        let structured = fs::read_to_string(root.join("memory/events.jsonl")).unwrap();
        let record: Value = serde_json::from_str(structured.lines().next().unwrap()).unwrap();
        assert_eq!(record["schema"], "kiana.memory-record.v1");
        assert_eq!(record["kind"], "decision");
        assert_eq!(record["source"], "workflow");

        let status = MemoryCommand
            .execute(context("status --json"))
            .await
            .unwrap();
        let status: Value = serde_json::from_str(&status.value).unwrap();
        assert_eq!(status["schema"], "kiana.memory-status.v1");
        assert_eq!(status["record_count"], 1);
        assert_eq!(status["kind_counts"]["decision"], 1);
        assert_eq!(status["redaction_count"], 0);

        let search = MemoryCommand
            .execute(context("search --json retry policy"))
            .await
            .unwrap();
        let search: Value = serde_json::from_str(&search.value).unwrap();
        assert_eq!(search["schema"], "kiana.memory-search.v1");
        assert_eq!(search["hits"].as_array().unwrap().len(), 1);
        assert_eq!(search["hits"][0]["kind"], "decision");
        assert_eq!(search["hits"][0]["redaction_count"], 0);

        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn memory_append_redacts_obvious_secrets_before_persistence() {
        let _guard = lock_env();
        let root = temp_home();
        std::env::set_var("KIANA_HOME", &root);

        let result = MemoryCommand
            .execute(context(
                "append --kind decision token=abc123 password=hunter2 sk-abcdefghijklmnopqrstuvwxyz keep this",
            ))
            .await
            .unwrap();
        assert!(result.value.contains("redactions: 3"));

        let legacy = fs::read_to_string(root.join("memory.md")).unwrap();
        assert!(legacy.contains("[REDACTED_SECRET] [REDACTED_SECRET] [REDACTED_SECRET] keep this"));
        assert!(!legacy.contains("abc123"));
        assert!(!legacy.contains("hunter2"));
        assert!(!legacy.contains("sk-abcdefghijklmnopqrstuvwxyz"));

        let structured = fs::read_to_string(root.join("memory/events.jsonl")).unwrap();
        let record: Value = serde_json::from_str(structured.lines().next().unwrap()).unwrap();
        assert_eq!(record["schema"], "kiana.memory-record.v1");
        assert_eq!(record["redaction_count"], 3);
        assert_eq!(
            record["text"],
            "[REDACTED_SECRET] [REDACTED_SECRET] [REDACTED_SECRET] keep this"
        );
        assert!(!structured.contains("abc123"));
        assert!(!structured.contains("hunter2"));

        let status = MemoryCommand
            .execute(context("status --json"))
            .await
            .unwrap();
        let status: Value = serde_json::from_str(&status.value).unwrap();
        assert_eq!(status["redaction_count"], 3);

        let search = MemoryCommand
            .execute(context("search --json keep"))
            .await
            .unwrap();
        let search: Value = serde_json::from_str(&search.value).unwrap();
        assert_eq!(search["hits"][0]["redaction_count"], 3);
        assert!(!search.to_string().contains("abc123"));
        assert!(!search.to_string().contains("hunter2"));

        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn memory_clear_removes_legacy_and_structured_store() {
        let _guard = lock_env();
        let root = temp_home();
        std::env::set_var("KIANA_HOME", &root);

        MemoryCommand
            .execute(context("append keep this"))
            .await
            .unwrap();
        assert!(root.join("memory.md").exists());
        assert!(root.join("memory/events.jsonl").exists());

        MemoryCommand.execute(context("clear")).await.unwrap();

        assert!(!root.join("memory.md").exists());
        assert!(!root.join("memory/events.jsonl").exists());

        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn memory_status_fails_closed_on_corrupt_structured_store() {
        let _guard = lock_env();
        let root = temp_home();
        std::env::set_var("KIANA_HOME", &root);
        fs::create_dir_all(root.join("memory")).unwrap();
        fs::write(root.join("memory/events.jsonl"), "not-json\n").unwrap();

        let error = MemoryCommand
            .execute(context("status --json"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("invalid memory JSONL record"));

        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn memory_append_rejects_invalid_kind_without_writing() {
        let _guard = lock_env();
        let root = temp_home();
        std::env::set_var("KIANA_HOME", &root);

        let error = MemoryCommand
            .execute(context("append --kind bad/kind should not write"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("memory kind contains unsupported characters"));
        assert!(!root.join("memory.md").exists());
        assert!(!root.join("memory/events.jsonl").exists());

        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("KIANA_HOME");
    }
}

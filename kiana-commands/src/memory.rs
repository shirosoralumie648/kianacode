use crate::local_state::kiana_home_dir;
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use kiana_domain::{
    memory_query_terms, retrieval_scope_digest, EvidenceStatus, Freshness, RetrievalHealth,
    RetrievalItem, RetrievalItemProvenance, RetrievalProfile, RetrievalRequest,
    RETRIEVAL_UNVERSIONED_GENERATION,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
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
                let outcome = search_memory_records(&records, query, 10)?;
                let report = json!({
                    "schema": "kiana.memory-search.v1",
                    "store_file": store_path.display().to_string(),
                    "query": query,
                    "terms": memory_query_terms(query),
                    "limit": 10,
                    "record_count": records.len(),
                    "hits": outcome.hits,
                    // CM-14：空结果/降级/拒绝必须显式可见，而不是只给一个空 hits 数组。
                    "retrieval": {
                        "health": outcome.health,
                        "empty_reason": outcome.empty_reason,
                    },
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

/// 一次 CLI memory 检索的命中投影与健康标签。
///
/// `hits` 是既有的 JSON hit 形状（新增 `retrieval_evidence`）；`health`/`empty_reason` 来自统一检索
/// 结果，说明本次读取是否完成、为什么为空。
struct MemorySearchOutcome {
    hits: Vec<Value>,
    health: RetrievalHealth,
    empty_reason: Option<String>,
}

/// 用统一的 sparse/exact 排名合同对本地记忆记录排序。
///
/// 【作用】CLI 不再自带“词项包含计数 + 时间”排序：这里把每条记录投影成 `RetrievalItem`（匹配面与旧
/// 实现一致地包含 kind/source/text，摘要用记录正文的 SHA-256，来源证据用 `local-memory:{id}` 与内容摘要
/// revision），然后交给 `kiana_domain::rank_retrieval` 用同一 `RetrievalProfile::unified()`
/// （exact/BM25-CJK → RRF60 → MMR）排名，最后映射回既有 hit 形状并附带命中级 `retrieval_evidence`。
/// 【输入/输出】本地结构化记录、查询文本与上限；成功返回 hits（既有字段 + `retrieval_evidence`）与派生
/// health/空结果原因。
/// 【失败】请求或条目畸形（例如手改文件里的空 id）、候选或上限越界时返回错误，不静默降级成部分结果。
/// 【边界】CLI 检索没有服务端 ACL 与 dense 通道：`scope` 摘要只把同一次本地记录快照绑定到 request/item，
/// 不是权限判断；本地记录读取由 `kiana memory` 自身范围决定。本地存储不维护代际计数器，因此 generation
/// 使用文档化的 `RETRIEVAL_UNVERSIONED_GENERATION` 标签，freshness 只是只读标签。
fn search_memory_records(
    records: &[MemoryRecord],
    query: &str,
    limit: usize,
) -> anyhow::Result<MemorySearchOutcome> {
    let scope_labels = records
        .iter()
        .map(|record| record.id.clone())
        .collect::<Vec<_>>();
    let permission_scope_digest = retrieval_scope_digest(scope_labels.iter().map(String::as_str));
    let items = records
        .iter()
        .map(|record| {
            let text_digest = format!("sha256:{:x}", Sha256::digest(record.text.as_bytes()));
            Ok(RetrievalItem {
                id: record.id.clone(),
                text: format!("{} {} {}", record.kind, record.source, record.text),
                source_digest: text_digest.clone(),
                permission_scope_digest: permission_scope_digest.clone(),
                provenance: RetrievalItemProvenance::new(
                    format!("local-memory:{}", record.id),
                    format!("content-digest:{text_digest}"),
                    RETRIEVAL_UNVERSIONED_GENERATION,
                    Freshness::Current,
                    if record.source.trim().is_empty() {
                        EvidenceStatus::Unverifiable
                    } else {
                        EvidenceStatus::Attributed
                    },
                )
                .map_err(|error| anyhow!("memory retrieval provenance invalid: {error}"))?,
                allowed: true,
                created_at_ms: u64::try_from(record.created_at_ms).unwrap_or(u64::MAX),
                dense_vector: None,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let request = RetrievalRequest::new(query, permission_scope_digest, None)
        .map_err(|error| anyhow!("memory retrieval request invalid: {error}"))?;
    let result =
        kiana_domain::rank_retrieval(&RetrievalProfile::unified(), &request, &items, limit)
            .map_err(|error| anyhow!("memory retrieval failed: {error}"))?;
    let mut by_id = std::collections::BTreeMap::new();
    for (index, record) in records.iter().enumerate() {
        by_id.entry(record.id.as_str()).or_insert(index);
    }
    let mut hits = Vec::with_capacity(result.hits.len());
    for hit in &result.hits {
        let record = by_id
            .get(hit.id.as_str())
            .map(|index| &records[*index])
            .ok_or_else(|| anyhow!("memory retrieval hit without record"))?;
        hits.push(json!({
            "id": record.id,
            "kind": record.kind,
            "source": record.source,
            "score": hit.matched_terms.len(),
            "matched_terms": hit.matched_terms,
            "text": record.text,
            "redaction_count": record.redaction_count,
            "created_at_ms": record.created_at_ms,
            // CM-14：命中级来源证据（source ref/revision/generation/freshness/evidence/rank components）。
            "retrieval_evidence": hit.provenance,
        }));
    }
    Ok(MemorySearchOutcome {
        hits,
        health: result.health.clone(),
        empty_reason: result.empty_reason.clone(),
    })
}

fn render_search_report(report: &Value) -> String {
    let query = report["query"].as_str().unwrap_or_default();
    let mut lines = vec![format!("Memory search\nquery: {query}")];
    let hits = report["hits"].as_array();
    if hits.is_none_or(|hits| hits.is_empty()) {
        // CM-14：空结果必须解释原因（无记录/无匹配/limit 为 0），而不是静默输出空列表。
        let reason = report["retrieval"]["empty_reason"]
            .as_str()
            .unwrap_or("no_query_match");
        lines.push(format!("(no hits: {reason})"));
    }
    for hit in hits.into_iter().flatten() {
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
    use sha2::{Digest, Sha256};
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
        // CM-14：命中的来源证据与检索 health 一起可见；空结果原因只在空结果时出现。
        assert_eq!(search["retrieval"]["health"]["status"], "ready");
        assert_eq!(search["retrieval"]["empty_reason"], Value::Null);
        let evidence = &search["hits"][0]["retrieval_evidence"];
        assert_eq!(evidence["generation"], 1);
        assert_eq!(evidence["freshness"], "current");
        assert_eq!(evidence["evidence"], "attributed");
        assert!(evidence["source_ref"]
            .as_str()
            .is_some_and(|value| value.starts_with("local-memory:mem-")));
        assert!(evidence["source_revision"]
            .as_str()
            .is_some_and(|value| value.starts_with("content-digest:sha256:")));

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

    fn parity_record(
        id: &str,
        kind: &str,
        source: &str,
        text: &str,
        created_at_ms: u128,
    ) -> super::MemoryRecord {
        super::MemoryRecord {
            schema: "kiana.memory-record.v1".to_string(),
            id: id.to_string(),
            kind: kind.to_string(),
            source: source.to_string(),
            text: text.to_string(),
            redaction_count: 0,
            created_at_ms,
        }
    }

    #[test]
    /// CM-12：CLI 搜索必须与统一算法给出同一顺序，而不是旧的“词项计数 + 时间”排序。
    /// 两条记录命中相同词项数、但正文长度不同：BM25 长度归一化让短记录排在前面，
    /// 旧排序会因 created_at 更大而先返回长记录。
    fn cli_memory_search_matches_the_unified_retrieval_order() {
        let records = vec![
            parity_record(
                "mem-long",
                "decision",
                "workflow",
                "checkout deployment rollback notes",
                30,
            ),
            parity_record("mem-short", "note", "manual", "checkout deployment", 20),
            parity_record("mem-other", "note", "manual", "界面 颜色 说明", 10),
        ];
        let outcome =
            super::search_memory_records(&records, "checkout deployment", 5).expect("CLI ranking");
        let hits = &outcome.hits;
        let ids = hits
            .iter()
            .map(|hit| hit["id"].as_str().unwrap_or_default().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["mem-short", "mem-long"]);
        for hit in hits {
            assert_eq!(hit["score"], 2);
        }

        // 同一批记录投影到统一算法，CLI 顺序必须与 port/domain 结果一致。
        let scope_labels = records
            .iter()
            .map(|record| record.id.clone())
            .collect::<Vec<_>>();
        let scope = super::retrieval_scope_digest(scope_labels.iter().map(String::as_str));
        let items = records
            .iter()
            .map(|record| {
                let text_digest = format!("sha256:{:x}", Sha256::digest(record.text.as_bytes()));
                super::RetrievalItem {
                    id: record.id.clone(),
                    text: format!("{} {} {}", record.kind, record.source, record.text),
                    source_digest: text_digest.clone(),
                    permission_scope_digest: scope.clone(),
                    // CM-14：与生产路径同源的来源证据，才能逐字段比对命中证据。
                    provenance: super::RetrievalItemProvenance::new(
                        format!("local-memory:{}", record.id),
                        format!("content-digest:{text_digest}"),
                        super::RETRIEVAL_UNVERSIONED_GENERATION,
                        super::Freshness::Current,
                        super::EvidenceStatus::Attributed,
                    )
                    .expect("parity provenance"),
                    allowed: true,
                    created_at_ms: u64::try_from(record.created_at_ms).unwrap_or(u64::MAX),
                    dense_vector: None,
                }
            })
            .collect::<Vec<_>>();
        let request = super::RetrievalRequest::new("checkout deployment", scope, None)
            .expect("parity request");
        let expected =
            kiana_domain::rank_retrieval(&super::RetrievalProfile::unified(), &request, &items, 5)
                .expect("unified ranking");
        let expected_ids = expected
            .hits
            .iter()
            .map(|hit| hit.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(ids, expected_ids);
        assert_eq!(outcome.health, expected.health);
        for (hit, expected_hit) in hits.iter().zip(&expected.hits) {
            assert_eq!(
                hit["matched_terms"],
                serde_json::json!(expected_hit.matched_terms)
            );
            // CM-14：命中级来源证据必须与统一算法结果逐字段一致。
            assert_eq!(
                hit["retrieval_evidence"],
                serde_json::json!(expected_hit.provenance)
            );
        }
    }

    #[test]
    /// CM-14：空结果必须给出原因（无记录/无匹配），命中结果必须带来源证据；两者都不能静默省略。
    fn cli_memory_search_explains_empty_results_and_keeps_hit_evidence() {
        let records = vec![parity_record(
            "mem-color",
            "note",
            "manual",
            "界面 颜色 说明",
            10,
        )];
        let empty =
            super::search_memory_records(&records, "checkout deployment", 5).expect("CLI ranking");
        assert!(empty.hits.is_empty());
        assert_eq!(empty.empty_reason.as_deref(), Some("no_query_match"));
        assert_eq!(
            empty.health.status,
            kiana_domain::RetrievalHealthStatus::Ready
        );

        // 没有记录时原因是“范围内没有候选”，而不是伪装成“查询无匹配”。
        let store_empty =
            super::search_memory_records(&[], "checkout deployment", 5).expect("CLI empty store");
        assert!(store_empty.hits.is_empty());
        assert_eq!(
            store_empty.empty_reason.as_deref(),
            Some("no_candidates_in_scope")
        );

        // 命中路径：来源证据使用无版本存储标签与内容摘要 revision，且 generation 非零。
        let hit_outcome = super::search_memory_records(&records, "颜色", 5).expect("CLI ranking");
        assert_eq!(hit_outcome.hits.len(), 1);
        assert_eq!(hit_outcome.empty_reason, None);
        let evidence = &hit_outcome.hits[0]["retrieval_evidence"];
        assert_eq!(evidence["generation"], 1);
        assert_eq!(evidence["freshness"], "current");
        assert_eq!(evidence["evidence"], "attributed");
        assert_eq!(evidence["source_ref"], "local-memory:mem-color");
        assert!(evidence["source_revision"]
            .as_str()
            .is_some_and(|value| value.starts_with("content-digest:sha256:")));
        assert!(evidence["rank_components"]["rrf"].is_number());
    }
}

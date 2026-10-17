//! Local deterministic BM25, pinned dense token vectors, RRF and MMR.
//! No network or hash-generated embeddings. ONNX inference is not implemented here.
//!
//! 2026-10-17（CM-12）：本模块不再自带第二套排名公式。`rank_records()` 把已由
//! `harness_memory::search_records_scoped` 完成 collection/role 授权、投影 fence 与 supersede 过滤的候选
//! 投影成 `RetrievalItem`，连同可选 dense 向量交给 `kiana_query::UnifiedRetrievalPort`（Domain
//! `rank_retrieval()` 的唯一实现）排名，再把 `RetrievalResult` 映射回既有 JSON hit 形状。本模块继续负责
//! dense 模型加载、fail-closed 校验、降级原因和兼容输出字段。
use kiana_domain::{
    memory_tokens, retrieval_scope_digest, MemoryRecord, RetrievalItem, RetrievalProfile,
    RetrievalRequest,
};
use kiana_ports::{PortError, RetrievalPort};
use kiana_query::UnifiedRetrievalPort;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const MODEL_LIMIT: u64 = 64 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelManifest {
    schema: String,
    model_id: String,
    version: String,
    sha256: String,
    dimensions: usize,
    format: String,
    path: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenVectors {
    schema: String,
    vectors: BTreeMap<String, Vec<f64>>,
}
struct DenseModel {
    id: String,
    sha256: String,
    dimensions: usize,
    vectors: BTreeMap<String, Vec<f64>>,
}
impl DenseModel {
    fn embed(&self, tokens: &[String]) -> Option<Vec<f64>> {
        let mut vector = vec![0.0; self.dimensions];
        let mut matched = false;
        for token in tokens {
            if let Some(row) = self.vectors.get(token) {
                matched = true;
                for (value, weight) in vector.iter_mut().zip(row) {
                    *value += weight;
                }
            }
        }
        let norm = vector.iter().map(|x| x * x).sum::<f64>().sqrt();
        if !matched || norm == 0.0 || !norm.is_finite() {
            return None;
        }
        vector.iter_mut().for_each(|x| *x /= norm);
        Some(vector)
    }
}
fn fail(reason: impl Into<String>) -> PortError {
    PortError::Failed(reason.into())
}
fn read_local(path: &Path) -> Result<Vec<u8>, PortError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let mut file = options
        .open(path)
        .map_err(|_| fail("memory_embedding_file_unavailable"))?;
    let meta = file
        .metadata()
        .map_err(|_| fail("memory_embedding_file_unavailable"))?;
    if !meta.is_file() || meta.len() > MODEL_LIMIT {
        return Err(fail("memory_embedding_file_invalid"));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MODEL_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| fail("memory_embedding_file_unavailable"))?;
    if bytes.len() as u64 > MODEL_LIMIT {
        return Err(fail("memory_embedding_file_invalid"));
    }
    Ok(bytes)
}
fn load_model() -> Result<(Option<DenseModel>, Option<String>), PortError> {
    let Some(path) = std::env::var_os("KIANA_MEMORY_EMBEDDING_MANIFEST")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
    else {
        return Ok((None, Some("embedding_not_configured".to_owned())));
    };
    if !path.is_absolute() {
        return Err(fail("memory_embedding_manifest_absolute_path_required"));
    }
    if !path
        .try_exists()
        .map_err(|_| fail("memory_embedding_file_unavailable"))?
    {
        return Ok((None, Some("embedding_model_missing".to_owned())));
    }
    let manifest: ModelManifest = serde_json::from_slice(&read_local(&path)?)
        .map_err(|_| fail("memory_embedding_manifest_invalid"))?;
    if manifest.schema != "kiana.embedding-model.v1"
        || manifest.model_id.trim().is_empty()
        || manifest.version.trim().is_empty()
        || !(1..=2048).contains(&manifest.dimensions)
        || manifest.sha256.len() != 64
        || !manifest.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(fail("memory_embedding_manifest_invalid"));
    }
    if manifest.format != "token-vectors" {
        return Err(fail(format!(
            "memory_embedding_format_not_supported:{}",
            manifest.format
        )));
    }
    let model_path = if manifest.path.is_absolute() {
        manifest.path.clone()
    } else {
        path.parent()
            .ok_or_else(|| fail("memory_embedding_manifest_invalid"))?
            .join(&manifest.path)
    };
    if !model_path
        .try_exists()
        .map_err(|_| fail("memory_embedding_file_unavailable"))?
    {
        return Ok((None, Some("embedding_model_missing".to_owned())));
    }
    let bytes = read_local(&model_path)?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    if sha256 != manifest.sha256.to_ascii_lowercase() {
        return Err(fail("memory_embedding_hash_mismatch"));
    }
    let model: TokenVectors =
        serde_json::from_slice(&bytes).map_err(|_| fail("memory_embedding_model_invalid"))?;
    if model.schema != "kiana.embedding.token-vectors.v1"
        || model.vectors.is_empty()
        || model.vectors.iter().any(|(term, row)| {
            term.is_empty()
                || row.len() != manifest.dimensions
                || row.iter().any(|x| !x.is_finite() || x.abs() > 1_000_000.0)
        })
    {
        return Err(fail("memory_embedding_model_invalid"));
    }
    Ok((
        Some(DenseModel {
            id: format!("{}@{}", manifest.model_id, manifest.version),
            sha256,
            dimensions: manifest.dimensions,
            vectors: model.vectors,
        }),
        None,
    ))
}
pub(crate) fn tokenize(text: &str) -> Vec<String> {
    memory_tokens(text)
}
/// 计算记录正文的无密钥内容摘要，用作 `RetrievalItem::source_digest`。
///
/// 这里对当前读到的正文重新取 SHA-256：它绑定的是本次排名输入的内容，不是来源认证，也不替代上游
/// projection/journal 校验；记录自称的 `content_hash` 与正文不一致时，本摘要仍只反映正文本身。
fn text_digest(text: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(text.as_bytes()))
}

/// 对已授权候选快照执行统一检索排名，并把结果映射回 memory tool 的 JSON hit 形状。
///
/// 【作用】把记录正文投影成稀疏文档，把可选 dense 向量（由 `KIANA_MEMORY_EMBEDDING_MANIFEST` 指定的
/// token-vectors 模型求和归一化得到）投影成条目/查询向量，然后交给
/// `kiana_query::UnifiedRetrievalPort`（Domain `rank_retrieval()` 的唯一实现）执行
/// exact/BM25-CJK → 可选 dense → RRF60 → MMR 的确定性排名。
/// 【调用者】`harness_memory::search_records_scoped`；该调用者必须先完成 collection/role 授权、投影 fence
/// 与 supersede 过滤，本函数只排名传入的候选。
/// 【输入/输出】已筛选的 `MemoryRecord`、查询文本和上限；成功返回与旧实现同形状的 hit（id/layer/
/// collection/revision 等来自 `MemoryRecord::hit()`，另加 score、matched_terms、score_components、
/// embedding_model_*、degraded/degraded_reason 和统一算法标签）。
/// 【失败】模型文件/manifest 非法、记录无法投影成合法检索条目、请求或上限越界时返回 `PortError::Failed`；
/// 畸形记录让整次检索失败，而不是静默丢弃。
/// 【降级】未配置 manifest（`embedding_not_configured`）、模型文件缺失（`embedding_model_missing`）或查询
/// 词全部不在模型词表（`embedding_query_out_of_vocabulary`）时只走稀疏通道，并把原因写入每个 hit 与
/// `degraded=true`；请求要求 dense 但没有任何有效条目向量时采用统一算法报出的降级原因。
/// 【边界】`scope` 摘要由候选快照标签派生，只用于把 request/item 绑定到同一次快照，不是权限判断；本函数
/// 不读取授权状态、不写 EventLog、不执行工具，也不证明排名质量或外部向量模型的语义正确性。
pub(crate) fn rank_records(
    records: &[MemoryRecord],
    query: &str,
    limit: usize,
) -> Result<Vec<Value>, PortError> {
    let (model, mut degraded_reason) = load_model()?;
    let query_tokens = tokenize(query);
    let query_vector = model.as_ref().and_then(|model| model.embed(&query_tokens));
    if model.is_some() && query_vector.is_none() {
        degraded_reason = Some("embedding_query_out_of_vocabulary".to_owned());
    }
    // 快照 scope 标签：候选已经过上游授权与 fence 过滤；这里只把 project/layer/collection 规范成摘要，
    // 让 request 与每个 item 绑定同一次候选快照。
    let scope_labels = records
        .iter()
        .map(|record| {
            format!(
                "{}|{}|{}",
                record.project_root, record.layer, record.collection
            )
        })
        .collect::<Vec<_>>();
    let permission_scope_digest = retrieval_scope_digest(scope_labels.iter().map(String::as_str));
    let items = records
        .iter()
        .map(|record| RetrievalItem {
            id: record.id.clone(),
            text: record.text.clone(),
            source_digest: text_digest(&record.text),
            permission_scope_digest: permission_scope_digest.clone(),
            allowed: true,
            created_at_ms: record.created_at_ms,
            dense_vector: model
                .as_ref()
                .and_then(|model| model.embed(&tokenize(&record.text))),
        })
        .collect::<Vec<_>>();
    let request = RetrievalRequest::new(query, permission_scope_digest, query_vector)
        .map_err(|error| fail(format!("memory_retrieval_request_invalid:{error}")))?;
    let port = UnifiedRetrievalPort::new(RetrievalProfile::unified())
        .map_err(|error| fail(format!("memory_retrieval_profile_invalid:{error}")))?;
    let result = port
        .retrieve(&request, &items, limit)
        .map_err(|error| fail(format!("memory_retrieval_failed:{error}")))?;
    let reason = degraded_reason.or_else(|| result.degraded_reasons.first().cloned());
    let mut by_id = BTreeMap::new();
    for (index, record) in records.iter().enumerate() {
        by_id.entry(record.id.as_str()).or_insert(index);
    }
    let mut results = Vec::with_capacity(result.hits.len());
    for hit in &result.hits {
        let Some(index) = by_id.get(hit.id.as_str()).copied() else {
            return Err(fail("memory_retrieval_hit_unknown"));
        };
        let mut output = records[index].hit();
        output["score"] = json!(hit.matched_terms.len());
        output["matched_terms"] = json!(hit.matched_terms);
        output["score_components"] = json!({
            "bm25": hit.bm25_score,
            "bm25_rank": hit.sparse_rank,
            "dense": hit.dense_score,
            "dense_rank": hit.dense_rank,
            "rrf": hit.rrf_score,
            "mmr": hit.mmr_score
        });
        output["embedding_model_id"] = json!(model.as_ref().map(|model| &model.id));
        output["embedding_model_sha256"] = json!(model.as_ref().map(|model| &model.sha256));
        output["degraded"] = json!(reason.is_some());
        output["degraded_reason"] = json!(reason);
        output["retrieval_algorithm"] = json!(result.algorithm_version);
        results.push(output);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENVIRONMENT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct EnvGuard {
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var_os("KIANA_MEMORY_EMBEDDING_MANIFEST");
            std::env::set_var("KIANA_MEMORY_EMBEDDING_MANIFEST", value);
            Self { previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var("KIANA_MEMORY_EMBEDDING_MANIFEST", value),
                None => std::env::remove_var("KIANA_MEMORY_EMBEDDING_MANIFEST"),
            }
        }
    }

    fn record(id: &str, text: &str, created_at_ms: u64) -> MemoryRecord {
        MemoryRecord {
            schema: kiana_domain::MEMORY_RECORD_SCHEMA_V2.to_owned(),
            id: id.to_owned(),
            layer: "project".to_owned(),
            collection: "project:fixture".to_owned(),
            text: text.to_owned(),
            created_at_ms,
            ..MemoryRecord::default()
        }
    }

    #[test]
    fn hybrid_retrieval_is_deterministic_for_a_pinned_model() {
        let _lock = ENVIRONMENT_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("kiana-memory-hybrid-{stamp}"));
        fs::create_dir_all(&root).expect("create fixture directory");
        let model_path = root.join("fixture-vectors.json");
        let manifest_path = root.join("manifest.json");
        let model_bytes = serde_json::to_vec(&json!({
            "schema": "kiana.embedding.token-vectors.v1",
            "vectors": {
                "部署": [1.0, 0.0, 0.0],
                "署策": [0.8, 0.2, 0.0],
                "策略": [0.0, 1.0, 0.0],
                "策略化": [0.0, 0.9, 0.1]
            }
        }))
        .expect("encode fixture vectors");
        fs::write(&model_path, &model_bytes).expect("write fixture vectors");
        let digest = format!("{:x}", Sha256::digest(&model_bytes));
        fs::write(
            &manifest_path,
            serde_json::to_vec(&json!({
                "schema": "kiana.embedding-model.v1",
                "model_id": "fixture-embedder",
                "version": "2026-09-18",
                "sha256": digest,
                "dimensions": 3,
                "format": "token-vectors",
                "path": "fixture-vectors.json"
            }))
            .expect("encode fixture manifest"),
        )
        .expect("write fixture manifest");

        let _env = EnvGuard::set(&manifest_path);
        let records = vec![
            record("matching", "部署策略需要可复核收据", 2),
            record("related", "部署记录保留审计证据", 1),
            record("unrelated", "只讨论界面颜色", 3),
        ];
        let first = rank_records(&records, "部署策略", 3).expect("first hybrid ranking");
        let second = rank_records(&records, "部署策略", 3).expect("second hybrid ranking");
        assert_eq!(first, second);
        assert_eq!(first[0]["id"], "matching");
        assert_eq!(
            first[0]["embedding_model_id"],
            "fixture-embedder@2026-09-18"
        );
        assert_eq!(first[0]["embedding_model_sha256"], digest);
        assert_eq!(first[0]["degraded"], false);
        // CM-12：命中标签改成 Domain 统一 profile 记录的算法版本；memory tool 不再自称第二套公式。
        assert_eq!(
            first[0]["retrieval_algorithm"],
            RetrievalProfile::unified().algorithm_version
        );
        // 同一算法版本也是 `UnifiedRetrievalPort` 默认 profile 的标签，CLI/query 适配器读同一个值。
        assert_eq!(
            first[0]["retrieval_algorithm"],
            UnifiedRetrievalPort::default().profile().algorithm_version
        );
        // 两条命中都来自同一 profile；退化原因在没有 manifest 时由本模块补写，这里是 dense 正常路径。
        assert_eq!(first.len(), 2);
        assert!(first[1]["score_components"]["dense_rank"].is_u64());

        drop(_env);
        fs::remove_dir_all(root).expect("remove fixture directory");
    }

    #[test]
    /// CM-12：memory tool 的排名必须与同一个 `RetrievalPort` 实现逐条一致（顺序、RRF/MMR 与降级原因），
    /// 而不是只“看起来相似”。两侧使用同一 records、查询和同一 manifest 模型。
    fn memory_tool_ranking_matches_the_unified_retrieval_port() {
        let _lock = ENVIRONMENT_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("kiana-memory-parity-{stamp}"));
        fs::create_dir_all(&root).expect("create fixture directory");
        let model_path = root.join("fixture-vectors.json");
        let manifest_path = root.join("manifest.json");
        let model_bytes = serde_json::to_vec(&json!({
            "schema": "kiana.embedding.token-vectors.v1",
            "vectors": {
                "部署": [1.0, 0.0, 0.0],
                "署策": [0.8, 0.2, 0.0],
                "策略": [0.0, 1.0, 0.0]
            }
        }))
        .expect("encode fixture vectors");
        fs::write(&model_path, &model_bytes).expect("write fixture vectors");
        let digest = format!("{:x}", Sha256::digest(&model_bytes));
        fs::write(
            &manifest_path,
            serde_json::to_vec(&json!({
                "schema": "kiana.embedding-model.v1",
                "model_id": "fixture-embedder",
                "version": "2026-09-18",
                "sha256": digest,
                "dimensions": 3,
                "format": "token-vectors",
                "path": "fixture-vectors.json"
            }))
            .expect("encode fixture manifest"),
        )
        .expect("write fixture manifest");

        let _env = EnvGuard::set(&manifest_path);
        let records = vec![
            record("alpha", "部署策略需要收据", 3),
            record("beta", "部署记录保留审计证据", 2),
            record("gamma", "只讨论界面颜色", 1),
        ];
        let hits = rank_records(&records, "部署策略", 3).expect("memory tool ranking");

        // 用同一模型和同一批记录重建 port 的输入，证明两条路径共享同一排名结果。
        let (model, _) = load_model().expect("reload pinned model");
        let model = model.expect("fixture model configured");
        let scope_labels = records
            .iter()
            .map(|record| {
                format!(
                    "{}|{}|{}",
                    record.project_root, record.layer, record.collection
                )
            })
            .collect::<Vec<_>>();
        let scope = retrieval_scope_digest(scope_labels.iter().map(String::as_str));
        let items = records
            .iter()
            .map(|record| RetrievalItem {
                id: record.id.clone(),
                text: record.text.clone(),
                source_digest: text_digest(&record.text),
                permission_scope_digest: scope.clone(),
                allowed: true,
                created_at_ms: record.created_at_ms,
                dense_vector: model.embed(&tokenize(&record.text)),
            })
            .collect::<Vec<_>>();
        let request = RetrievalRequest::new("部署策略", scope, model.embed(&tokenize("部署策略")))
            .expect("build parity request");
        let expected = UnifiedRetrievalPort::default()
            .retrieve(&request, &items, 3)
            .expect("port ranking");

        let observed = hits
            .iter()
            .map(|hit| hit["id"].as_str().unwrap_or_default().to_owned())
            .collect::<Vec<_>>();
        let port_ids = expected
            .hits
            .iter()
            .map(|hit| hit.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(observed, port_ids);
        assert_eq!(observed, vec!["alpha", "beta"]);
        for (hit, port_hit) in hits.iter().zip(&expected.hits) {
            assert_eq!(hit["score_components"]["rrf"], json!(port_hit.rrf_score));
            assert_eq!(hit["score_components"]["mmr"], json!(port_hit.mmr_score));
            assert_eq!(hit["score_components"]["bm25"], json!(port_hit.bm25_score));
            assert_eq!(
                hit["degraded"],
                json!(!expected.degraded_reasons.is_empty())
            );
        }

        drop(_env);
        fs::remove_dir_all(root).expect("remove fixture directory");
    }
}

//! Local deterministic BM25, pinned dense token vectors, RRF and MMR.
//! No network or hash-generated embeddings. ONNX inference is not implemented here.
//!
//! 2026-10-17（CM-12）：本模块不再自带第二套排名公式。`rank_records()` 把已由
//! `harness_memory::search_records_scoped` 完成 collection/role 授权、投影 fence 与 supersede 过滤的候选
//! 投影成 `RetrievalItem`，连同可选 dense 向量交给 `kiana_query::UnifiedRetrievalPort`（Domain
//! `rank_retrieval()` 的唯一实现）排名，再把 `RetrievalResult` 映射回既有 JSON hit 形状。本模块继续负责
//! dense 模型加载、fail-closed 校验、降级原因和兼容输出字段。
//! 2026-10-17（CM-14）：每个候选带上 `RetrievalItemProvenance`（source ref/revision/快照 generation/
//! freshness/evidence），命中的 `RetrievalEvidence` 与结果 `RetrievalHealth` 一起出现在 tool 输出里；
//! embedding 不可用等确定性降级会写成 Degraded + 原因且不可重试。
use kiana_domain::{
    memory_tokens, retrieval_scope_digest, EvidenceStatus, Freshness, MemoryOrigin, MemoryRecord,
    RetrievalHealth, RetrievalItem, RetrievalItemProvenance, RetrievalProfile, RetrievalRequest,
    RetrievalRetryPolicy,
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

/// 本次排名的快照标签：数据治理 epoch 与 freshness 判定时间。
///
/// 【作用】让同一次检索的所有候选共享一个非零 generation 标签（`data_epoch`），并使用同一个时间基准
/// 判定记录 validity 窗口，避免逐条读取时钟造成同一次结果内部不一致。
/// 【边界】`data_epoch` 来自调用方的 data policy，不是索引 generation；`now_ms` 只用于派生只读
/// freshness 标签，不做新鲜度探测。
#[derive(Clone, Copy, Debug)]
pub(crate) struct RetrievalSnapshotContext {
    pub data_epoch: u64,
    pub now_ms: u64,
}

/// 一次 memory 检索的命中投影与健康状态。
///
/// `hits` 是既有的 JSON hit 形状（新增 `retrieval_evidence`），`health` 是 Domain `RetrievalHealth`：
/// Ready/Degraded/Denied/Unavailable 加原因与显式重试边界，供 tool 输出与调用者判断是否值得重试。
pub(crate) struct RankedRecords {
    pub hits: Vec<Value>,
    pub health: RetrievalHealth,
}

/// 由 memory 记录派生候选来源证据。
///
/// 【规则】`source_ref` 用 `memory:{layer}:{collection}:{id}`；`source_revision` 用记录 revision 计数标签；
/// `generation` 取调用方的快照 data epoch（同一次检索所有候选一致，不是每条记录各自的 revision）；
/// `freshness` 由 validity 窗口与快照时间派生（`valid_to_ms <= now_ms` → Stale，其余 → Current）；
/// `evidence` 只区分 Attributed/Unverifiable（origin 未知或 source 为空 → Unverifiable），从不声称
/// Verified。
/// 【失败】记录带非法 generation（epoch 为 0）或畸形来源标签时返回 `PortError::Failed`，整次检索
/// fail-closed。
/// 【边界】这些是只读派生标签：不重新读来源、不重新授权、不做新鲜度探测。
fn record_provenance(
    record: &MemoryRecord,
    context: RetrievalSnapshotContext,
) -> Result<RetrievalItemProvenance, PortError> {
    let freshness = match record.validity.valid_to_ms {
        Some(valid_to) if valid_to <= context.now_ms => Freshness::Stale,
        _ => Freshness::Current,
    };
    let evidence = if record.origin == MemoryOrigin::Unknown || record.source.trim().is_empty() {
        EvidenceStatus::Unverifiable
    } else {
        EvidenceStatus::Attributed
    };
    RetrievalItemProvenance::new(
        format!(
            "memory:{}:{}:{}",
            record.layer, record.collection, record.id
        ),
        format!("revision:{}", record.revision),
        context.data_epoch,
        freshness,
        evidence,
    )
    .map_err(|error| fail(format!("memory_retrieval_provenance_invalid:{error}")))
}

/// 对已授权候选快照执行统一检索排名，并把结果映射回 memory tool 的 JSON hit 形状。
///
/// 【作用】把记录正文投影成稀疏文档，把可选 dense 向量（由 `KIANA_MEMORY_EMBEDDING_MANIFEST` 指定的
/// token-vectors 模型求和归一化得到）投影成条目/查询向量，然后交给
/// `kiana_query::UnifiedRetrievalPort`（Domain `rank_retrieval()` 的唯一实现）执行
/// exact/BM25-CJK → 可选 dense → RRF60 → MMR 的确定性排名。
/// 【调用者】`harness_memory::search_records_scoped`；该调用者必须先完成 collection/role 授权、投影 fence
/// 与 supersede 过滤，本函数只排名传入的候选。
/// 【输入/输出】已筛选的 `MemoryRecord`、查询文本、上限与快照标签（data epoch/时间）；成功返回与旧实现
/// 同形状的 hit（id/layer/collection/revision 等来自 `MemoryRecord::hit()`，另加 score、matched_terms、
/// score_components、`retrieval_evidence`、embedding_model_*、degraded/degraded_reason 和统一算法标签）
/// 以及带原因与重试边界的 `RetrievalHealth`。
/// 【失败】模型文件/manifest 非法、记录无法投影成合法检索条目、请求或上限越界时返回 `PortError::Failed`；
/// 畸形记录让整次检索失败，而不是静默丢弃。
/// 【降级】未配置 manifest（`embedding_not_configured`）、模型文件缺失（`embedding_model_missing`）或查询
/// 词全部不在模型词表（`embedding_query_out_of_vocabulary`）时只走稀疏通道，并把原因写入每个 hit 与
/// health（Degraded + 原因，确定性降级不可重试）；请求要求 dense 但没有任何有效条目向量时采用统一算法
/// 报出的降级原因。
/// 【边界】`scope` 摘要由候选快照标签派生，只用于把 request/item 绑定到同一次快照，不是权限判断；本函数
/// 不读取授权状态、不写 EventLog、不执行工具，也不证明排名质量或外部向量模型的语义正确性。
pub(crate) fn rank_records(
    records: &[MemoryRecord],
    query: &str,
    limit: usize,
    context: RetrievalSnapshotContext,
) -> Result<RankedRecords, PortError> {
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
        .map(|record| {
            Ok(RetrievalItem {
                id: record.id.clone(),
                text: record.text.clone(),
                source_digest: text_digest(&record.text),
                permission_scope_digest: permission_scope_digest.clone(),
                provenance: record_provenance(record, context)?,
                allowed: true,
                created_at_ms: record.created_at_ms,
                dense_vector: model
                    .as_ref()
                    .and_then(|model| model.embed(&tokenize(&record.text))),
            })
        })
        .collect::<Result<Vec<_>, PortError>>()?;
    let request = RetrievalRequest::new(query, permission_scope_digest, query_vector)
        .map_err(|error| fail(format!("memory_retrieval_request_invalid:{error}")))?;
    let port = UnifiedRetrievalPort::new(RetrievalProfile::unified())
        .map_err(|error| fail(format!("memory_retrieval_profile_invalid:{error}")))?;
    let result = port
        .retrieve(&request, &items, limit)
        .map_err(|error| fail(format!("memory_retrieval_failed:{error}")))?;
    let reason = degraded_reason.or_else(|| result.degraded_reasons.first().cloned());
    // health：本模块已知的通道降级优先于统一算法自身的降级原因；两者都是确定性配置事实，因此不可重试。
    let health = match &reason {
        Some(reason) => RetrievalHealth::degraded_channel(
            reason.clone(),
            RetrievalRetryPolicy::Deterministic,
            result.source_generation,
            result.hits.len() as u32,
        )
        .map_err(|error| fail(format!("memory_retrieval_health_invalid:{error}")))?,
        None => result.health.clone(),
    };
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
        // CM-14：命中级来源证据（source ref/revision/generation/freshness/evidence/rank components）。
        output["retrieval_evidence"] = json!(hit.provenance);
        output["embedding_model_id"] = json!(model.as_ref().map(|model| &model.id));
        output["embedding_model_sha256"] = json!(model.as_ref().map(|model| &model.sha256));
        output["degraded"] = json!(reason.is_some());
        output["degraded_reason"] = json!(reason);
        output["retrieval_algorithm"] = json!(result.algorithm_version);
        results.push(output);
    }
    Ok(RankedRecords {
        hits: results,
        health,
    })
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

        /// CM-14：模拟“未配置 embedding manifest”的确定性降级路径。
        fn unset() -> Self {
            let previous = std::env::var_os("KIANA_MEMORY_EMBEDDING_MANIFEST");
            std::env::remove_var("KIANA_MEMORY_EMBEDDING_MANIFEST");
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

    /// 固定的快照标签：非零 data epoch + 足够大的 now，让 fixture 不依赖真实时钟。
    fn snapshot() -> RetrievalSnapshotContext {
        RetrievalSnapshotContext {
            data_epoch: 9,
            now_ms: 1_000,
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
        let first =
            rank_records(&records, "部署策略", 3, snapshot()).expect("first hybrid ranking");
        let second =
            rank_records(&records, "部署策略", 3, snapshot()).expect("second hybrid ranking");
        assert_eq!(first.hits, second.hits);
        assert_eq!(first.health, second.health);
        let first = first.hits;
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
        // CM-14：每个命中都带来源证据，generation 取本次快照的 data epoch。
        for hit in &first {
            let evidence = &hit["retrieval_evidence"];
            assert_eq!(evidence["generation"], 9);
            assert!(evidence["source_ref"]
                .as_str()
                .is_some_and(|value| value.starts_with("memory:project:project:fixture:")));
            assert!(evidence["source_revision"]
                .as_str()
                .is_some_and(|value| value.starts_with("revision:")));
            assert_eq!(evidence["freshness"], "current");
            assert_eq!(evidence["evidence"], "unverifiable");
            assert!(evidence["rank_components"]["rrf"].is_number());
        }
        // 两条命中都来自同一 profile；退化原因在没有 manifest 时由本模块补写，这里是 dense 正常路径。
        assert_eq!(first.len(), 2);
        assert!(first[1]["score_components"]["dense_rank"].is_u64());

        drop(_env);
        fs::remove_dir_all(root).expect("remove fixture directory");
    }

    #[test]
    /// CM-14：embedding 通道失败必须可见（Degraded + 原因）且不悄悄变成可重试成功；同一次调用里
    /// 稀疏命中仍然返回，来源证据保持不变。
    fn embedding_failure_is_visible_and_retryable_only_when_safe() {
        let _lock = ENVIRONMENT_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _env = EnvGuard::unset();
        let records = vec![
            record("alpha", "部署策略需要收据", 3),
            record("beta", "部署记录保留审计证据", 2),
            record("gamma", "只讨论界面颜色", 1),
        ];
        let ranked = rank_records(&records, "部署策略", 3, snapshot()).expect("degraded ranking");
        // 失败可见：状态是 Degraded 且带具体原因，而不是 Ready 空结果或整次调用失败。
        assert_eq!(
            ranked.health.status,
            kiana_domain::RetrievalHealthStatus::Degraded
        );
        assert_eq!(
            ranked.health.reason.as_deref(),
            Some("embedding_not_configured")
        );
        // 确定性配置降级不可重试；`retryable` 与 `retry_safe` 只能同时为真或同时为假。
        assert!(!ranked.health.retryable);
        assert!(!ranked.health.retry_safe);
        // 稀疏命中仍然返回，并继续携带来源证据。
        assert_eq!(ranked.hits.len(), 2);
        for hit in &ranked.hits {
            assert_eq!(hit["degraded"], true);
            assert_eq!(hit["degraded_reason"], "embedding_not_configured");
            assert_eq!(hit["retrieval_evidence"]["generation"], 9);
            let components = hit["retrieval_evidence"]["rank_components"]
                .as_object()
                .expect("rank components object");
            // 没有 dense 通道时，命中证据必须缺少 dense 分量，而不是伪造 0 分。
            assert!(!components.contains_key("dense"));
        }
        // 拒绝路径：Domain 合同不允许 retryable 而没有 retry_safe，也不允许 Denied 可重试。
        assert!(RetrievalHealth::new(
            kiana_domain::RetrievalHealthStatus::Degraded,
            Some("embedding_not_configured".to_owned()),
            true,
            false,
            9,
            2,
        )
        .is_err());
        assert!(RetrievalHealth::new(
            kiana_domain::RetrievalHealthStatus::Denied,
            Some("acl_scope_denied_all_candidates".to_owned()),
            true,
            true,
            9,
            0,
        )
        .is_err());
        // 只有显式声明瞬时故障的调用方才拿到可安全重试的 health。
        let safe = RetrievalHealth::degraded_channel(
            "embedding_channel_transient_unavailable",
            RetrievalRetryPolicy::Safe,
            9,
            2,
        )
        .expect("safe retry health");
        assert!(safe.retryable && safe.retry_safe);
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
        let hits = rank_records(&records, "部署策略", 3, snapshot())
            .expect("memory tool ranking")
            .hits;

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
                // CM-14：两条路径必须用同一函数派生同一种来源证据，才能逐条比对命中证据。
                provenance: record_provenance(record, snapshot()).expect("parity provenance"),
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
            // CM-14：tool 输出的来源证据必须与 port 结果逐字段一致（同一快照标签）。
            assert_eq!(hit["retrieval_evidence"], json!(port_hit.provenance));
            assert_eq!(
                hit["degraded"],
                json!(!expected.degraded_reasons.is_empty())
            );
        }

        drop(_env);
        fs::remove_dir_all(root).expect("remove fixture directory");
    }
}

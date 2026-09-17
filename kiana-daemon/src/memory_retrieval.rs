//! Local deterministic BM25, pinned dense token vectors, RRF and MMR.
//! No network or hash-generated embeddings. ONNX inference is not implemented here.
use kiana_domain::{memory_match_terms, memory_query_terms, MemoryRecord};
use kiana_ports::PortError;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
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
    kiana_domain::memory_tokens(text)
}
fn cosine(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(a, b)| a * b)
        .sum::<f64>()
        .clamp(-1.0, 1.0)
}
fn overlap(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        0.0
    } else {
        a.intersection(b).count() as f64 / union as f64
    }
}

pub(crate) fn rank_records(
    records: &[MemoryRecord],
    query: &str,
    limit: usize,
) -> Result<Vec<Value>, PortError> {
    let (model, mut degraded_reason) = load_model()?;
    let query_tokens = tokenize(query);
    let query_terms = query_tokens.iter().cloned().collect::<BTreeSet<_>>();
    let query_vector = model.as_ref().and_then(|model| model.embed(&query_tokens));
    if model.is_some() && query_vector.is_none() {
        degraded_reason = Some("embedding_query_out_of_vocabulary".to_owned());
    }
    let tokens = records
        .iter()
        .map(|r| tokenize(&r.text))
        .collect::<Vec<_>>();
    let sets = tokens
        .iter()
        .map(|t| t.iter().cloned().collect::<BTreeSet<_>>())
        .collect::<Vec<_>>();
    let vectors = tokens
        .iter()
        .map(|t| model.as_ref().and_then(|m| m.embed(t)))
        .collect::<Vec<_>>();
    let avg = (tokens.iter().map(Vec::len).sum::<usize>() as f64 / (records.len().max(1) as f64))
        .max(1.0);
    let count = records.len() as f64;
    let df = query_terms
        .iter()
        .map(|term| {
            (
                term.clone(),
                sets.iter().filter(|s| s.contains(term)).count() as f64,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut bm25 = vec![0.0; records.len()];
    let mut dense = vec![None; records.len()];
    for (i, terms) in tokens.iter().enumerate() {
        for term in &query_terms {
            let tf = terms.iter().filter(|t| *t == term).count() as f64;
            if tf > 0.0 {
                let frequency = df[term];
                let idf = (1.0 + (count - frequency + 0.5) / (frequency + 0.5)).ln();
                bm25[i] += idf * tf * 2.2 / (tf + 1.2 * (0.25 + 0.75 * terms.len() as f64 / avg));
            }
        }
        dense[i] = query_vector
            .as_ref()
            .zip(vectors[i].as_ref())
            .map(|(q, v)| cosine(q, v))
            .filter(|score| *score > 0.0);
    }
    let mut sparse_order = (0..records.len())
        .filter(|i| bm25[*i] > 0.0)
        .collect::<Vec<_>>();
    sparse_order.sort_by(|a, b| bm25[*b].total_cmp(&bm25[*a]).then(a.cmp(b)));
    let mut dense_order = (0..records.len())
        .filter(|i| dense[*i].is_some())
        .collect::<Vec<_>>();
    dense_order.sort_by(|a, b| {
        dense[*b]
            .unwrap_or(0.0)
            .total_cmp(&dense[*a].unwrap_or(0.0))
            .then(a.cmp(b))
    });
    let mut sparse_rank = vec![None; records.len()];
    let mut dense_rank = vec![None; records.len()];
    let mut rrf = vec![0.0; records.len()];
    for (rank, i) in sparse_order.into_iter().enumerate() {
        sparse_rank[i] = Some(rank + 1);
        rrf[i] += 1.0 / (61.0 + rank as f64);
    }
    for (rank, i) in dense_order.into_iter().enumerate() {
        dense_rank[i] = Some(rank + 1);
        rrf[i] += 1.0 / (61.0 + rank as f64);
    }
    let max_rrf = rrf.iter().copied().fold(0.0, f64::max).max(f64::EPSILON);
    let mut remaining = (0..records.len())
        .filter(|i| rrf[*i] > 0.0)
        .collect::<BTreeSet<_>>();
    let mut selected: Vec<usize> = Vec::new();
    let mut results = Vec::new();
    while selected.len() < limit && !remaining.is_empty() {
        let mut ranked = remaining
            .iter()
            .map(|i| {
                let redundancy = selected
                    .iter()
                    .map(|j| match (&vectors[*i], &vectors[*j]) {
                        (Some(a), Some(b)) => cosine(a, b).max(0.0),
                        _ => overlap(&sets[*i], &sets[*j]),
                    })
                    .fold(0.0, f64::max);
                (*i, 0.7 * rrf[*i] / max_rrf - 0.3 * redundancy)
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|(a, sa), (b, sb)| {
            sb.total_cmp(sa)
                .then_with(|| records[*b].created_at_ms.cmp(&records[*a].created_at_ms))
                .then_with(|| {
                    (&records[*a].collection, &records[*a].id)
                        .cmp(&(&records[*b].collection, &records[*b].id))
                })
        });
        let (i, mmr) = ranked[0];
        remaining.remove(&i);
        selected.push(i);
        let mut hit = records[i].hit();
        let matched = memory_match_terms(&records[i].text, &memory_query_terms(query));
        hit["score"] = json!(matched.len());
        hit["matched_terms"] = json!(matched);
        hit["score_components"] = json!({"bm25":bm25[i],"bm25_rank":sparse_rank[i],"dense":dense[i],"dense_rank":dense_rank[i],"rrf":rrf[i],"mmr":mmr});
        hit["embedding_model_id"] = json!(model.as_ref().map(|m| &m.id));
        hit["embedding_model_sha256"] = json!(model.as_ref().map(|m| &m.sha256));
        hit["degraded"] = json!(degraded_reason.is_some());
        hit["degraded_reason"] = json!(degraded_reason);
        hit["retrieval_algorithm"] = json!("bm25-cjk+cosine+rrf60+mmr0.7.v1");
        results.push(hit);
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
        assert_eq!(
            first[0]["retrieval_algorithm"],
            "bm25-cjk+cosine+rrf60+mmr0.7.v1"
        );

        drop(_env);
        fs::remove_dir_all(root).expect("remove fixture directory");
    }
}

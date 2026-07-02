use crate::repo_map::{collect_paths, language_for_path, IgnoreRules};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_LIMIT: usize = 10;
const DEFAULT_MAX_BYTES_PER_FILE: usize = 128 * 1024;
const DEFAULT_MAX_SNIPPET_LINES: usize = 5;

#[derive(Debug, Clone, Copy)]
pub struct ContextIndexOptions {
    pub max_bytes_per_file: Option<usize>,
}

impl Default for ContextIndexOptions {
    fn default() -> Self {
        Self {
            max_bytes_per_file: Some(DEFAULT_MAX_BYTES_PER_FILE),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ContextSearchOptions {
    pub limit: Option<usize>,
    pub max_bytes_per_file: Option<usize>,
}

impl Default for ContextSearchOptions {
    fn default() -> Self {
        Self {
            limit: Some(DEFAULT_LIMIT),
            max_bytes_per_file: Some(DEFAULT_MAX_BYTES_PER_FILE),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ContextPackOptions {
    pub limit: Option<usize>,
    pub max_bytes_per_file: Option<usize>,
    pub max_snippet_lines: Option<usize>,
}

impl Default for ContextPackOptions {
    fn default() -> Self {
        Self {
            limit: Some(DEFAULT_LIMIT),
            max_bytes_per_file: Some(DEFAULT_MAX_BYTES_PER_FILE),
            max_snippet_lines: Some(DEFAULT_MAX_SNIPPET_LINES),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextIndex {
    pub schema: String,
    pub root: String,
    pub files_indexed: usize,
    pub skipped_files: usize,
    pub total_bytes: u64,
    pub files: Vec<ContextIndexedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextIndexedFile {
    pub path: String,
    pub language: Option<String>,
    pub bytes: u64,
    pub line_count: usize,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSearchResults {
    pub schema: String,
    pub root: String,
    pub query: String,
    pub terms: Vec<String>,
    pub limit: usize,
    pub files_indexed: usize,
    pub skipped_files: usize,
    pub hits: Vec<ContextSearchHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSearchHit {
    pub path: String,
    pub language: Option<String>,
    pub score: u64,
    pub occurrences: u64,
    pub matched_terms: Vec<String>,
    pub line_number: usize,
    pub line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPack {
    pub schema: String,
    pub root: String,
    pub query: String,
    pub terms: Vec<String>,
    pub limit: usize,
    pub max_snippet_lines: usize,
    pub files_indexed: usize,
    pub skipped_files: usize,
    pub snippets: Vec<ContextPackSnippet>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPackSnippet {
    pub path: String,
    pub language: Option<String>,
    pub content_hash: String,
    pub score: u64,
    pub occurrences: u64,
    pub matched_terms: Vec<String>,
    pub start_line: usize,
    pub end_line: usize,
    pub excerpt: String,
}

pub fn build_context_index(
    root: impl AsRef<Path>,
    options: ContextIndexOptions,
) -> Result<ContextIndex> {
    let root = canonical_root(root.as_ref(), "context index")?;
    let max_bytes = options
        .max_bytes_per_file
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_BYTES_PER_FILE);
    let mut files = Vec::new();
    let mut skipped_files = 0;
    let mut total_bytes = 0;

    for path in candidate_paths(&root)? {
        let Some(indexed) = index_file(&root, &path, max_bytes)? else {
            skipped_files += 1;
            continue;
        };
        total_bytes += indexed.bytes;
        files.push(indexed);
    }

    Ok(ContextIndex {
        schema: "kiana.context-index.v1".to_string(),
        root: root.to_string_lossy().to_string(),
        files_indexed: files.len(),
        skipped_files,
        total_bytes,
        files,
    })
}

pub fn search_context_index(
    root: impl AsRef<Path>,
    query: &str,
    options: ContextSearchOptions,
) -> Result<ContextSearchResults> {
    let root = canonical_root(root.as_ref(), "context search")?;
    let terms = query_terms(query);
    if terms.is_empty() {
        return Err(anyhow!(
            "context search query must contain at least one term"
        ));
    }
    let limit = options
        .limit
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_LIMIT);
    let max_bytes = options
        .max_bytes_per_file
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_BYTES_PER_FILE);
    let mut hits = Vec::new();
    let mut files_indexed = 0;
    let mut skipped_files = 0;

    for path in candidate_paths(&root)? {
        let Some(hit) = search_file(&root, &path, &terms, max_bytes)? else {
            skipped_files += 1;
            continue;
        };
        files_indexed += 1;
        if hit.occurrences > 0 {
            hits.push(hit);
        }
    }

    hits.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then(left.path.cmp(&right.path))
            .then(left.line_number.cmp(&right.line_number))
    });
    hits.truncate(limit);

    Ok(ContextSearchResults {
        schema: "kiana.context-search.v1".to_string(),
        root: root.to_string_lossy().to_string(),
        query: query.trim().to_string(),
        terms,
        limit,
        files_indexed,
        skipped_files,
        hits,
    })
}

pub fn build_context_pack(
    root: impl AsRef<Path>,
    query: &str,
    options: ContextPackOptions,
) -> Result<ContextPack> {
    let root = canonical_root(root.as_ref(), "context pack")?;
    let terms = query_terms(query);
    if terms.is_empty() {
        return Err(anyhow!("context pack query must contain at least one term"));
    }
    let limit = options
        .limit
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_LIMIT);
    let max_bytes = options
        .max_bytes_per_file
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_BYTES_PER_FILE);
    let max_snippet_lines = options
        .max_snippet_lines
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_SNIPPET_LINES);
    let mut snippets = Vec::new();
    let mut files_indexed = 0;
    let mut skipped_files = 0;

    for path in candidate_paths(&root)? {
        let Some(snippet) = pack_file(&root, &path, &terms, max_bytes, max_snippet_lines)? else {
            skipped_files += 1;
            continue;
        };
        files_indexed += 1;
        if snippet.occurrences > 0 {
            snippets.push(snippet);
        }
    }

    snippets.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then(left.path.cmp(&right.path))
            .then(left.start_line.cmp(&right.start_line))
    });
    snippets.truncate(limit);

    Ok(ContextPack {
        schema: "kiana.context-pack.v1".to_string(),
        root: root.to_string_lossy().to_string(),
        query: query.trim().to_string(),
        terms,
        limit,
        max_snippet_lines,
        files_indexed,
        skipped_files,
        snippets,
    })
}

fn canonical_root(root: &Path, label: &str) -> Result<PathBuf> {
    fs::canonicalize(root)
        .with_context(|| format!("failed to resolve {label} root {}", root.display()))
}

fn candidate_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let ignore_rules = IgnoreRules::load(root);
    let mut paths = Vec::new();
    collect_paths(root, root, &ignore_rules, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn index_file(root: &Path, path: &Path, max_bytes: usize) -> Result<Option<ContextIndexedFile>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.len() > max_bytes || bytes.contains(&0) {
        return Ok(None);
    }
    let Ok(content) = String::from_utf8(bytes) else {
        return Ok(None);
    };
    let rel = relative_path(root, path);
    Ok(Some(ContextIndexedFile {
        path: rel,
        language: language_for_path(path).map(str::to_string),
        bytes: content.len() as u64,
        line_count: content.lines().count(),
        content_hash: stable_hash(content.as_bytes()),
    }))
}

fn search_file(
    root: &Path,
    path: &Path,
    terms: &[String],
    max_bytes: usize,
) -> Result<Option<ContextSearchHit>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.len() > max_bytes || bytes.contains(&0) {
        return Ok(None);
    }
    let Ok(content) = String::from_utf8(bytes) else {
        return Ok(None);
    };
    let rel = relative_path(root, path);
    let mut occurrences_by_term = BTreeMap::new();
    for token in tokenize(&content) {
        if terms.binary_search(&token).is_ok() {
            *occurrences_by_term.entry(token).or_insert(0_u64) += 1;
        }
    }
    let occurrences = occurrences_by_term.values().sum::<u64>();
    let matched_terms = occurrences_by_term.keys().cloned().collect::<Vec<_>>();
    let (line_number, line) = first_matching_line(&content, terms);
    let path_score = tokenize(&rel)
        .into_iter()
        .filter(|token| terms.binary_search(token).is_ok())
        .count() as u64;
    let score = occurrences * 10 + matched_terms.len() as u64 * 5 + path_score * 3;

    Ok(Some(ContextSearchHit {
        path: rel,
        language: language_for_path(path).map(str::to_string),
        score,
        occurrences,
        matched_terms,
        line_number,
        line,
    }))
}

fn pack_file(
    root: &Path,
    path: &Path,
    terms: &[String],
    max_bytes: usize,
    max_snippet_lines: usize,
) -> Result<Option<ContextPackSnippet>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.len() > max_bytes || bytes.contains(&0) {
        return Ok(None);
    }
    let Ok(content) = String::from_utf8(bytes) else {
        return Ok(None);
    };
    let rel = relative_path(root, path);
    let mut occurrences_by_term = BTreeMap::new();
    for token in tokenize(&content) {
        if terms.binary_search(&token).is_ok() {
            *occurrences_by_term.entry(token).or_insert(0_u64) += 1;
        }
    }
    let occurrences = occurrences_by_term.values().sum::<u64>();
    let matched_terms = occurrences_by_term.keys().cloned().collect::<Vec<_>>();
    let (line_number, _) = first_matching_line(&content, terms);
    let path_score = tokenize(&rel)
        .into_iter()
        .filter(|token| terms.binary_search(token).is_ok())
        .count() as u64;
    let score = occurrences * 10 + matched_terms.len() as u64 * 5 + path_score * 3;
    let (start_line, end_line, excerpt) = snippet_excerpt(&content, line_number, max_snippet_lines);

    Ok(Some(ContextPackSnippet {
        path: rel,
        language: language_for_path(path).map(str::to_string),
        content_hash: stable_hash(content.as_bytes()),
        score,
        occurrences,
        matched_terms,
        start_line,
        end_line,
        excerpt,
    }))
}

fn query_terms(query: &str) -> Vec<String> {
    tokenize(query)
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn first_matching_line(content: &str, terms: &[String]) -> (usize, String) {
    for (index, line) in content.lines().enumerate() {
        let line_tokens = tokenize(line);
        if line_tokens
            .iter()
            .any(|token| terms.binary_search(token).is_ok())
        {
            return (index + 1, line.trim().chars().take(240).collect());
        }
    }
    (0, String::new())
}

fn snippet_excerpt(
    content: &str,
    line_number: usize,
    max_snippet_lines: usize,
) -> (usize, usize, String) {
    if line_number == 0 {
        return (0, 0, String::new());
    }
    let lines = content.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return (0, 0, String::new());
    }
    let max_lines = max_snippet_lines.max(1);
    let match_index = line_number.saturating_sub(1).min(lines.len() - 1);
    let mut start = match_index.saturating_sub(max_lines / 2);
    let mut end = (start + max_lines).min(lines.len());
    if end.saturating_sub(start) < max_lines {
        start = end.saturating_sub(max_lines);
    }
    end = (start + max_lines).min(lines.len());
    let excerpt = lines[start..end]
        .iter()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    (start + 1, end, excerpt)
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn stable_hash(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-context-index-{name}-{}-{unique}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn context_index_reports_stable_file_metadata() {
        let root = fixture_root("metadata");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn search() {}\n").unwrap();
        fs::write(root.join("notes.md"), "# Search Notes\n").unwrap();

        let index = build_context_index(&root, ContextIndexOptions::default()).unwrap();
        let paths = index
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(index.schema, "kiana.context-index.v1");
        assert_eq!(paths, vec!["notes.md", "src/lib.rs"]);
        assert_eq!(index.files[1].language.as_deref(), Some("rust"));
        assert_eq!(index.skipped_files, 0);
        assert_eq!(index.files[1].content_hash.len(), 16);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_search_returns_ranked_hits_and_line_excerpt() {
        let root = fixture_root("search");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn checkout() {}\n// checkout checkout flow\n",
        )
        .unwrap();
        fs::write(root.join("README.md"), "checkout guide\n").unwrap();

        let results = search_context_index(
            &root,
            "checkout",
            ContextSearchOptions {
                limit: Some(5),
                max_bytes_per_file: None,
            },
        )
        .unwrap();

        assert_eq!(results.schema, "kiana.context-search.v1");
        assert_eq!(results.terms, vec!["checkout"]);
        assert_eq!(results.hits[0].path, "src/lib.rs");
        assert_eq!(results.hits[0].occurrences, 3);
        assert_eq!(results.hits[0].line_number, 1);
        assert!(results.hits[0]
            .matched_terms
            .contains(&"checkout".to_string()));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_pack_builds_ranked_snippets() {
        let root = fixture_root("pack");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn checkout() {}\n// prepare checkout flow\n// checkout checkout done\n",
        )
        .unwrap();
        fs::write(root.join("README.md"), "checkout guide\n").unwrap();

        let pack = build_context_pack(
            &root,
            "checkout",
            ContextPackOptions {
                limit: Some(5),
                max_bytes_per_file: None,
                max_snippet_lines: Some(2),
            },
        )
        .unwrap();

        assert_eq!(pack.schema, "kiana.context-pack.v1");
        assert_eq!(pack.terms, vec!["checkout"]);
        assert_eq!(pack.max_snippet_lines, 2);
        assert_eq!(pack.snippets[0].path, "src/lib.rs");
        assert_eq!(pack.snippets[0].language.as_deref(), Some("rust"));
        assert_eq!(pack.snippets[0].occurrences, 4);
        assert_eq!(pack.snippets[0].start_line, 1);
        assert_eq!(pack.snippets[0].end_line, 2);
        assert!(pack.snippets[0].excerpt.contains("prepare checkout flow"));
        assert_eq!(pack.snippets[0].content_hash.len(), 16);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_pack_is_deterministic_and_budgeted() {
        let root = fixture_root("pack-budget");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn lifecycle() {}\n// lifecycle lifecycle\n",
        )
        .unwrap();
        fs::write(root.join("README.md"), "lifecycle guide\n").unwrap();

        let options = ContextPackOptions {
            limit: Some(1),
            max_bytes_per_file: None,
            max_snippet_lines: Some(1),
        };
        let first = build_context_pack(&root, "lifecycle", options).unwrap();
        let second = build_context_pack(&root, "lifecycle", options).unwrap();

        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(&second).unwrap()
        );
        assert_eq!(first.snippets.len(), 1);
        assert_eq!(first.snippets[0].path, "src/lib.rs");
        assert_eq!(first.snippets[0].start_line, first.snippets[0].end_line);
        assert_eq!(first.limit, 1);

        let _ = fs::remove_dir_all(root);
    }
}

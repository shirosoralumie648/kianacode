use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const DEFAULT_TOKEN_BUDGET: u64 = 4_000;
const MAX_SYMBOLS_PER_FILE: usize = 12;

#[derive(Debug, Clone, Copy)]
pub struct RepoMapOptions {
    pub max_tokens: Option<u64>,
}

impl Default for RepoMapOptions {
    fn default() -> Self {
        Self {
            max_tokens: Some(DEFAULT_TOKEN_BUDGET),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoMap {
    pub root: String,
    pub token_budget: u64,
    pub estimated_tokens: u64,
    pub truncated: bool,
    pub omitted_files: usize,
    pub files: Vec<RepoMapFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoMapFile {
    pub path: String,
    pub language: Option<String>,
    pub bytes: u64,
    pub estimated_tokens: u64,
    pub symbols: Vec<String>,
}

pub fn build_repo_map(root: impl AsRef<Path>, options: RepoMapOptions) -> Result<RepoMap> {
    let root = root.as_ref();
    let root = fs::canonicalize(root)
        .with_context(|| format!("failed to resolve repo map root {}", root.display()))?;
    let token_budget = options
        .max_tokens
        .filter(|budget| *budget > 0)
        .unwrap_or(DEFAULT_TOKEN_BUDGET);
    let ignore_rules = IgnoreRules::load(&root);
    let mut paths = Vec::new();
    collect_paths(&root, &root, &ignore_rules, &mut paths)?;
    paths.sort();

    let mut files = Vec::new();
    let mut estimated_tokens = 8;
    let mut omitted_files = 0;
    for path in paths {
        let Some(entry) = map_file(&root, &path)? else {
            continue;
        };
        if estimated_tokens + entry.estimated_tokens > token_budget {
            omitted_files += 1;
            continue;
        }
        estimated_tokens += entry.estimated_tokens;
        files.push(entry);
    }

    Ok(RepoMap {
        root: root.to_string_lossy().to_string(),
        token_budget,
        estimated_tokens,
        truncated: omitted_files > 0,
        omitted_files,
        files,
    })
}

fn collect_paths(
    root: &Path,
    dir: &Path,
    ignore_rules: &IgnoreRules,
    paths: &mut Vec<PathBuf>,
) -> Result<()> {
    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("failed to read repo map directory {}", dir.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("failed to read repo map directory entry {}", dir.display()))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to read file type {}", path.display()))?;
        let rel = path.strip_prefix(root).unwrap_or(path.as_path());
        if ignore_rules.ignores(rel, file_type.is_dir()) {
            continue;
        }
        if file_type.is_dir() {
            collect_paths(root, &path, ignore_rules, paths)?;
        } else if file_type.is_file() && language_for_path(&path).is_some() {
            paths.push(path);
        }
    }

    Ok(())
}

fn map_file(root: &Path, path: &Path) -> Result<Option<RepoMapFile>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.contains(&0) {
        return Ok(None);
    }
    let Ok(content) = String::from_utf8(bytes) else {
        return Ok(None);
    };
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let language = language_for_path(path).map(str::to_string);
    let symbols = language
        .as_deref()
        .map(|language| extract_symbols(language, &content))
        .unwrap_or_default();
    let estimated_tokens = estimate_entry_tokens(&rel, language.as_deref(), &symbols);

    Ok(Some(RepoMapFile {
        path: rel,
        language,
        bytes: content.len() as u64,
        estimated_tokens,
        symbols,
    }))
}

fn estimate_entry_tokens(path: &str, language: Option<&str>, symbols: &[String]) -> u64 {
    let symbol_chars = symbols.iter().map(String::len).sum::<usize>();
    let language_chars = language.map(str::len).unwrap_or(0);
    (((path.len() + language_chars + symbol_chars) as u64) / 4 + 6).max(4)
}

fn language_for_path(path: &Path) -> Option<&'static str> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("rs") => Some("rust"),
        Some("py") => Some("python"),
        Some("ts") | Some("tsx") => Some("typescript"),
        Some("js") | Some("jsx") => Some("javascript"),
        Some("go") => Some("go"),
        Some("java") => Some("java"),
        Some("kt") => Some("kotlin"),
        Some("swift") => Some("swift"),
        Some("c") | Some("h") => Some("c"),
        Some("cc") | Some("cpp") | Some("hpp") => Some("cpp"),
        Some("cs") => Some("csharp"),
        Some("rb") => Some("ruby"),
        Some("php") => Some("php"),
        Some("sh") | Some("bash") | Some("zsh") => Some("shell"),
        Some("ps1") => Some("powershell"),
        Some("md") => Some("markdown"),
        Some("toml") => Some("toml"),
        Some("yaml") | Some("yml") => Some("yaml"),
        Some("json") => Some("json"),
        Some("html") | Some("htm") => Some("html"),
        Some("css") | Some("scss") => Some("css"),
        Some("sql") => Some("sql"),
        Some("xml") => Some("xml"),
        _ => None,
    }
}

fn extract_symbols(language: &str, content: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    for line in content.lines() {
        if symbols.len() >= MAX_SYMBOLS_PER_FILE {
            break;
        }
        let trimmed = line.trim();
        let symbol = match language {
            "rust" => rust_symbol(trimmed),
            "python" => python_symbol(trimmed),
            "typescript" | "javascript" => js_symbol(trimmed),
            _ => generic_symbol(trimmed),
        };
        if let Some(symbol) = symbol {
            symbols.push(symbol);
        }
    }
    symbols
}

fn rust_symbol(line: &str) -> Option<String> {
    let line = strip_prefixes(line, &["pub(crate) ", "pub ", "async "]);
    for keyword in ["fn", "struct", "enum", "trait", "impl"] {
        if let Some(name) = symbol_name_after(line, keyword) {
            return Some(format!("{keyword} {name}"));
        }
    }
    None
}

fn python_symbol(line: &str) -> Option<String> {
    for keyword in ["class", "def"] {
        if let Some(name) = symbol_name_after(line, keyword) {
            return Some(format!("{keyword} {name}"));
        }
    }
    None
}

fn js_symbol(line: &str) -> Option<String> {
    let line = strip_prefixes(line, &["export default ", "export "]);
    for keyword in ["function", "class", "interface", "type", "const", "let"] {
        if let Some(name) = symbol_name_after(line, keyword) {
            return Some(format!("{keyword} {name}"));
        }
    }
    None
}

fn generic_symbol(line: &str) -> Option<String> {
    line.strip_prefix("# ")
        .or_else(|| line.strip_prefix("## "))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| format!("heading {name}"))
}

fn strip_prefixes<'a>(mut line: &'a str, prefixes: &[&str]) -> &'a str {
    loop {
        let mut stripped = false;
        for prefix in prefixes {
            if let Some(rest) = line.strip_prefix(prefix) {
                line = rest.trim_start();
                stripped = true;
            }
        }
        if !stripped {
            return line;
        }
    }
}

fn symbol_name_after(line: &str, keyword: &str) -> Option<String> {
    let rest = line.strip_prefix(keyword)?.trim_start();
    if rest.is_empty() {
        return None;
    }
    let name = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect::<String>();
    (!name.is_empty()).then_some(name)
}

#[derive(Debug, Default)]
struct IgnoreRules {
    names: HashSet<String>,
    dir_names: HashSet<String>,
    path_patterns: Vec<String>,
}

impl IgnoreRules {
    fn load(root: &Path) -> Self {
        let mut rules = Self {
            names: common_ignored_names(),
            dir_names: common_ignored_dirs(),
            path_patterns: Vec::new(),
        };
        let Ok(contents) = fs::read_to_string(root.join(".gitignore")) else {
            return rules;
        };
        for line in contents.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                continue;
            }
            let line = line.trim_start_matches('/');
            if line.ends_with('/') {
                rules
                    .dir_names
                    .insert(line.trim_end_matches('/').to_string());
            } else if line.contains('*') || line.contains('/') {
                rules.path_patterns.push(line.to_string());
            } else {
                rules.names.insert(line.to_string());
            }
        }
        rules
    }

    fn ignores(&self, rel: &Path, is_dir: bool) -> bool {
        let rel_text = rel.to_string_lossy().replace('\\', "/");
        let file_name = rel.file_name().and_then(|name| name.to_str()).unwrap_or("");
        if file_name.starts_with('.') {
            return true;
        }
        if self.names.contains(file_name) {
            return true;
        }
        if is_dir && self.dir_names.contains(file_name) {
            return true;
        }
        if rel
            .components()
            .any(|component| ignored_component(component, self))
        {
            return true;
        }
        self.path_patterns.iter().any(|pattern| {
            wildcard_matches(pattern, &rel_text) || wildcard_matches(pattern, file_name)
        })
    }
}

fn ignored_component(component: Component<'_>, rules: &IgnoreRules) -> bool {
    let Component::Normal(value) = component else {
        return false;
    };
    let Some(value) = value.to_str() else {
        return false;
    };
    rules.dir_names.contains(value) || value == ".git"
}

fn common_ignored_names() -> HashSet<String> {
    [".DS_Store", "Cargo.lock"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn common_ignored_dirs() -> HashSet<String> {
    [
        ".git",
        "target",
        "node_modules",
        ".venv",
        "venv",
        "__pycache__",
        "dist",
        "build",
        ".next",
        "coverage",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn wildcard_matches(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return pattern == text;
    }

    let mut remaining = text;
    if let Some(first) = parts.first().filter(|part| !part.is_empty()) {
        let Some(rest) = remaining.strip_prefix(first) else {
            return false;
        };
        remaining = rest;
    }

    for part in parts.iter().skip(1).take(parts.len().saturating_sub(2)) {
        if part.is_empty() {
            continue;
        }
        let Some(index) = remaining.find(part) else {
            return false;
        };
        remaining = &remaining[index + part.len()..];
    }

    if let Some(last) = parts.last().filter(|part| !part.is_empty()) {
        remaining.ends_with(last)
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-repo-map-{name}-{}-{unique}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn repo_map_uses_stable_order_ignore_rules_and_language_symbols() {
        let root = fixture_root("symbols");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("web")).unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".gitignore"), "ignored.tmp\n").unwrap();
        fs::write(root.join("ignored.tmp"), "temporary").unwrap();
        fs::write(root.join(".git/config"), "[core]\n").unwrap();
        fs::write(root.join("target/generated.rs"), "fn generated() {}\n").unwrap();
        fs::write(
            root.join("node_modules/pkg/index.js"),
            "function vendor() {}\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub struct Alpha;\nfn beta() {}\n").unwrap();
        fs::write(
            root.join("src/main.py"),
            "class Worker:\n    def run(self):\n        pass\n",
        )
        .unwrap();
        fs::write(root.join("web/app.ts"), "export function render() {}\n").unwrap();

        let map = build_repo_map(
            &root,
            RepoMapOptions {
                max_tokens: Some(10_000),
            },
        )
        .unwrap();

        let paths = map
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(paths, vec!["src/lib.rs", "src/main.py", "web/app.ts"]);
        assert_eq!(map.files[0].language.as_deref(), Some("rust"));
        assert!(map.files[0]
            .symbols
            .iter()
            .any(|symbol| symbol == "struct Alpha"));
        assert!(map.files[0]
            .symbols
            .iter()
            .any(|symbol| symbol == "fn beta"));
        assert_eq!(map.files[1].language.as_deref(), Some("python"));
        assert!(map.files[1]
            .symbols
            .iter()
            .any(|symbol| symbol == "class Worker"));
        assert!(map.files[1]
            .symbols
            .iter()
            .any(|symbol| symbol == "def run"));
        assert_eq!(map.files[2].language.as_deref(), Some("typescript"));
        assert!(map.files[2]
            .symbols
            .iter()
            .any(|symbol| symbol == "function render"));
        assert!(!map.truncated);
        assert_eq!(map.omitted_files, 0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repo_map_respects_token_budget_and_reports_omitted_files() {
        let root = fixture_root("budget");
        fs::write(root.join("a.rs"), "pub fn alpha() {}\n".repeat(20)).unwrap();
        fs::write(root.join("b.rs"), "pub fn beta() {}\n".repeat(20)).unwrap();
        fs::write(root.join("c.rs"), "pub fn gamma() {}\n".repeat(20)).unwrap();

        let map = build_repo_map(
            &root,
            RepoMapOptions {
                max_tokens: Some(40),
            },
        )
        .unwrap();

        assert!(map.truncated);
        assert!(map.omitted_files > 0);
        assert!(map.estimated_tokens <= 40);
        assert!(!map.files.is_empty());

        let _ = fs::remove_dir_all(root);
    }
}

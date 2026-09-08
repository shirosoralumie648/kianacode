//! 面向模型上下文的轻量仓库结构摘要。
//!
//! Repo map 只读取本地工作区中可识别的文本文件，按稳定路径顺序提取语言、字节数和少量
//! 顶层符号，再在 token 预算内截断结果。它不是完整语法树、构建系统或权限扫描器：忽略
//! 规则、二进制过滤和符号提取都采用保守的启发式。返回的 map 只能帮助查询上下文组织，
//! 不能替代 `ControlPlane` 的 trust、path lock 或 sandbox 校验。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const DEFAULT_TOKEN_BUDGET: u64 = 4_000;
const MAX_SYMBOLS_PER_FILE: usize = 12;

#[derive(Debug, Clone, Copy)]
/// 构建 repo map 时采用的大小限制。
pub struct RepoMapOptions {
    /// 输出允许使用的估算 token 上限；`None` 或 0 使用默认值 4000。
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
/// 一个仓库的结构摘要及其截断状态。
pub struct RepoMap {
    /// 已规范化的仓库根路径。
    pub root: String,
    /// 本次采用的 token 预算。
    pub token_budget: u64,
    /// 已纳入文件条目的估算 token 总数，不是模型供应商的精确计费数。
    pub estimated_tokens: u64,
    /// 是否因预算不足而省略了文件。
    pub truncated: bool,
    /// 被预算省略的文件数量。
    pub omitted_files: usize,
    /// 按相对路径排序的文件摘要。
    pub files: Vec<RepoMapFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Repo map 中的单文件摘要。
pub struct RepoMapFile {
    /// 相对于 map 根目录的正斜杠路径。
    pub path: String,
    /// 根据扩展名推断的语言；无法识别时为 `None`。
    pub language: Option<String>,
    /// UTF-8 文本的字节长度。
    pub bytes: u64,
    /// 路径、语言和符号名称的粗略 token 估算。
    pub estimated_tokens: u64,
    /// 最多保留 `MAX_SYMBOLS_PER_FILE`（当前为 12）个顶层符号的启发式名称。
    pub symbols: Vec<String>,
}

/// 扫描仓库并构建稳定、受 token 预算限制的结构摘要。
///
/// 根目录会先 canonicalize；读取失败会返回带路径上下文的错误。路径先完整收集并排序，
/// 再按预算顺序纳入文件，因此同一工作区和预算可以得到稳定输出。二进制、非 UTF-8、被
/// 默认忽略或 `.gitignore` 忽略的条目不会进入结果。
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
        // 预算以条目估算值计算；超限文件被计数但不再尝试拆分其符号，避免输出顺序漂移。
        if estimated_tokens + entry.estimated_tokens > token_budget {
            omitted_files += 1;
            continue;
        }
        estimated_tokens += entry.estimated_tokens;
        files.push(entry);
    }

    Ok(RepoMap {
        root: display_path(&root),
        token_budget,
        estimated_tokens,
        truncated: omitted_files > 0,
        omitted_files,
        files,
    })
}

/// 递归收集满足忽略规则且扩展名可识别的文件路径。
///
/// 每层目录先按文件名排序，调用者在顶层再次排序以抵御不同文件系统的 `read_dir` 顺序。
/// `root` 只用于计算相对路径和匹配 `.gitignore`，函数本身不读取文件内容。
pub(crate) fn collect_paths(
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
        // 目录在这里被整体跳过，避免进入 target、node_modules 等高噪声或生成目录。
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

/// 将一个候选文本文件转换为 [`RepoMapFile`]；二进制和非法 UTF-8 返回 `None`。
///
/// 当前符号提取只读取内容的行文本，并且每文件有固定上限，因此不会因为超大源码文件
/// 生成无限大的上下文。路径显示保持相对且使用 `/`，便于跨平台比较和模型阅读。
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

fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        value.into_owned()
    }
}

/// 估算一个文件条目的上下文 token 占用。
///
/// 这是按字符数除以 4 的粗略预算，不承诺与任何模型 tokenizer 一致；它只用于保证输出
/// 有确定上限，不能作为计费、配额或安全判断。
fn estimate_entry_tokens(path: &str, language: Option<&str>, symbols: &[String]) -> u64 {
    let symbol_chars = symbols.iter().map(String::len).sum::<usize>();
    let language_chars = language.map(str::len).unwrap_or(0);
    (((path.len() + language_chars + symbol_chars) as u64) / 4 + 6).max(4)
}

/// 根据文件扩展名返回内部语言标签。
///
/// 映射是白名单，未知扩展名返回 `None` 并被扫描器跳过；它不读取 shebang，也不判断
/// 文件真实语法，所以标签只能用于摘要展示和符号提取分派。
pub(crate) fn language_for_path(path: &Path) -> Option<&'static str> {
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

/// 按语言选择轻量符号提取器，并限制每个文件的结果数量。
///
/// 该函数故意不引入完整 parser：repo map 需要快速、容错地生成上下文。遇到无法识别的
/// 行会跳过，提取出的名称不应被当作编译器或语义索引的权威结果。
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
/// 组合默认目录、`.gitignore` 名称和简单通配符的忽略规则。
pub(crate) struct IgnoreRules {
    /// 只按文件名匹配的忽略名称。
    names: HashSet<String>,
    /// 只对目录生效的忽略名称。
    dir_names: HashSet<String>,
    /// 含 `*` 或 `/` 的相对路径模式。
    path_patterns: Vec<String>,
}

impl IgnoreRules {
    /// 加载默认规则并追加仓库根目录的 `.gitignore`。
    ///
    /// 当前实现跳过否定规则（以 `!` 开头）和复杂 gitignore 语义；这是一项明确的摘要
    /// 简化，不应被描述成与 Git 完全等价。配置文件损坏或不存在时保留默认规则。
    pub(crate) fn load(root: &Path) -> Self {
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

    /// 判断相对路径是否应该跳过。
    ///
    /// 隐藏文件、默认名称、默认目录、路径任一组件命中规则或通配符匹配都会被忽略。该
    /// 顺序先做廉价的文件名判断，再做模式扫描，结果只影响上下文摘要，不改变任何文件。
    pub(crate) fn ignores(&self, rel: &Path, is_dir: bool) -> bool {
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

/// 判断单个路径组件是否属于默认忽略目录。
fn ignored_component(component: Component<'_>, rules: &IgnoreRules) -> bool {
    let Component::Normal(value) = component else {
        return false;
    };
    let Some(value) = value.to_str() else {
        return false;
    };
    rules.dir_names.contains(value) || value == ".git"
}

/// 返回仓库摘要默认忽略的常见元文件名。
fn common_ignored_names() -> HashSet<String> {
    [".DS_Store", "Cargo.lock"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

/// 返回仓库摘要默认忽略的生成、依赖和缓存目录名。
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

/// 实现仅支持 `*` 的简单通配符匹配。
///
/// 星号可以匹配任意长度文本；函数不实现 `?`、字符类、转义或 Git 的目录锚定语义，
/// 因此只应在本模块的摘要过滤中使用。模式与文本均已由调用方规范化为正斜杠路径。
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

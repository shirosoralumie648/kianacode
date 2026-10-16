//! 面向模型上下文的轻量仓库结构摘要。
//!
//! 本文件负责把本地工作区压缩成轻量仓库结构摘要（RepoMap），供上下文查询展示或检索适配器
//! 使用。它按可识别的文件扩展名寻找 UTF-8 文本文件，提取路径、语言、字节数、内容摘要和
//! 少量符合行前缀规则的声明名称，再按粗略词元（token）估算限制输出大小。
//!
//! 当前可以确认的上游有三类：Core 已授权后由 Daemon `RepoMapHandler` 调用；CLI 的 Direct
//! Connect bearer 路由直接调用；`kiana-query::context_manifest` 也复用扫描结果。Direct
//! Connect 是独立入口，不能因为调用本函数就推断它走过 ControlPlane。主要数据流是：
//!
//! ```text
//! 项目根目录
//!   → canonicalize + 根目录 .gitignore 简化规则
//!   → collect_paths() 收集/排序候选路径
//!   → map_file() 读取字节、过滤二进制/非法 UTF-8、提取符号/摘要/估算
//!   → build_repo_map() 按估算预算组成 RepoMap
//!   → Daemon capability 结果、Direct Connect JSON 或 context_manifest
//! ```
//!
//! ⚠ 这是只读上下文摘要，不是完整语法树、构建系统或权限扫描器。文件忽略、语言识别、符号
//! 提取和 token 计算都有明确简化；扫描本身不会替代 `ControlPlane` 的项目信任、PathLock
//! 或 sandbox 检查。输出里的哈希与符号也不能证明内容已通过编译或可信审计。
//!
//! `Serialize` / `Deserialize` 来自 serde：前者把 Rust 结构编码成 JSON 等 wire 格式，后者
//! 把外部值读回 Rust 结构。`#[serde(default)]` 让旧 JSON 缺少新增字段时使用该字段类型的
//! 默认值；本文件中缺少 `content_hash` 会变成空字符串。
//!
//! `anyhow::Result` 用于把文件系统失败一路返回调用入口；`with_context()`/`Context` 给底层
//! 错误补上目录或文件路径，`?` 再把该错误交给 `build_repo_map()` 的调用者。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// 调用者没有指定正预算时使用的默认输出估算上限。
const DEFAULT_TOKEN_BUDGET: u64 = 4_000;
/// 每个文本文件最多保留的行级启发式名称数；超过后停止检查该文件后续行。
const MAX_SYMBOLS_PER_FILE: usize = 12;

#[derive(Debug, Clone, Copy)]
/// 构建 repo map 时采用的大小限制。
pub struct RepoMapOptions {
    /// 输出允许使用的估算 token 上限；`None` 或 0 会回退到 `DEFAULT_TOKEN_BUDGET`（4,000）。
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
    /// 本次采用的粗略估算 token 预算；不是模型供应商 tokenizer 的精确计数。
    pub token_budget: u64,
    /// 已纳入文件条目的粗略估算 token 总数，不是模型供应商的精确计费数。
    ///
    /// 预算小于固定基数 8 时，此值仍可能大于 `token_budget`；当前实现没有拒绝该输入。
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
    /// 从文件 UTF-8 文本原始字节计算出的安全哈希（SHA-256）；符号提取使用同一份文本。
    ///
    /// serde 缺少该字段时按 `String::default()` 读成空串，用于兼容较早版本的 RepoMap JSON。
    /// 该摘要只识别字节变化，不带密钥，也不证明文件来源可信。
    #[serde(default)]
    pub content_hash: String,
    /// 相对于 map 根目录的正斜杠路径。
    pub path: String,
    /// 根据扩展名推断的语言；无法识别时为 `None`。
    pub language: Option<String>,
    /// UTF-8 文本的字节长度。
    pub bytes: u64,
    /// 路径、语言和符号名称的粗略 token 估算。
    pub estimated_tokens: u64,
    /// 最多保留 `MAX_SYMBOLS_PER_FILE`（当前为 12）个符合行前缀规则的启发式名称。
    ///
    /// 扫描器不检查语法作用域，所以这个字段不能保证名称属于语言意义上的顶层符号。
    pub symbols: Vec<String>,
}

/// 扫描仓库并构建稳定、受 token 预算限制的结构摘要。
///
/// 【调用者】Daemon 的 RepoMap capability handler、CLI Direct Connect repo-map 路由以及
/// `context_manifest` 会调用；分别承担受控 handler、直接 bearer HTTP adapter 和持久化上下文
/// manifest 的职责。
///
/// 【输入】`root` 是需要扫描的工作区根路径；签名中的 `impl AsRef<Path>` 接受 `Path`、`PathBuf`
/// 或字符串路径引用，函数开始时统一借用为 `&Path`。`options.max_tokens` 为 None/0 时使用 4,000。
///
/// 【核心流程】先解析规范化根目录；加载默认忽略项和根目录 `.gitignore` 的简化子集；递归
/// 收集所有扩展名可识别的路径并排序；逐个读取文件生成 RepoMapFile，再在预算超限时记录
/// omitted 数量而不纳入结果。扫描预算累计从固定 8 个估算单位开始；该基数和每文件公式
/// 都是估算，不是上游模型 tokenizer 的精确结果。
///
/// 【输出】返回含规范化根路径、预算、已纳入文件、估算总数、截断标志和省略数量的 RepoMap。
///
/// 【失败】根路径规范化、目录读取、文件类型读取或候选文件读取失败时通过 anyhow::Result
/// 返回带路径上下文的错误；根 `.gitignore` 不可读则只保留默认规则。二进制、NUL 字节、非法
/// UTF-8、未知扩展名或忽略路径被跳过，不作为错误。
///
/// 【副作用/设计边界】只读文件系统。注意 token budget 限制的是最终纳入摘要的规模，不限制
/// 前置路径收集量，也不阻止对后续超预算文件执行 `fs::read()`；文件数和单文件读取大小没有
/// 独立上限。路径先排序、再贪心纳入，避免依赖 `read_dir()` 的平台遍历顺序。
/// ⚠ `max_tokens` 在 1..=7 时，固定基数 8 已超过预算；有无文件都可能返回
/// `estimated_tokens > token_budget`，且无文件时 `truncated` 仍为 false。这是当前行为，不是预算保证。
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
    // 固定预留 8 个估算单位作为 map 基数；具体单位仍是下方启发式 token 公式，不是精确计费。
    // 不要据此假设小于 8 的 max_tokens 会被自动拒绝或调整。
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
/// 【调用者】由 `build_repo_map()` 从规范化根目录开始调用；找到的文件路径会交给 `map_file()`。
/// 【输入】`root` 用于计算相对路径，`dir` 是当前递归目录，`ignore_rules` 是已加载的过滤规则，
/// `paths` 是跨递归层共享的输出向量。
/// 【输出/副作用】只读目录项并把合格文件路径追加到 `paths`；不读文件内容、不修改文件。
/// 【失败】`read_dir()` 或读取条目类型失败时加上目录/文件路径上下文后整体返回 Err，而不是
/// 静默跳过权限错误。每层目录排序，顶层还会再次排序，抵御文件系统枚举顺序差异。
///
/// `DirEntry::file_type()` 不跟随符号链接；symlink 既不是这里继续递归的目录，也不会作为
///普通文件加入列表。此函数只应用过滤规则，不证明扫描目录属于可信项目。
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
        // 过滤先于递归，因此命中规则的目录下任何子文件都不会再被枚举。
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
/// 【调用者】由已排序路径循环调用；结果为 Some 时进入预算判断，为 None 时不计为 omitted。
/// 【输入/输出】读取 `path` 的完整字节；包含 NUL 或不是合法 UTF-8 就返回 None。否则计算相对
/// 正斜杠路径、扩展名语言、有限符号、估算 token 和原始文本 SHA-256，构造 RepoMapFile。
/// 【失败/资源边界】文件读取 I/O 错误会返回带路径上下文的 Err；`format!` 宏在这里构造
/// 带路径的错误上下文。虽然符号输出最多 12 项，
/// 这里仍会先用 `fs::read()` 将整个文件读入内存，没有单文件字节上限。
/// 【设计原因】只把可作为文本上下文的内容放进 map；符号和语言仍是启发式提示，不是 parser 结果。
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
        content_hash: {
            // sha2 crate 的 Digest trait 提供 Sha256::digest()，对 UTF-8 原始字节做无密钥哈希。
            use sha2::{Digest, Sha256};
            format!("sha256:{:x}", Sha256::digest(content.as_bytes()))
        },
        path: rel,
        language,
        bytes: content.len() as u64,
        estimated_tokens,
        symbols,
    }))
}

/// 把规范化路径转换成便于展示的字符串，并移除 Windows 扩展长度路径前缀。
///
/// 只变换显示文本，不访问或重新规范化文件系统路径；普通 Unix 路径原样转为 lossy String。
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
/// 计算 `(路径字节数 + 语言标签字节数 + 符号名称字节数) / 4 + 6`，并保证至少为 4。
/// 这里使用 Rust 字符串的 UTF-8 字节长度，不是 Unicode 字符数量。它只是粗略输出预算，
/// 不承诺与任何模型 tokenizer 一致，不能作为计费、配额或安全判断。
fn estimate_entry_tokens(path: &str, language: Option<&str>, symbols: &[String]) -> u64 {
    let symbol_chars = symbols.iter().map(String::len).sum::<usize>();
    let language_chars = language.map(str::len).unwrap_or(0);
    (((path.len() + language_chars + symbol_chars) as u64) / 4 + 6).max(4)
}

/// 根据文件扩展名返回内部语言标签。
///
/// 映射是白名单，未知扩展名返回 `None` 并被扫描器跳过；它不读取 shebang，也不判断
/// 文件真实语法，所以标签只能用于摘要展示和符号提取分派。扩展名匹配区分大小写，例如
/// `.rs` 被识别而 `.RS` 不在当前映射中。
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
/// 【调用者】`map_file()` 在文件通过 NUL/UTF-8 筛选后调用；结果放入 RepoMapFile，再参与估算。
/// 【输入/输出】逐行检查文本，返回最先遇到的 12 个可识别声明/标题；不保存源码行号或正文。
/// 【设计原因】避免引入完整 parser，让本地上下文摘要快速且能容忍不完整源码。
/// 【边界】只识别少量语言的简单行前缀，无法正确处理注释、宏、跨行声明、作用域或语法错误；
/// 提取名称不能当成编译器事实、符号索引或权限依据。
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

/// 从 Rust 行首识别少量常见声明前缀。
///
/// 会先去掉 `pub(crate)`、`pub`、`async` 等已列前缀，再尝试 `fn/struct/enum/trait/impl`。
/// 这是基于字符串的启发式：不会解析属性、注释或完整 Rust 语法。
fn rust_symbol(line: &str) -> Option<String> {
    let line = strip_prefixes(line, &["pub(crate) ", "pub ", "async "]);
    for keyword in ["fn", "struct", "enum", "trait", "impl"] {
        if let Some(name) = symbol_name_after(line, keyword) {
            return Some(format!("{keyword} {name}"));
        }
    }
    None
}

/// 从 Python 行首识别 `class` 或 `def` 声明。
///
/// 只读取关键字之后的第一个名字片段；装饰器、缩进语义和多行签名都不参与解析。
fn python_symbol(line: &str) -> Option<String> {
    for keyword in ["class", "def"] {
        if let Some(name) = symbol_name_after(line, keyword) {
            return Some(format!("{keyword} {name}"));
        }
    }
    None
}

/// 从 JavaScript/TypeScript 行首识别导出、函数、类型或变量声明。
///
/// 只剥离列出的 `export` 前缀，再按顺序尝试关键字；没有运行 parser，也不会辨认完整模块语义。
fn js_symbol(line: &str) -> Option<String> {
    let line = strip_prefixes(line, &["export default ", "export "]);
    for keyword in ["function", "class", "interface", "type", "const", "let"] {
        if let Some(name) = symbol_name_after(line, keyword) {
            return Some(format!("{keyword} {name}"));
        }
    }
    None
}

/// 为没有专用提取器的语言返回 Markdown 风格标题。
///
/// 由于调用者会把所有其他已识别扩展名也传入这里，它不等价于这些语言的符号解析器；
/// 例如 Go/Java/C 等文件当前只有匹配 `# 标题` 的行可能产生摘要符号。
fn generic_symbol(line: &str) -> Option<String> {
    line.strip_prefix("# ")
        .or_else(|| line.strip_prefix("## "))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| format!("heading {name}"))
}

/// 反复剥离行首的已知修饰前缀，并返回仍指向原输入的切片。
///
/// 生命周期 `'a` 表示返回的 `&str` 不拥有新字符串，其有效期不能超过传入的 `line`。
/// 使用循环是为了处理 `pub async fn` 这类多个修饰词连续出现的行；遇到一个都不匹配时停止。
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

/// 当一行以指定关键字开头时，提取其后连续的 ASCII 名称片段。
///
/// 允许字母、数字、下划线和连字符。当前只用 `starts_with`，没有额外要求关键字后是空格
/// 或语法分隔符，所以形如 `fnname` 的文本也可能被误识别为 `fn name`；这是启发式边界。
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
/// 组合默认目录、根目录 `.gitignore` 子集和简单通配符的忽略规则。
///
/// 这些规则只决定 RepoMap 是否展示路径，不删除文件、不修改 Git 忽略配置，也不是访问控制。
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
    /// 【输入/输出】以根目录默认名称/目录规则创建 HashSet，再尝试读取根目录唯一一份
    /// `.gitignore`；成功时将普通名称放入集合、目录名放入 `dir_names`、含 `*` 或 `/` 的模式
    /// 放入有序 Vec。相同忽略名自动去重。
    ///
    /// 【失败】`.gitignore` 不存在、不可读或不是合法 UTF-8 时静默保留默认规则；该错误不会
    /// 像目录/文件读取错误那样中止 `build_repo_map()`。
    ///
    /// 【简化】只读仓库根 `.gitignore`，跳过 `!` 否定行；不支持 Git 的完整转义、锚定和嵌套
    /// 规则。以 `/` 开头的部分会被剥除，末尾 `/` 会被归类为目录名。带斜杠的目录规则例如
    /// `src/generated/` 会作为字符串 `src/generated` 存入 `dir_names`，而后续按单个路径组件
    /// 比较，因此当前匹配器不会按预期命中该嵌套目录。
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
            // 注释、空行和“重新纳入”规则均跳过；因此这里不是完整 gitignore 解释器。
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
    /// 依次检查末级隐藏名称、默认文件名、目录名、每个路径组件和通配符。`names` 只匹配
    /// 文件/目录的末级名称；`dir_names` 也会对路径中每个普通组件生效。路径模式会对转换成
    /// `/` 的完整相对路径及末级文件名各尝试一次。
    ///
    /// 先做 HashSet 名称查找，再遍历较少的 path_patterns；命中后只影响 map，不改变文件。
    /// `rel` 应来自当前 root 下的路径，但此函数不做 canonicalize 或信任检查。
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

/// 判断一个普通路径组件是否属于目录忽略集合或 `.git`。
///
/// 根、当前目录等非 Normal 组件以及不能转成 UTF-8 的组件不匹配；它们不会因此触发忽略。
fn ignored_component(component: Component<'_>, rules: &IgnoreRules) -> bool {
    let Component::Normal(value) = component else {
        return false;
    };
    let Some(value) = value.to_str() else {
        return false;
    };
    rules.dir_names.contains(value) || value == ".git"
}

/// 返回仓库摘要默认忽略的常见元文件名集合。
///
/// 当前包含 `.DS_Store` 和 `Cargo.lock`；这些值只从 RepoMap 中排除，不会从仓库删除。
fn common_ignored_names() -> HashSet<String> {
    [".DS_Store", "Cargo.lock"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

/// 返回仓库摘要默认忽略的生成、依赖和缓存目录名集合。
///
/// 名称按任一路径组件匹配，所以即使同名目录嵌套在非根位置也会被跳过。
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
/// 实现按 `*` 切开模式：先核对非空前缀，再从剩余文本中按顺序寻找中间片段，最后要求非空
/// 后缀出现在文本末尾；单独的 `*` 匹配所有文本。
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
    //! RepoMap scanner 的临时文件系统 fixture。
    //!
    //! 测试在系统临时目录创建合成工作区，写入少量文件后调用公开 scanner，再删除该目录。
    //! 它覆盖本地文件行为，但不启动 Daemon、ControlPlane 或真实用户入口。

    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// 以进程 ID 和当前时间纳秒组成临时目录名，避免多个测试共享同一 fixture 路径。
    ///
    /// 创建前会尝试清理同名旧目录并忽略清理错误；目录创建失败则由测试中的 `unwrap()` 终止。
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
    /// 验证路径排序、默认/根 `.gitignore` 过滤和 Rust/Python/TypeScript 名称提取。
    ///
    /// 文件内容和规则全部由测试写入临时目录；断言不证明完整 Git 忽略语义或完整语言解析。
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
    /// 验证小预算会截掉部分已识别文件并在输出中反映截断状态和估算上限。
    ///
    /// 这里只检查 synthetic workspace 的当前估算行为，不验证真实模型 tokenizer 精度。
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

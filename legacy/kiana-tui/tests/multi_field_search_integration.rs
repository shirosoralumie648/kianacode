//! 多字段搜索集成测试

use kiana_tui::search::{AggregationMethod, MultiFieldSearchEngine, SearchConfig, SearchField};

#[derive(Debug, Clone)]
struct FileItem {
    path: String,
    name: String,
    content: String,
}

impl FileItem {
    fn new(path: &str, name: &str, content: &str) -> Self {
        Self {
            path: path.to_string(),
            name: name.to_string(),
            content: content.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
struct CommandItem {
    name: String,
    description: String,
    tags: String,
}

impl CommandItem {
    fn new(name: &str, description: &str, tags: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            tags: tags.to_string(),
        }
    }
}

#[test]
fn test_file_search_basic() {
    let files = vec![
        FileItem::new(
            "src/main.rs",
            "main.rs",
            "fn main() { println!(\"Hello\"); }",
        ),
        FileItem::new("src/lib.rs", "lib.rs", "pub mod utils;"),
        FileItem::new("tests/test_main.rs", "test_main.rs", "use main;"),
    ];

    let config = SearchConfig::for_files();
    let engine = MultiFieldSearchEngine::new(config);

    let results = engine.search_items(files, "main", |file, field| match field {
        "path" => Some(&file.path),
        "name" => Some(&file.name),
        "content" => Some(&file.content),
        _ => None,
    });

    assert!(!results.is_empty());
    assert!(results[0].matched_field_count > 0);
}

#[test]
fn test_file_search_with_weight() {
    let files = vec![
        FileItem::new("docs/readme.md", "readme.md", "This is the readme file"),
        FileItem::new("src/readme.rs", "readme.rs", "Module documentation"),
    ];

    let config = SearchConfig::for_files();
    let engine = MultiFieldSearchEngine::new(config);

    let results = engine.search_items(files, "readme", |file, field| match field {
        "path" => Some(&file.path),
        "name" => Some(&file.name),
        "content" => Some(&file.content),
        _ => None,
    });

    assert_eq!(results.len(), 2);
    // 文件名匹配的权重更高，应该排在前面
    assert!(results[0].has_field_match("name"));
}

#[test]
fn test_command_search() {
    let commands = vec![
        CommandItem::new("git commit", "Commit changes to repository", "git,vcs"),
        CommandItem::new("git push", "Push changes to remote", "git,vcs,remote"),
        CommandItem::new("commit-msg", "Git commit message hook", "hooks,git"),
    ];

    let config = SearchConfig::for_commands();
    let engine = MultiFieldSearchEngine::new(config);

    let results = engine.search_items(commands, "commit", |cmd, field| match field {
        "name" => Some(&cmd.name),
        "description" => Some(&cmd.description),
        "tags" => Some(&cmd.tags),
        _ => None,
    });

    assert!(!results.is_empty());
    // "git commit" 应该排在最前面（名字直接匹配）
    assert!(results[0].item.name.contains("commit"));
}

#[test]
fn test_weighted_sum_aggregation() {
    let files = vec![
        FileItem::new("src/test.rs", "test.rs", "Test file"),
        FileItem::new("tests/main.rs", "main.rs", "Test suite"),
    ];

    let config = SearchConfig::new(vec![
        SearchField::new("path").with_weight(1.0),
        SearchField::new("name").with_weight(2.0),
        SearchField::new("content").with_weight(0.5),
    ])
    .with_aggregation(AggregationMethod::WeightedSum);

    let engine = MultiFieldSearchEngine::new(config);

    let results = engine.search_items(files, "test", |file, field| match field {
        "path" => Some(&file.path),
        "name" => Some(&file.name),
        "content" => Some(&file.content),
        _ => None,
    });

    assert_eq!(results.len(), 2);
    assert!(results[0].total_score.is_finite());
}

#[test]
fn test_max_aggregation() {
    let files = vec![
        FileItem::new("src/test.rs", "test.rs", "Main file"),
        FileItem::new("docs/guide.md", "guide.md", "Test guide"),
    ];

    let config = SearchConfig::new(vec![
        SearchField::new("path").with_weight(1.0),
        SearchField::new("name").with_weight(2.0),
        SearchField::new("content").with_weight(1.0),
    ])
    .with_aggregation(AggregationMethod::Max);

    let engine = MultiFieldSearchEngine::new(config);

    let results = engine.search_items(files, "test", |file, field| match field {
        "path" => Some(&file.path),
        "name" => Some(&file.name),
        "content" => Some(&file.content),
        _ => None,
    });

    assert_eq!(results.len(), 2);
}

#[test]
fn test_required_field() {
    let files = vec![
        FileItem::new("src/main.rs", "main.rs", "Main file"),
        FileItem::new("tests/test.rs", "test.rs", "Test file"),
    ];

    let config = SearchConfig::new(vec![
        SearchField::new("path").with_weight(1.0).required(),
        SearchField::new("name").with_weight(2.0),
    ])
    .with_normalization(false);

    let engine = MultiFieldSearchEngine::new(config);

    // 搜索 "main" - 只有 src/main.rs 的 path 字段包含 "main"
    let results = engine.search_items(files, "main", |file, field| match field {
        "path" => Some(&file.path),
        "name" => Some(&file.name),
        _ => None,
    });

    // 由于 path 是必须字段，只有匹配 path 的才会返回
    assert!(!results.is_empty());
    for result in &results {
        assert!(result.has_field_match("path"));
    }
}

#[test]
fn test_normalization() {
    let files = vec![
        FileItem::new("src/test.rs", "test.rs", "Test file"),
        FileItem::new("tests/main.rs", "main.rs", "Main file"),
        FileItem::new("docs/test.md", "test.md", "Documentation"),
    ];

    let config = SearchConfig::for_files().with_normalization(true);
    let engine = MultiFieldSearchEngine::new(config);

    let results = engine.search_items(files, "test", |file, field| match field {
        "path" => Some(&file.path),
        "name" => Some(&file.name),
        "content" => Some(&file.content),
        _ => None,
    });

    assert!(!results.is_empty());
    // 检查归一化分数在 0-100 范围内
    for result in &results {
        assert!(result.normalized_score >= 0.0);
        assert!(result.normalized_score <= 100.0);
    }
    // 最佳匹配应该有最高的归一化分数
    assert_eq!(results[0].normalized_score, 100.0);
}

#[test]
fn test_boost_exact_match() {
    let files = vec![
        FileItem::new("src/test.rs", "test.rs", "Test file"),
        FileItem::new("tests/test_main.rs", "test_main.rs", "Main test file"),
    ];

    let config = SearchConfig::new(vec![
        SearchField::new("path").with_weight(1.0),
        SearchField::new("name")
            .with_weight(2.0)
            .with_boost_exact(true),
    ])
    .with_normalization(true);

    let engine = MultiFieldSearchEngine::new(config);

    let results = engine.search_items(files, "test.rs", |file, field| match field {
        "path" => Some(&file.path),
        "name" => Some(&file.name),
        _ => None,
    });

    assert!(!results.is_empty());
    // 精确匹配的应该排在前面
    assert_eq!(results[0].item.name, "test.rs");
}

#[test]
fn test_no_match() {
    let files = vec![
        FileItem::new("src/main.rs", "main.rs", "Main file"),
        FileItem::new("tests/test.rs", "test.rs", "Test file"),
    ];

    let config = SearchConfig::for_files();
    let engine = MultiFieldSearchEngine::new(config);

    let results = engine.search_items(files, "xyz", |file, field| match field {
        "path" => Some(&file.path),
        "name" => Some(&file.name),
        "content" => Some(&file.content),
        _ => None,
    });

    assert_eq!(results.len(), 0);
}

#[test]
fn test_field_match_details() {
    let files = vec![FileItem::new(
        "src/test_utils.rs",
        "test_utils.rs",
        "Test utilities",
    )];

    let config = SearchConfig::for_files();
    let engine = MultiFieldSearchEngine::new(config);

    let results = engine.search_items(files, "test", |file, field| match field {
        "path" => Some(&file.path),
        "name" => Some(&file.name),
        "content" => Some(&file.content),
        _ => None,
    });

    assert_eq!(results.len(), 1);
    let result = &results[0];

    // 检查字段匹配详情
    assert_eq!(result.field_matches.len(), 3);

    let path_match = result.get_field_match("path").unwrap();
    assert_eq!(path_match.matched, true);

    let name_match = result.get_field_match("name").unwrap();
    assert_eq!(name_match.matched, true);

    let content_match = result.get_field_match("content").unwrap();
    assert_eq!(content_match.matched, true);
}

#[test]
fn test_multiple_matches_ordering() {
    let files = vec![
        FileItem::new("test.rs", "test.rs", "Main test file"),
        FileItem::new("src/tests/mod.rs", "mod.rs", "Test module"),
        FileItem::new("docs/testing.md", "testing.md", "Test guide"),
        FileItem::new("examples/test_example.rs", "test_example.rs", "Example"),
    ];

    let config = SearchConfig::for_files();
    let engine = MultiFieldSearchEngine::new(config);

    let results = engine.search_items(files, "test", |file, field| match field {
        "path" => Some(&file.path),
        "name" => Some(&file.name),
        "content" => Some(&file.content),
        _ => None,
    });

    assert!(results.len() >= 2);
    // 结果应该按相关性排序
    for i in 0..results.len() - 1 {
        assert!(results[i].total_score >= results[i + 1].total_score);
    }
}

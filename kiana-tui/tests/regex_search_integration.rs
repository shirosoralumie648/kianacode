use kiana_tui::search::{HighlightConfig, RegexHighlighter, RegexSearch};

#[test]
fn test_regex_search_integration() {
    // 基本搜索
    let search = RegexSearch::new(r"\d+").unwrap();
    let text = "I have 5 apples and 3 oranges";
    let matches = search.search(text);

    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].text, "5");
    assert_eq!(matches[1].text, "3");
}

#[test]
fn test_regex_with_captures() {
    // 测试捕获组
    let search = RegexSearch::new(r"(\w+)@(\w+\.\w+)").unwrap();
    let text = "Contact: user@example.com";
    let matches = search.search(text);

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].text, "user@example.com");
    assert_eq!(matches[0].captures.len(), 3); // 完整匹配 + 2个捕获组
    assert_eq!(matches[0].captures[1].text, "user");
    assert_eq!(matches[0].captures[2].text, "example.com");
}

#[test]
fn test_regex_multiline() {
    // 测试多行模式
    let text = "line 1\nline 2\nline 3";
    let search = RegexSearch::new(r"^line \d")
        .unwrap()
        .multiline(true)
        .unwrap();

    let matches = search.search(text);
    assert_eq!(matches.len(), 3);
}

#[test]
fn test_regex_case_insensitive() {
    // 测试大小写不敏感
    let search = RegexSearch::new(r"hello")
        .unwrap()
        .case_sensitive(false)
        .unwrap();

    let matches = search.search("Hello HELLO hello");
    assert_eq!(matches.len(), 3);
}

#[test]
fn test_regex_error_handling() {
    // 测试错误处理
    let result = RegexSearch::new(r"[unclosed");
    assert!(result.is_err());

    let error = result.unwrap_err();
    assert!(error.suggestion.is_some());
}

#[test]
fn test_regex_replace() {
    // 测试替换功能
    let search = RegexSearch::new(r"\d+").unwrap();

    let result = search.replace("I have 5 apples", "ten");
    assert_eq!(result, "I have ten apples");

    let result = search.replace_all("I have 5 apples and 3 oranges", "X");
    assert_eq!(result, "I have X apples and X oranges");
}

#[test]
fn test_regex_highlighter() {
    // 测试高亮功能
    let search = RegexSearch::new(r"\d+").unwrap();
    let text = "There are 123 apples";
    let matches = search.search(text);

    let highlighter = RegexHighlighter::default_config();
    let spans = highlighter.highlight_match(text, &matches[0]);

    assert!(spans.len() >= 3); // before, match, after
    assert!(spans.iter().any(|s| s.is_match));
}

#[test]
fn test_regex_search_lines() {
    // 测试按行搜索
    let lines = vec!["line one with 123", "line two with 456", "line three"];
    let search = RegexSearch::new(r"\d+").unwrap();
    let matches = search.search_lines(&lines);

    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].line, 0);
    assert_eq!(matches[0].text, "123");
    assert_eq!(matches[1].line, 1);
    assert_eq!(matches[1].text, "456");
}

#[test]
fn test_unicode_support() {
    // 测试 Unicode 支持
    let search = RegexSearch::new(r"[一-龥]+").unwrap();
    let matches = search.search("Hello 世界 World 你好");

    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].text, "世界");
    assert_eq!(matches[1].text, "你好");
}

#[test]
fn test_word_boundaries() {
    // 测试词边界
    let search = RegexSearch::new(r"\btest\b").unwrap();
    let matches = search.search("test testing tested test");

    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].text, "test");
    assert_eq!(matches[1].text, "test");
}

#[test]
fn test_complex_regex() {
    // 测试复杂正则表达式
    let search = RegexSearch::new(r"(\d{1,3}\.){3}\d{1,3}").unwrap();
    let text = "Server IP: 192.168.1.1 and 10.0.0.1";
    let matches = search.search(text);

    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].text, "192.168.1.1");
    assert_eq!(matches[1].text, "10.0.0.1");
}

#[test]
fn test_highlight_with_captures() {
    // 测试捕获组高亮
    let search = RegexSearch::new(r"(\d+)-(\d+)").unwrap();
    let text = "Call 555-1234";
    let matches = search.search(text);

    let highlighter = RegexHighlighter::default_config();
    let spans = highlighter.highlight_match(text, &matches[0]);

    // 应该有捕获组被标记
    assert!(spans.iter().any(|s| s.is_capture));
}

#[test]
fn test_dot_matches_newline() {
    // 测试 . 匹配换行符
    let text = "start\nmiddle\nend";
    let search = RegexSearch::new(r"start.+end")
        .unwrap()
        .dot_matches_newline(true)
        .unwrap();

    let matches = search.search(text);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].text, text);
}

#[test]
fn test_anchors() {
    // 测试锚点
    let search_start = RegexSearch::new(r"^start").unwrap();
    assert!(search_start.find("start of line").is_some());
    assert!(search_start.find("not at start").is_none());

    let search_end = RegexSearch::new(r"end$").unwrap();
    assert!(search_end.find("at the end").is_some());
    assert!(search_end.find("end not there").is_none());
}

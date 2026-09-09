use kiana_tui::syntax::{HighlightOptions, HighlightTheme, SyntaxHighlighter};

#[test]
fn test_rust_syntax_highlighting() {
    let highlighter = SyntaxHighlighter::new();
    let code = r#"fn main() {
    println!("Hello, world!");
}"#;

    let options = HighlightOptions::default();
    let lines = highlighter.highlight(code, "rust", &options);

    assert_eq!(lines.len(), 3);
    // Each line should have multiple spans (line number + colored tokens)
    assert!(lines[0].spans.len() > 1);
}

#[test]
fn test_python_syntax_highlighting() {
    let highlighter = SyntaxHighlighter::new();
    let code = r#"def hello():
    print("Hello")"#;

    let options = HighlightOptions::default();
    let lines = highlighter.highlight(code, "python", &options);

    assert_eq!(lines.len(), 2);
}

#[test]
fn test_javascript_syntax_highlighting() {
    let highlighter = SyntaxHighlighter::new();
    let code = "function test() { return 42; }";

    let options = HighlightOptions::default();
    let lines = highlighter.highlight(code, "javascript", &options);

    assert_eq!(lines.len(), 1);
}

#[test]
fn test_json_syntax_highlighting() {
    let highlighter = SyntaxHighlighter::new();
    let code = r#"{"key": "value"}"#;

    let options = HighlightOptions::default();
    let lines = highlighter.highlight(code, "json", &options);

    assert_eq!(lines.len(), 1);
}

#[test]
fn test_line_numbers_enabled() {
    let highlighter = SyntaxHighlighter::new();
    let code = "let x = 42;\nlet y = 24;";

    let options = HighlightOptions {
        show_line_numbers: true,
        ..Default::default()
    };

    let lines = highlighter.highlight(code, "rust", &options);
    assert_eq!(lines.len(), 2);

    // First span should be the line number
    assert!(lines[0].spans[0].content.trim().parse::<usize>().is_ok());
}

#[test]
fn test_line_numbers_disabled() {
    let highlighter = SyntaxHighlighter::new();
    let code = "let x = 42;";

    let options = HighlightOptions {
        show_line_numbers: false,
        ..Default::default()
    };

    let lines = highlighter.highlight(code, "rust", &options);
    assert_eq!(lines.len(), 1);

    // Should not start with a number when line numbers are disabled
    let first_content = &lines[0].spans[0].content;
    assert!(!first_content
        .trim()
        .chars()
        .next()
        .unwrap_or(' ')
        .is_ascii_digit());
}

#[test]
fn test_unknown_language_fallback() {
    let highlighter = SyntaxHighlighter::new();
    let code = "some unknown code";

    let options = HighlightOptions::default();
    let lines = highlighter.highlight(code, "unknownlang", &options);

    // Should still work with plain text fallback
    assert_eq!(lines.len(), 1);
}

#[test]
fn test_empty_code() {
    let highlighter = SyntaxHighlighter::new();
    let code = "";

    let options = HighlightOptions::default();
    let lines = highlighter.highlight(code, "rust", &options);

    assert_eq!(lines.len(), 0);
}

#[test]
fn test_multiline_code() {
    let highlighter = SyntaxHighlighter::new();
    let code = "line1\nline2\nline3\nline4\nline5";

    let options = HighlightOptions::default();
    let lines = highlighter.highlight(code, "rust", &options);

    assert_eq!(lines.len(), 5);
}

#[test]
fn test_theme_switching() {
    let mut highlighter = SyntaxHighlighter::new();
    let code = "let x = 42;";

    // Test dark theme
    highlighter.set_theme(HighlightTheme::Dark);
    let options = HighlightOptions {
        theme: HighlightTheme::Dark,
        show_line_numbers: false,
        ..Default::default()
    };
    let dark_lines = highlighter.highlight(code, "rust", &options);

    // Test light theme
    highlighter.set_theme(HighlightTheme::Light);
    let options = HighlightOptions {
        theme: HighlightTheme::Light,
        show_line_numbers: false,
        ..Default::default()
    };
    let light_lines = highlighter.highlight(code, "rust", &options);

    // Both should have the same structure
    assert_eq!(dark_lines.len(), light_lines.len());
}

#[test]
fn test_language_aliases() {
    let highlighter = SyntaxHighlighter::new();
    let code = "let x = 42;";
    let options = HighlightOptions::default();

    // Test that aliases work
    let rust_lines = highlighter.highlight(code, "rust", &options);
    let rs_lines = highlighter.highlight(code, "rs", &options);

    assert_eq!(rust_lines.len(), rs_lines.len());
}

#[test]
fn test_bash_highlighting() {
    let highlighter = SyntaxHighlighter::new();
    let code = r#"#!/bin/bash
echo "Hello"
ls -la"#;

    let options = HighlightOptions::default();
    let lines = highlighter.highlight(code, "bash", &options);

    assert_eq!(lines.len(), 3);
}

#[test]
fn test_yaml_highlighting() {
    let highlighter = SyntaxHighlighter::new();
    let code = r#"name: test
version: 1.0
items:
  - one
  - two"#;

    let options = HighlightOptions::default();
    let lines = highlighter.highlight(code, "yaml", &options);

    assert_eq!(lines.len(), 5);
}

#[test]
fn test_markdown_code_blocks() {
    use kiana_tui::markdown::render_markdown;

    let markdown = r#"# Test

Here is some Rust code:

```rust
fn main() {
    println!("Hello!");
}
```

And some text after."#;

    let lines = render_markdown(markdown);

    // Should have multiple lines including the code
    assert!(lines.len() > 5);
}

#[test]
fn test_line_number_width() {
    let highlighter = SyntaxHighlighter::new();

    // Generate code with 100 lines
    let mut code = String::new();
    for i in 1..=100 {
        code.push_str(&format!("let x{} = {};\n", i, i));
    }

    let options = HighlightOptions {
        show_line_numbers: true,
        line_number_width: 4,
        ..Default::default()
    };

    let lines = highlighter.highlight(&code, "rust", &options);

    assert_eq!(lines.len(), 100);

    // Line 1 should have padded number
    let first_line = &lines[0].spans[0].content;
    assert!(first_line.contains('1'));

    // Line 100 should fit properly
    let last_line = &lines[99].spans[0].content;
    assert!(last_line.contains("100"));
}

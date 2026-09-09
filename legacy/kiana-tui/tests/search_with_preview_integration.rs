// Integration test for search with preview overlay
// Tests Phase 5 Feature 31: Search Preview

use kiana_tui::overlay::search_with_preview::{SearchResultWithPreview, SearchWithPreviewOverlay};

#[test]
fn test_search_with_preview_basic_workflow() {
    let mut overlay = SearchWithPreviewOverlay::new();

    // Prepare test data with full content for preview
    let items = vec![
        (
            "fn main() { println!(\"Hello\"); }".to_string(),
            Some("fn main() {\n    println!(\"Hello, world!\");\n}\n".to_string()),
        ),
        (
            "fn test() { assert!(true); }".to_string(),
            Some("fn test() {\n    assert!(true);\n}\n".to_string()),
        ),
        (
            "fn helper() { return 42; }".to_string(),
            Some("fn helper() {\n    return 42;\n}\n".to_string()),
        ),
    ];

    // Initial state
    assert_eq!(overlay.query(), "");
    assert_eq!(overlay.results().len(), 0);

    // Perform search
    overlay.update_query("main".to_string(), &items);
    assert_eq!(overlay.query(), "main");
    assert_eq!(overlay.results().len(), 1);
    assert_eq!(
        overlay.results()[0].text,
        "fn main() { println!(\"Hello\"); }"
    );

    // Search for multiple matches
    overlay.update_query("fn".to_string(), &items);
    assert_eq!(overlay.results().len(), 3);
}

#[test]
fn test_search_preview_navigation() {
    let mut overlay = SearchWithPreviewOverlay::new();

    let items = vec![
        ("result 1".to_string(), Some("content 1".to_string())),
        ("result 2".to_string(), Some("content 2".to_string())),
        ("result 3".to_string(), Some("content 3".to_string())),
    ];

    overlay.update_query("result".to_string(), &items);

    // Test navigation
    assert_eq!(overlay.selected_index(), 0);

    overlay.select_next();
    assert_eq!(overlay.selected_index(), 1);
    assert_eq!(overlay.selected_result().unwrap().text, "result 2");

    overlay.select_next();
    assert_eq!(overlay.selected_index(), 2);

    // Test boundary - should not go beyond
    overlay.select_next();
    assert_eq!(overlay.selected_index(), 2);

    // Navigate back
    overlay.select_previous();
    assert_eq!(overlay.selected_index(), 1);

    overlay.select_previous();
    assert_eq!(overlay.selected_index(), 0);

    // Test boundary - should not go below 0
    overlay.select_previous();
    assert_eq!(overlay.selected_index(), 0);
}

#[test]
fn test_search_preview_toggle() {
    let mut overlay = SearchWithPreviewOverlay::new();

    // Preview should be enabled by default
    assert!(overlay.show_preview());

    // Toggle preview off
    overlay.toggle_preview();
    assert!(!overlay.show_preview());

    // Toggle preview back on
    overlay.toggle_preview();
    assert!(overlay.show_preview());
}

#[test]
fn test_search_with_no_matches() {
    let mut overlay = SearchWithPreviewOverlay::new();

    let items = vec![
        ("apple".to_string(), None),
        ("banana".to_string(), None),
        ("cherry".to_string(), None),
    ];

    overlay.update_query("xyz".to_string(), &items);

    assert_eq!(overlay.results().len(), 0);
    assert!(overlay.selected_result().is_none());
}

#[test]
fn test_search_empty_query() {
    let mut overlay = SearchWithPreviewOverlay::new();

    let items = vec![("item 1".to_string(), None), ("item 2".to_string(), None)];

    overlay.update_query("".to_string(), &items);

    // Empty query should return no results
    assert_eq!(overlay.results().len(), 0);
}

#[test]
fn test_search_result_sorting() {
    let mut overlay = SearchWithPreviewOverlay::new();

    let items = vec![
        ("hello world".to_string(), None),
        ("hello".to_string(), None),
        ("say hello".to_string(), None),
    ];

    overlay.update_query("hello".to_string(), &items);

    // Results should be sorted by score (higher is better)
    assert_eq!(overlay.results().len(), 3);
    for i in 0..overlay.results().len() - 1 {
        assert!(overlay.results()[i].score >= overlay.results()[i + 1].score);
    }
}

#[test]
fn test_preview_with_multiline_content() {
    let mut overlay = SearchWithPreviewOverlay::new();

    let multiline_content = "\
line 1: introduction
line 2: some code
line 3: match here
line 4: more code
line 5: conclusion";

    let items = vec![(
        "line 3: match here".to_string(),
        Some(multiline_content.to_string()),
    )];

    overlay.update_query("match".to_string(), &items);

    assert_eq!(overlay.results().len(), 1);

    // Generate preview
    let preview = overlay.generate_preview();
    assert!(preview.is_some());

    let preview = preview.unwrap();
    // Should have context lines around the match
    assert!(preview.lines.len() > 1);
}

#[test]
fn test_search_result_with_preview_creation() {
    let result = SearchResultWithPreview::new(
        "test line".to_string(),
        5,
        vec![0, 1, 2],
        10,
        Some("full content".to_string()),
    );

    assert_eq!(result.text, "test line");
    assert_eq!(result.score, 5);
    assert_eq!(result.positions, vec![0, 1, 2]);
    assert_eq!(result.index, 10);
    assert!(result.full_content.is_some());
}

#[test]
fn test_incremental_search_updates() {
    let mut overlay = SearchWithPreviewOverlay::new();

    let items = vec![
        ("hello world".to_string(), None),
        ("hello rust".to_string(), None),
        ("goodbye".to_string(), None),
    ];

    // Start with broad search
    overlay.update_query("h".to_string(), &items);
    assert_eq!(overlay.results().len(), 2);

    // Refine search
    overlay.update_query("he".to_string(), &items);
    assert_eq!(overlay.results().len(), 2);

    // Further refine
    overlay.update_query("hello".to_string(), &items);
    assert_eq!(overlay.results().len(), 2);

    // Refine to single result
    overlay.update_query("hello w".to_string(), &items);
    assert_eq!(overlay.results().len(), 1);
    assert_eq!(overlay.results()[0].text, "hello world");
}

#[test]
fn test_result_limit() {
    let mut overlay = SearchWithPreviewOverlay::new();

    // Create 100 items that all match
    let items: Vec<(String, Option<String>)> = (0..100)
        .map(|i| (format!("test item {}", i), None))
        .collect();

    overlay.update_query("test".to_string(), &items);

    // Should be limited to 50 results
    assert_eq!(overlay.results().len(), 50);
}

#[test]
fn test_preview_without_full_content() {
    let mut overlay = SearchWithPreviewOverlay::new();

    let items = vec![("result without preview".to_string(), None)];

    overlay.update_query("result".to_string(), &items);
    assert_eq!(overlay.results().len(), 1);

    // Preview should be None when full_content is not available
    let preview = overlay.generate_preview();
    assert!(preview.is_none());
}

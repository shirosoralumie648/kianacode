//! Integration tests for code folding functionality

use kiana_tui::components::folding::{
    FoldKind, FoldRange, FoldState, FoldStateStore, FoldingManager, RustFoldDetector,
};
use std::env;
use std::path::PathBuf;

fn temp_storage_path() -> PathBuf {
    let temp_dir = env::temp_dir();
    temp_dir.join(format!(
        "test_folding_integration_{}.json",
        std::process::id()
    ))
}

#[test]
fn test_end_to_end_rust_folding() {
    let code = r#"fn main() {
    let x = 1;
    let y = 2;
    println!("{}, {}", x, y);
    let sum = x + y;
}

fn helper() {
    let a = 1;
    let b = 2;
    println!("Help!");
    println!("Done!");
}
"#;

    let mut manager = FoldingManager::new(code);
    let detector = RustFoldDetector::new();
    manager.detect_regions(&detector);

    // Should detect both functions
    let regions = manager.regions();
    assert!(
        regions.len() >= 2,
        "Should detect at least 2 functions, found: {}. Regions: {:?}",
        regions.len(),
        regions
    );

    // Initially all visible
    let initial_visible = manager.visible_line_count();
    assert!(initial_visible > 0);

    // Fold first function
    let folded = manager.fold_at(0);
    assert!(folded, "Should be able to fold the first function");
    let folded_visible = manager.visible_line_count();
    assert!(
        folded_visible < initial_visible,
        "Folding should hide lines"
    );

    // Unfold
    manager.unfold_at(0);
    assert_eq!(manager.visible_line_count(), initial_visible);
}

#[test]
fn test_nested_folding() {
    let code = r#"fn outer() {
    let x = 1;
    if true {
        let y = 2;
        for i in 0..5 {
            println!("{}", i);
            println!("More");
        }
    }
    let z = 3;
}
"#;

    let mut manager = FoldingManager::new(code);
    let detector = RustFoldDetector::new();
    manager.detect_regions(&detector);

    let initial_visible = manager.visible_line_count();

    // Fold outer function - should hide everything inside
    let folded = manager.fold_at(0);
    assert!(
        folded,
        "Should be able to fold outer function. Regions: {:?}",
        manager.regions()
    );

    let outer_folded_count = manager.visible_line_count();

    // Should show only the function definition line
    assert!(
        outer_folded_count < initial_visible,
        "Folded count ({}) should be less than initial ({})",
        outer_folded_count,
        initial_visible
    );
    assert!(outer_folded_count < 5, "Most lines should be hidden");

    // Unfold outer, then fold inner block
    manager.unfold_at(0);
    manager.fold_at(2); // if block starts at line 2

    let inner_folded_count = manager.visible_line_count();
    assert!(inner_folded_count < initial_visible);
    assert!(inner_folded_count > outer_folded_count);
}

#[test]
fn test_fold_all_unfold_all() {
    let code = r#"
fn first() {
    let a = 1;
}

fn second() {
    let b = 2;
}

fn third() {
    let c = 3;
}
"#;

    let mut manager = FoldingManager::new(code);
    let detector = RustFoldDetector::new();
    manager.detect_regions(&detector);

    let initial_visible = manager.visible_line_count();

    // Fold all
    manager.fold_all();
    let all_folded = manager.visible_line_count();
    assert!(all_folded < initial_visible);

    // Unfold all
    manager.unfold_all();
    assert_eq!(manager.visible_line_count(), initial_visible);
}

#[test]
fn test_fold_indicators() {
    let code = r#"
fn test() {
    let x = 1;
}
"#;

    let mut manager = FoldingManager::new(code);
    let detector = RustFoldDetector::new();
    manager.detect_regions(&detector);

    // Function line should have fold indicator
    let indicator = manager.get_fold_indicator(1);
    assert_ne!(indicator.symbol(), " ", "Should have fold indicator");

    // Fold it
    manager.fold_at(1);
    let folded_indicator = manager.get_fold_indicator(1);
    assert_eq!(folded_indicator.symbol(), "▶", "Should show folded symbol");

    // Unfold it
    manager.unfold_at(1);
    let unfolded_indicator = manager.get_fold_indicator(1);
    assert_eq!(
        unfolded_indicator.symbol(),
        "▼",
        "Should show expanded symbol"
    );
}

#[test]
fn test_fold_preview() {
    let code = r#"
fn long_function() {
    let a = 1;
    let b = 2;
    let c = 3;
    let d = 4;
}
"#;

    let mut manager = FoldingManager::new(code);
    let detector = RustFoldDetector::new();
    manager.detect_regions(&detector);

    // No preview when unfolded
    assert!(manager.get_fold_preview(1).is_none());

    // Fold it
    manager.fold_at(1);

    // Should have preview
    let preview = manager.get_fold_preview(1);
    assert!(preview.is_some());
    assert!(preview.unwrap().contains("lines"));
}

#[test]
fn test_state_persistence() {
    let code = r#"
fn test1() {
    let x = 1;
}

fn test2() {
    let y = 2;
}
"#;

    // Create manager and fold some regions
    let mut manager = FoldingManager::new(code);
    let detector = RustFoldDetector::new();
    manager.detect_regions(&detector);

    manager.fold_at(1); // Fold first function

    // Save state
    let state = FoldState::from_regions("/test.rs".to_string(), manager.regions());

    // Create new manager with same code
    let mut new_manager = FoldingManager::new(code);
    new_manager.detect_regions(&detector);

    // Initially all unfolded
    let unfolded_count = new_manager.visible_line_count();

    // Apply saved state by manually folding the same regions
    for (start, end) in &state.folded_ranges {
        // Find the region and fold it
        new_manager.fold_at(*start);
    }

    let restored_count = new_manager.visible_line_count();
    assert!(restored_count < unfolded_count, "Folds should be restored");
}

#[test]
fn test_state_store_integration() {
    let path = temp_storage_path();
    let code = r#"
fn test() {
    let x = 1;
}
"#;

    // Create and save state
    {
        let mut manager = FoldingManager::new(code);
        let detector = RustFoldDetector::new();
        manager.detect_regions(&detector);
        manager.fold_at(1);

        let mut store = FoldStateStore::new(path.clone());
        let state = FoldState::from_regions("/test.rs".to_string(), manager.regions());
        store.set_state(state);
        store.save_to_disk().unwrap();
    }

    // Load state in new process
    {
        let mut store = FoldStateStore::new(path.clone());
        store.load_from_disk().unwrap();

        let state = store.get_state("/test.rs");
        assert!(state.is_some());
        assert!(!state.unwrap().folded_ranges.is_empty());
    }

    // Cleanup
    let _ = std::fs::remove_file(path);
}

#[test]
fn test_comment_folding() {
    let code = r#"
// This is a comment
// Second line
// Third line
// Fourth line

fn test() {
    let x = 1;
}
"#;

    let mut manager = FoldingManager::new(code);
    let detector = RustFoldDetector::new();
    manager.detect_regions(&detector);

    // Should detect comment block
    let comment_region = manager
        .regions()
        .iter()
        .find(|r| r.kind == FoldKind::Comment);

    assert!(comment_region.is_some(), "Should detect comment fold");
}

#[test]
fn test_import_folding() {
    let code = r#"
use std::io;
use std::fs;
use std::path::Path;
use std::collections::HashMap;

fn main() {}
"#;

    let mut manager = FoldingManager::new(code);
    let detector = RustFoldDetector::new();
    manager.detect_regions(&detector);

    // Should detect import block
    let import_region = manager
        .regions()
        .iter()
        .find(|r| r.kind == FoldKind::Imports);

    assert!(import_region.is_some(), "Should detect import fold");
}

#[test]
fn test_empty_file() {
    let code = "";
    let mut manager = FoldingManager::new(code);
    let detector = RustFoldDetector::new();
    manager.detect_regions(&detector);

    assert_eq!(manager.total_lines(), 0);
    assert_eq!(manager.visible_line_count(), 0);
    assert!(manager.regions().is_empty());
}

#[test]
fn test_single_line_file() {
    let code = "fn main() {}";
    let mut manager = FoldingManager::new(code);
    let detector = RustFoldDetector::new();
    manager.detect_regions(&detector);

    // Single line functions don't fold (min_lines = 2)
    assert!(manager.regions().is_empty());
}

#[test]
fn test_large_file_performance() {
    // Generate a large file
    let mut code = String::new();
    for i in 0..1000 {
        code.push_str(&format!(
            "fn function_{}() {{\n    let x = {};\n    println!(\"{{}}\"s, x);\n}}\n\n",
            i, i
        ));
    }

    let start = std::time::Instant::now();

    let mut manager = FoldingManager::new(&code);
    let detector = RustFoldDetector::new();
    manager.detect_regions(&detector);

    let detection_time = start.elapsed();
    assert!(
        detection_time.as_millis() < 1000,
        "Detection should be fast"
    );

    // Test fold performance
    let start = std::time::Instant::now();
    manager.fold_all();
    let fold_time = start.elapsed();

    assert!(fold_time.as_millis() < 100, "Folding should be fast");

    // Test unfold performance
    let start = std::time::Instant::now();
    manager.unfold_all();
    let unfold_time = start.elapsed();

    assert!(unfold_time.as_millis() < 100, "Unfolding should be fast");
}

#[test]
fn test_visible_lines_correctness() {
    let code = r#"line 0
fn test() {
    line 2
    line 3
}
line 5"#;

    let mut manager = FoldingManager::new(code);
    let detector = RustFoldDetector::new();
    manager.detect_regions(&detector);

    // All visible initially
    let visible = manager.get_visible_lines();
    assert_eq!(visible.len(), 6);
    assert_eq!(visible, &[0, 1, 2, 3, 4, 5]);

    // Fold the function
    manager.fold_at(1);
    let visible = manager.get_visible_lines();

    // Should show: line 0, line 1 (function start), line 5
    assert_eq!(visible.len(), 3);
    assert_eq!(visible, &[0, 1, 5]);
}

#[test]
fn test_fold_range_properties() {
    let range = FoldRange::new(10, 20, FoldKind::Function);

    assert_eq!(range.line_count(), 11);
    assert_eq!(range.hidden_line_count(), 10);
    assert!(range.contains(15));
    assert!(!range.contains(25));
}

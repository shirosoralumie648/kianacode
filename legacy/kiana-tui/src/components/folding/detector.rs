//! Fold region detection
//!
//! This module provides functionality to detect foldable regions in source code.

use super::{FoldKind, FoldRange};

/// Trait for detecting foldable regions in source code
pub trait FoldRegionDetector {
    /// Detect all foldable regions in the given content
    fn detect(&self, content: &str) -> Vec<FoldRange>;
}

/// Generic fold detector based on indentation
pub struct GenericFoldDetector {
    /// Minimum number of lines for a foldable region
    min_lines: usize,
}

impl GenericFoldDetector {
    pub fn new() -> Self {
        Self { min_lines: 2 }
    }

    pub fn with_min_lines(min_lines: usize) -> Self {
        Self { min_lines }
    }

    fn get_indent_level(line: &str) -> usize {
        line.chars().take_while(|c| c.is_whitespace()).count()
    }
}

impl Default for GenericFoldDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl FoldRegionDetector for GenericFoldDetector {
    fn detect(&self, content: &str) -> Vec<FoldRange> {
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return Vec::new();
        }

        let mut regions = Vec::new();
        let mut stack: Vec<(usize, usize)> = Vec::new(); // (start_line, indent_level)

        for (line_idx, line) in lines.iter().enumerate() {
            if line.trim().is_empty() {
                continue;
            }

            let indent = Self::get_indent_level(line);

            // Close regions that have lower indentation
            while let Some(&(start, start_indent)) = stack.last() {
                if indent <= start_indent {
                    stack.pop();
                    if line_idx - start >= self.min_lines {
                        regions.push(FoldRange::new(start, line_idx - 1, FoldKind::Block));
                    }
                } else {
                    break;
                }
            }

            // Start new region if this line has content after it
            if line_idx + 1 < lines.len() {
                stack.push((line_idx, indent));
            }
        }

        // Close remaining regions
        while let Some((start, _)) = stack.pop() {
            if lines.len() - start >= self.min_lines {
                regions.push(FoldRange::new(start, lines.len() - 1, FoldKind::Block));
            }
        }

        Self::assign_levels(&mut regions);
        regions.sort();
        regions
    }
}

impl GenericFoldDetector {
    fn assign_levels(regions: &mut [FoldRange]) {
        for i in 0..regions.len() {
            let mut level = 0;
            for j in 0..regions.len() {
                if i != j && regions[j].contains_range(&regions[i]) {
                    level += 1;
                }
            }
            regions[i].level = level;
        }
    }
}

/// Rust-specific fold detector
pub struct RustFoldDetector {
    min_lines: usize,
}

impl RustFoldDetector {
    pub fn new() -> Self {
        Self { min_lines: 2 }
    }

    pub fn with_min_lines(min_lines: usize) -> Self {
        Self { min_lines }
    }
}

impl Default for RustFoldDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl FoldRegionDetector for RustFoldDetector {
    fn detect(&self, content: &str) -> Vec<FoldRange> {
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return Vec::new();
        }

        let mut regions = Vec::new();

        // Detect brace-based regions (functions, blocks, etc.)
        regions.extend(self.detect_brace_blocks(&lines));

        // Detect multi-line comments
        regions.extend(self.detect_comments(&lines));

        // Detect import blocks
        regions.extend(self.detect_imports(&lines));

        Self::assign_levels(&mut regions);
        regions.sort();
        regions
    }
}

impl RustFoldDetector {
    fn detect_brace_blocks(&self, lines: &[&str]) -> Vec<FoldRange> {
        let mut regions = Vec::new();
        let mut stack: Vec<(usize, FoldKind)> = Vec::new();

        for (line_idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Detect function/impl/struct/enum definitions
            let kind = if trimmed.starts_with("fn ")
                || trimmed.starts_with("pub fn ")
                || trimmed.starts_with("async fn ")
                || trimmed.starts_with("pub async fn ")
            {
                Some(FoldKind::Function)
            } else if trimmed.starts_with("impl ")
                || trimmed.starts_with("struct ")
                || trimmed.starts_with("enum ")
                || trimmed.starts_with("trait ")
                || trimmed.starts_with("pub struct ")
                || trimmed.starts_with("pub enum ")
                || trimmed.starts_with("pub trait ")
            {
                Some(FoldKind::Block)
            } else {
                None
            };

            // Opening brace
            if trimmed.contains('{') {
                if let Some(k) = kind {
                    stack.push((line_idx, k));
                } else if !stack.is_empty() {
                    // Nested block within a function/struct (only if we're already inside something)
                    stack.push((line_idx, FoldKind::Block));
                }
            }

            // Closing brace
            if trimmed.contains('}') {
                if let Some((start, fold_kind)) = stack.pop() {
                    if line_idx - start >= self.min_lines {
                        regions.push(FoldRange::new(start, line_idx, fold_kind));
                    }
                }
            }
        }

        regions
    }

    fn detect_comments(&self, lines: &[&str]) -> Vec<FoldRange> {
        let mut regions = Vec::new();
        let mut block_start: Option<usize> = None;
        let mut line_comment_start: Option<usize> = None;

        for (line_idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Multi-line comment blocks (/* ... */)
            if trimmed.starts_with("/*") {
                block_start = Some(line_idx);
            }
            if trimmed.ends_with("*/") {
                if let Some(start) = block_start.take() {
                    if line_idx - start >= self.min_lines {
                        regions.push(FoldRange::new(start, line_idx, FoldKind::Comment));
                    }
                }
            }

            // Consecutive line comments (// ...)
            if trimmed.starts_with("//") {
                if line_comment_start.is_none() {
                    line_comment_start = Some(line_idx);
                }
            } else if let Some(start) = line_comment_start.take() {
                if line_idx - start >= self.min_lines {
                    regions.push(FoldRange::new(start, line_idx - 1, FoldKind::Comment));
                }
            }
        }

        // Close any remaining line comment block
        if let Some(start) = line_comment_start {
            if lines.len() - start >= self.min_lines {
                regions.push(FoldRange::new(start, lines.len() - 1, FoldKind::Comment));
            }
        }

        regions
    }

    fn detect_imports(&self, lines: &[&str]) -> Vec<FoldRange> {
        let mut regions = Vec::new();
        let mut import_start: Option<usize> = None;

        for (line_idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            if trimmed.starts_with("use ") || trimmed.starts_with("extern crate ") {
                if import_start.is_none() {
                    import_start = Some(line_idx);
                }
            } else if !trimmed.is_empty() {
                // End of import block
                if let Some(start) = import_start.take() {
                    if line_idx - start >= self.min_lines {
                        regions.push(FoldRange::new(start, line_idx - 1, FoldKind::Imports));
                    }
                }
            }
        }

        // Close any remaining import block
        if let Some(start) = import_start {
            if lines.len() - start >= self.min_lines {
                regions.push(FoldRange::new(start, lines.len() - 1, FoldKind::Imports));
            }
        }

        regions
    }

    fn assign_levels(regions: &mut [FoldRange]) {
        for i in 0..regions.len() {
            let mut level = 0;
            for j in 0..regions.len() {
                if i != j && regions[j].contains_range(&regions[i]) {
                    level += 1;
                }
            }
            regions[i].level = level;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generic_detector_simple() {
        let content = "line 0\n  line 1\n  line 2\nline 3";
        let detector = GenericFoldDetector::new();
        let regions = detector.detect(content);

        assert!(!regions.is_empty());
    }

    #[test]
    fn test_generic_detector_empty() {
        let detector = GenericFoldDetector::new();
        let regions = detector.detect("");
        assert!(regions.is_empty());
    }

    #[test]
    fn test_rust_detector_function() {
        let content = r#"
fn main() {
    println!("Hello");
    let x = 1;
}
"#;
        let detector = RustFoldDetector::new();
        let regions = detector.detect(content);

        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].kind, FoldKind::Function);
        assert_eq!(regions[0].start_line, 1);
        assert_eq!(regions[0].end_line, 4);
    }

    #[test]
    fn test_rust_detector_nested_blocks() {
        let content = r#"
fn outer() {
    if true {
        let x = 1;
    }
}
"#;
        let detector = RustFoldDetector::new();
        let regions = detector.detect(content);

        assert!(regions.len() >= 1);
        let function_fold = regions.iter().find(|r| r.kind == FoldKind::Function);
        assert!(function_fold.is_some());
    }

    #[test]
    fn test_rust_detector_comments() {
        let content = r#"
// This is a comment
// Another line
// Third line
fn main() {}
"#;
        let detector = RustFoldDetector::new();
        let regions = detector.detect(content);

        let comment_fold = regions.iter().find(|r| r.kind == FoldKind::Comment);
        assert!(comment_fold.is_some());
    }

    #[test]
    fn test_rust_detector_multiline_comment() {
        let content = r#"
/*
 * Multi-line comment
 * Second line
 */
fn main() {}
"#;
        let detector = RustFoldDetector::new();
        let regions = detector.detect(content);

        let comment_fold = regions.iter().find(|r| r.kind == FoldKind::Comment);
        assert!(comment_fold.is_some());
    }

    #[test]
    fn test_rust_detector_imports() {
        let content = r#"
use std::io;
use std::fs;
use std::path::Path;

fn main() {}
"#;
        let detector = RustFoldDetector::new();
        let regions = detector.detect(content);

        let import_fold = regions.iter().find(|r| r.kind == FoldKind::Imports);
        assert!(import_fold.is_some());
    }

    #[test]
    fn test_rust_detector_struct() {
        let content = r#"
struct Point {
    x: i32,
    y: i32,
}
"#;
        let detector = RustFoldDetector::new();
        let regions = detector.detect(content);

        assert!(!regions.is_empty());
        assert_eq!(regions[0].kind, FoldKind::Block);
    }

    #[test]
    fn test_rust_detector_impl_block() {
        let content = r#"
impl Point {
    fn new() -> Self {
        Point { x: 0, y: 0 }
    }
}
"#;
        let detector = RustFoldDetector::new();
        let regions = detector.detect(content);

        assert!(regions.len() >= 1);
    }

    #[test]
    fn test_rust_detector_min_lines() {
        let content = r#"
fn short() {
}
"#;
        let detector = RustFoldDetector::with_min_lines(3);
        let regions = detector.detect(content);

        // Should not create fold for short function
        assert!(regions.is_empty());
    }

    #[test]
    fn test_fold_levels_nested() {
        let content = r#"
fn outer() {
    fn inner() {
        let x = 1;
    }
}
"#;
        let detector = RustFoldDetector::new();
        let regions = detector.detect(content);

        // Should have different levels for nested functions
        if regions.len() >= 2 {
            let levels: Vec<usize> = regions.iter().map(|r| r.level).collect();
            assert!(levels.iter().max().unwrap() > levels.iter().min().unwrap());
        }
    }

    #[test]
    fn test_async_function_detection() {
        let content = r#"
async fn fetch_data() {
    // async code
    let result = await_something();
}
"#;
        let detector = RustFoldDetector::new();
        let regions = detector.detect(content);

        assert!(!regions.is_empty());
        assert_eq!(regions[0].kind, FoldKind::Function);
    }

    #[test]
    fn test_pub_async_function_detection() {
        let content = r#"
pub async fn public_async() {
    // public async
}
"#;
        let detector = RustFoldDetector::new();
        let regions = detector.detect(content);

        assert!(!regions.is_empty());
        assert_eq!(regions[0].kind, FoldKind::Function);
    }
}

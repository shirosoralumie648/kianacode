//! Code folding support for the editor
//!
//! This module provides functionality to fold and unfold code regions,
//! making it easier to navigate large files by hiding sections of code.

use serde::{Deserialize, Serialize};

pub mod detector;
pub mod manager;
pub mod state_store;

pub use detector::{FoldRegionDetector, GenericFoldDetector, RustFoldDetector};
pub use manager::FoldingManager;
pub use state_store::{FoldState, FoldStateStore};

/// Unique identifier for a fold range
pub type FoldId = usize;

/// Represents a foldable region in the code
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoldRange {
    /// Starting line (0-indexed)
    pub start_line: usize,
    /// Ending line (inclusive, 0-indexed)
    pub end_line: usize,
    /// Nesting level (0 = top level)
    pub level: usize,
    /// Type of fold region
    pub kind: FoldKind,
    /// Whether this region is currently folded
    pub is_folded: bool,
}

impl FoldRange {
    /// Create a new fold range
    pub fn new(start_line: usize, end_line: usize, kind: FoldKind) -> Self {
        Self {
            start_line,
            end_line,
            level: 0,
            kind,
            is_folded: false,
        }
    }

    /// Check if this range contains the given line
    pub fn contains(&self, line: usize) -> bool {
        line >= self.start_line && line <= self.end_line
    }

    /// Check if this range contains another range
    pub fn contains_range(&self, other: &FoldRange) -> bool {
        self.start_line <= other.start_line && self.end_line >= other.end_line
    }

    /// Get the number of lines in this range
    pub fn line_count(&self) -> usize {
        self.end_line.saturating_sub(self.start_line) + 1
    }

    /// Get the number of hidden lines when folded
    pub fn hidden_line_count(&self) -> usize {
        self.end_line.saturating_sub(self.start_line)
    }
}

impl PartialOrd for FoldRange {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FoldRange {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Sort by start line first, then by end line (descending for nested folds)
        self.start_line
            .cmp(&other.start_line)
            .then_with(|| other.end_line.cmp(&self.end_line))
    }
}

/// Types of foldable regions
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FoldKind {
    /// Function or method definition
    Function,
    /// Code block (if, for, while, match, etc.)
    Block,
    /// Multi-line comment
    Comment,
    /// Import/use statements
    Imports,
    /// Custom fold region (marked by comments)
    Custom,
}

/// Visual indicator for fold state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldIndicator {
    /// Expanded and can be folded (▼)
    Expanded,
    /// Folded and can be expanded (▶)
    Folded,
    /// No fold region at this line
    None,
}

impl FoldIndicator {
    /// Get the display symbol for this indicator
    pub fn symbol(&self) -> &'static str {
        match self {
            FoldIndicator::Expanded => "▼",
            FoldIndicator::Folded => "▶",
            FoldIndicator::None => " ",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fold_range_creation() {
        let fold = FoldRange::new(10, 20, FoldKind::Function);
        assert_eq!(fold.start_line, 10);
        assert_eq!(fold.end_line, 20);
        assert_eq!(fold.kind, FoldKind::Function);
        assert!(!fold.is_folded);
        assert_eq!(fold.level, 0);
    }

    #[test]
    fn test_fold_range_contains() {
        let fold = FoldRange::new(10, 20, FoldKind::Function);
        assert!(fold.contains(10));
        assert!(fold.contains(15));
        assert!(fold.contains(20));
        assert!(!fold.contains(9));
        assert!(!fold.contains(21));
    }

    #[test]
    fn test_fold_range_contains_range() {
        let outer = FoldRange::new(10, 30, FoldKind::Function);
        let inner = FoldRange::new(15, 25, FoldKind::Block);
        let overlap = FoldRange::new(25, 35, FoldKind::Block);

        assert!(outer.contains_range(&inner));
        assert!(!outer.contains_range(&overlap));
        assert!(!inner.contains_range(&outer));
    }

    #[test]
    fn test_fold_range_line_count() {
        let fold = FoldRange::new(10, 20, FoldKind::Function);
        assert_eq!(fold.line_count(), 11);
        assert_eq!(fold.hidden_line_count(), 10);

        let single = FoldRange::new(5, 5, FoldKind::Block);
        assert_eq!(single.line_count(), 1);
        assert_eq!(single.hidden_line_count(), 0);
    }

    #[test]
    fn test_fold_range_ordering() {
        let mut folds = vec![
            FoldRange::new(20, 30, FoldKind::Block),
            FoldRange::new(10, 40, FoldKind::Function),
            FoldRange::new(10, 20, FoldKind::Block),
        ];

        folds.sort();

        assert_eq!(folds[0].start_line, 10);
        assert_eq!(folds[0].end_line, 40); // Outer fold first
        assert_eq!(folds[1].start_line, 10);
        assert_eq!(folds[1].end_line, 20); // Inner fold second
        assert_eq!(folds[2].start_line, 20);
    }

    #[test]
    fn test_fold_kind_variants() {
        let kinds = vec![
            FoldKind::Function,
            FoldKind::Block,
            FoldKind::Comment,
            FoldKind::Imports,
            FoldKind::Custom,
        ];

        for kind in kinds {
            let fold = FoldRange::new(0, 10, kind);
            assert_eq!(fold.kind, kind);
        }
    }

    #[test]
    fn test_fold_indicator_symbol() {
        assert_eq!(FoldIndicator::Expanded.symbol(), "▼");
        assert_eq!(FoldIndicator::Folded.symbol(), "▶");
        assert_eq!(FoldIndicator::None.symbol(), " ");
    }

    #[test]
    fn test_fold_range_serialization() {
        let fold = FoldRange::new(10, 20, FoldKind::Function);
        let json = serde_json::to_string(&fold).unwrap();
        let deserialized: FoldRange = serde_json::from_str(&json).unwrap();
        assert_eq!(fold, deserialized);
    }

    #[test]
    fn test_fold_range_edge_cases() {
        // Zero-width range
        let zero = FoldRange::new(10, 9, FoldKind::Block);
        assert_eq!(zero.line_count(), 1);
        assert_eq!(zero.hidden_line_count(), 0);

        // Large range
        let large = FoldRange::new(0, 10000, FoldKind::Function);
        assert_eq!(large.line_count(), 10001);
        assert!(large.contains(5000));
    }

    #[test]
    fn test_nested_fold_levels() {
        let mut outer = FoldRange::new(0, 100, FoldKind::Function);
        outer.level = 0;

        let mut inner = FoldRange::new(10, 50, FoldKind::Block);
        inner.level = 1;

        let mut innermost = FoldRange::new(20, 30, FoldKind::Block);
        innermost.level = 2;

        assert!(outer.level < inner.level);
        assert!(inner.level < innermost.level);
    }
}

//! Folding manager
//!
//! This module manages the state of foldable regions and provides operations
//! to fold, unfold, and query fold state.

use super::{FoldIndicator, FoldRange, FoldRegionDetector};
use std::collections::HashMap;

/// Manages fold regions and their state
pub struct FoldingManager {
    /// All detected foldable regions
    regions: Vec<FoldRange>,
    /// Original content lines
    lines: Vec<String>,
    /// Cache of visible line indices (updated when fold state changes)
    visible_lines: Vec<usize>,
    /// Quick lookup: line -> regions containing that line
    line_to_regions: HashMap<usize, Vec<usize>>,
}

impl FoldingManager {
    /// Create a new folding manager with the given content
    pub fn new(content: &str) -> Self {
        let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let mut manager = Self {
            regions: Vec::new(),
            lines,
            visible_lines: Vec::new(),
            line_to_regions: HashMap::new(),
        };
        manager.rebuild_visible_lines();
        manager
    }

    /// Detect foldable regions using the provided detector
    pub fn detect_regions(&mut self, detector: &dyn FoldRegionDetector) {
        let content = self.lines.join("\n");
        self.regions = detector.detect(&content);
        self.rebuild_line_map();
        self.rebuild_visible_lines();
    }

    /// Get all fold regions
    pub fn regions(&self) -> &[FoldRange] {
        &self.regions
    }

    /// Get mutable access to regions (for testing)
    #[cfg(test)]
    pub fn regions_mut(&mut self) -> &mut Vec<FoldRange> {
        &mut self.regions
    }

    /// Toggle fold at the given line
    pub fn toggle_fold_at(&mut self, line: usize) -> bool {
        if let Some(region_idx) = self.find_innermost_region_at(line) {
            self.regions[region_idx].is_folded = !self.regions[region_idx].is_folded;
            let is_folded = self.regions[region_idx].is_folded;
            self.rebuild_visible_lines();
            is_folded
        } else {
            false
        }
    }

    /// Fold the region at the given line
    pub fn fold_at(&mut self, line: usize) -> bool {
        if let Some(region_idx) = self.find_innermost_region_at(line) {
            if !self.regions[region_idx].is_folded {
                self.regions[region_idx].is_folded = true;
                self.rebuild_visible_lines();
                return true;
            }
        }
        false
    }

    /// Unfold the region at the given line
    pub fn unfold_at(&mut self, line: usize) -> bool {
        if let Some(region_idx) = self.find_innermost_region_at(line) {
            if self.regions[region_idx].is_folded {
                self.regions[region_idx].is_folded = false;
                self.rebuild_visible_lines();
                return true;
            }
        }
        false
    }

    /// Fold all regions up to the given level
    pub fn fold_level(&mut self, max_level: usize) {
        for region in &mut self.regions {
            if region.level <= max_level {
                region.is_folded = true;
            }
        }
        self.rebuild_visible_lines();
    }

    /// Unfold all regions down to the given level
    pub fn unfold_level(&mut self, min_level: usize) {
        for region in &mut self.regions {
            if region.level >= min_level {
                region.is_folded = false;
            }
        }
        self.rebuild_visible_lines();
    }

    /// Fold all regions
    pub fn fold_all(&mut self) {
        for region in &mut self.regions {
            region.is_folded = true;
        }
        self.rebuild_visible_lines();
    }

    /// Unfold all regions
    pub fn unfold_all(&mut self) {
        for region in &mut self.regions {
            region.is_folded = false;
        }
        self.rebuild_visible_lines();
    }

    /// Get the list of visible line indices
    pub fn get_visible_lines(&self) -> &[usize] {
        &self.visible_lines
    }

    /// Get the fold indicator for a given line
    pub fn get_fold_indicator(&self, line: usize) -> FoldIndicator {
        if let Some(region_idx) = self.find_fold_starting_at(line) {
            if self.regions[region_idx].is_folded {
                FoldIndicator::Folded
            } else {
                FoldIndicator::Expanded
            }
        } else {
            FoldIndicator::None
        }
    }

    /// Get the fold range starting at a specific line, if any
    pub fn get_fold_at(&self, line: usize) -> Option<&FoldRange> {
        self.find_fold_starting_at(line)
            .map(|idx| &self.regions[idx])
    }

    /// Get the number of total lines
    pub fn total_lines(&self) -> usize {
        self.lines.len()
    }

    /// Get the number of visible lines
    pub fn visible_line_count(&self) -> usize {
        self.visible_lines.len()
    }

    /// Check if a line is currently visible
    pub fn is_line_visible(&self, line: usize) -> bool {
        self.visible_lines.contains(&line)
    }

    /// Find the innermost (highest level) region containing the line
    fn find_innermost_region_at(&self, line: usize) -> Option<usize> {
        if let Some(region_indices) = self.line_to_regions.get(&line) {
            // Return the region with the highest level (innermost)
            region_indices
                .iter()
                .max_by_key(|&&idx| self.regions[idx].level)
                .copied()
        } else {
            None
        }
    }

    /// Find a fold region that starts at a specific line
    fn find_fold_starting_at(&self, line: usize) -> Option<usize> {
        self.regions.iter().position(|r| r.start_line == line)
    }

    /// Rebuild the line-to-regions mapping
    fn rebuild_line_map(&mut self) {
        self.line_to_regions.clear();
        for (idx, region) in self.regions.iter().enumerate() {
            for line in region.start_line..=region.end_line {
                self.line_to_regions
                    .entry(line)
                    .or_insert_with(Vec::new)
                    .push(idx);
            }
        }
    }

    /// Rebuild the visible lines list based on current fold state
    fn rebuild_visible_lines(&mut self) {
        self.visible_lines.clear();
        let mut skip_until: Option<usize> = None;

        for line_idx in 0..self.lines.len() {
            // If we're in a skipped region, check if we're past it
            if let Some(skip_end) = skip_until {
                if line_idx <= skip_end {
                    continue;
                }
                skip_until = None;
            }

            // Check if this line starts a folded region
            if let Some(region_idx) = self.find_fold_starting_at(line_idx) {
                let region = &self.regions[region_idx];
                if region.is_folded {
                    // Show the starting line, but skip everything else
                    self.visible_lines.push(line_idx);
                    skip_until = Some(region.end_line);
                    continue;
                }
            }

            // Check if this line is inside any folded region
            let mut inside_folded = false;
            if let Some(region_indices) = self.line_to_regions.get(&line_idx) {
                for &region_idx in region_indices {
                    let region = &self.regions[region_idx];
                    if region.is_folded && region.start_line != line_idx {
                        inside_folded = true;
                        skip_until = Some(region.end_line);
                        break;
                    }
                }
            }

            if !inside_folded {
                self.visible_lines.push(line_idx);
            }
        }
    }

    /// Get fold preview text for a folded region starting at line
    pub fn get_fold_preview(&self, line: usize) -> Option<String> {
        if let Some(region) = self.get_fold_at(line) {
            if region.is_folded {
                let hidden_count = region.hidden_line_count();
                return Some(format!("[+{} lines]", hidden_count));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::folding::{FoldKind, RustFoldDetector};

    #[test]
    fn test_folding_manager_creation() {
        let content = "line 1\nline 2\nline 3";
        let manager = FoldingManager::new(content);
        assert_eq!(manager.total_lines(), 3);
        assert_eq!(manager.visible_line_count(), 3);
    }

    #[test]
    fn test_detect_regions() {
        let content = r#"
fn main() {
    println!("Hello");
}
"#;
        let mut manager = FoldingManager::new(content);
        let detector = RustFoldDetector::new();
        manager.detect_regions(&detector);

        assert!(!manager.regions().is_empty());
    }

    #[test]
    fn test_toggle_fold() {
        let content = r#"fn main() {
    let x = 1;
    let y = 2;
}"#;
        let mut manager = FoldingManager::new(content);
        let detector = RustFoldDetector::new();
        manager.detect_regions(&detector);

        // Initially all visible
        assert_eq!(manager.visible_line_count(), 4);

        // Toggle fold
        let folded = manager.toggle_fold_at(0);
        assert!(folded);
        assert!(manager.visible_line_count() < 4);

        // Toggle again
        let unfolded = manager.toggle_fold_at(0);
        assert!(!unfolded);
        assert_eq!(manager.visible_line_count(), 4);
    }

    #[test]
    fn test_fold_all_unfold_all() {
        let content = r#"fn main() {
    let x = 1;
}
fn other() {
    let y = 2;
}"#;
        let mut manager = FoldingManager::new(content);
        let detector = RustFoldDetector::new();
        manager.detect_regions(&detector);

        let initial_visible = manager.visible_line_count();

        // Fold all
        manager.fold_all();
        assert!(manager.visible_line_count() < initial_visible);

        // Unfold all
        manager.unfold_all();
        assert_eq!(manager.visible_line_count(), initial_visible);
    }

    #[test]
    fn test_fold_indicator() {
        let content = r#"fn main() {
    let x = 1;
}"#;
        let mut manager = FoldingManager::new(content);
        let detector = RustFoldDetector::new();
        manager.detect_regions(&detector);

        // Should have indicator at function start
        let indicator = manager.get_fold_indicator(0);
        assert_eq!(indicator, FoldIndicator::Expanded);

        // Fold it
        manager.fold_at(0);
        let indicator = manager.get_fold_indicator(0);
        assert_eq!(indicator, FoldIndicator::Folded);
    }

    #[test]
    fn test_nested_folds() {
        let content = r#"fn outer() {
    if true {
        let x = 1;
    }
}"#;
        let mut manager = FoldingManager::new(content);
        let detector = RustFoldDetector::new();
        manager.detect_regions(&detector);

        // Fold outer function
        manager.fold_at(0);

        // Should hide inner content too
        assert!(!manager.is_line_visible(1));
        assert!(!manager.is_line_visible(2));
    }

    #[test]
    fn test_fold_level() {
        let mut manager = FoldingManager::new("test");

        // Manually add regions with different levels
        manager.regions_mut().push(FoldRange {
            start_line: 0,
            end_line: 10,
            level: 0,
            kind: FoldKind::Function,
            is_folded: false,
        });
        manager.regions_mut().push(FoldRange {
            start_line: 2,
            end_line: 8,
            level: 1,
            kind: FoldKind::Block,
            is_folded: false,
        });

        // Fold level 0 only
        manager.fold_level(0);
        assert!(manager.regions()[0].is_folded);
        assert!(!manager.regions()[1].is_folded);

        // Unfold all
        manager.unfold_all();

        // Fold all levels
        manager.fold_level(10);
        assert!(manager.regions()[0].is_folded);
        assert!(manager.regions()[1].is_folded);
    }

    #[test]
    fn test_fold_preview() {
        let content = r#"fn main() {
    line 2
    line 3
    line 4
}"#;
        let mut manager = FoldingManager::new(content);
        let detector = RustFoldDetector::new();
        manager.detect_regions(&detector);

        // Before folding, no preview
        assert!(manager.get_fold_preview(0).is_none());

        // After folding, show preview
        manager.fold_at(0);
        let preview = manager.get_fold_preview(0);
        assert!(preview.is_some());
        assert!(preview.unwrap().contains("lines"));
    }

    #[test]
    fn test_empty_content() {
        let manager = FoldingManager::new("");
        assert_eq!(manager.total_lines(), 0);
        assert_eq!(manager.visible_line_count(), 0);
    }

    #[test]
    fn test_no_foldable_regions() {
        let content = "line 1\nline 2";
        let mut manager = FoldingManager::new(content);
        let detector = RustFoldDetector::new();
        manager.detect_regions(&detector);

        assert_eq!(manager.visible_line_count(), 2);
        assert!(!manager.toggle_fold_at(0));
    }

    #[test]
    fn test_fold_at_unfold_at() {
        let content = r#"fn test() {
    code
}"#;
        let mut manager = FoldingManager::new(content);
        let detector = RustFoldDetector::new();
        manager.detect_regions(&detector);

        // Fold
        assert!(manager.fold_at(0));
        assert_eq!(manager.get_fold_indicator(0), FoldIndicator::Folded);

        // Try to fold again (should return false)
        assert!(!manager.fold_at(0));

        // Unfold
        assert!(manager.unfold_at(0));
        assert_eq!(manager.get_fold_indicator(0), FoldIndicator::Expanded);

        // Try to unfold again (should return false)
        assert!(!manager.unfold_at(0));
    }
}

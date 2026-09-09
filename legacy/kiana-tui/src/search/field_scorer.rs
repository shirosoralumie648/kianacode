//! 字段评分器
//!
//! 负责对单个字段进行匹配和评分

use crate::search::fuzzy::calculate_match_score;
use crate::search::multi_field::{FieldMatch, SearchField};

/// 字段评分器
pub struct FieldScorer;

impl FieldScorer {
    /// 对单个字段进行评分
    ///
    /// # Arguments
    ///
    /// * `field_value` - 字段的值
    /// * `pattern` - 搜索模式
    /// * `field_config` - 字段配置（包含权重等信息）
    ///
    /// # Returns
    ///
    /// 返回字段匹配结果，包含原始分数、加权分数和匹配位置
    /// 注意：分数越高越好（与新的 fuzzy search API 一致）
    pub fn score_field(field_value: &str, pattern: &str, field_config: &SearchField) -> FieldMatch {
        // 使用现有的 fuzzy search
        let match_result = calculate_match_score(field_value, pattern);

        let (raw_score, positions, matched) = match match_result {
            Some((score, pos)) => {
                let mut s = score as f64;

                // 精确匹配加成
                if field_config.boost_exact && field_value.to_lowercase() == pattern.to_lowercase()
                {
                    s *= 1.5; // 分数越高越好，所以乘以 1.5
                }

                (s, pos, true)
            }
            None => (0.0, vec![], false),
        };

        // 应用权重（分数越高越好，权重越大分数越高）
        let weighted_score = if matched {
            raw_score * field_config.weight
        } else {
            0.0
        };

        FieldMatch {
            field_name: field_config.name.clone(),
            raw_score,
            weighted_score,
            matched,
            positions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_score_field_exact_match() {
        let field = SearchField::new("title").with_weight(1.0);
        let result = FieldScorer::score_field("hello", "hello", &field);

        assert_eq!(result.matched, true);
        assert!(result.raw_score > 0.0);
        assert!(result.weighted_score > 0.0);
        assert_eq!(result.positions, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_score_field_partial_match() {
        let field = SearchField::new("title").with_weight(1.0);
        let result = FieldScorer::score_field("hello world", "world", &field);

        assert_eq!(result.matched, true);
        assert!(result.raw_score > 0.0);
        assert_eq!(result.weighted_score, result.raw_score); // weight = 1.0
    }

    #[test]
    fn test_score_field_no_match() {
        let field = SearchField::new("title").with_weight(1.0);
        let result = FieldScorer::score_field("hello world", "xyz", &field);

        assert_eq!(result.matched, false);
        assert_eq!(result.raw_score, 0.0);
        assert_eq!(result.weighted_score, 0.0);
        assert_eq!(result.positions.len(), 0);
    }

    #[test]
    fn test_score_field_with_weight() {
        let field = SearchField::new("title").with_weight(2.0);
        let result = FieldScorer::score_field("hello world", "world", &field);

        assert_eq!(result.matched, true);
        assert!(result.raw_score > 0.0);
        assert_eq!(result.weighted_score, result.raw_score * 2.0);
    }

    #[test]
    fn test_score_field_with_boost_exact() {
        let field = SearchField::new("title")
            .with_weight(1.0)
            .with_boost_exact(true);
        let result = FieldScorer::score_field("hello", "hello", &field);

        assert_eq!(result.matched, true);
        assert!(result.raw_score > 0.0);
        // 精确匹配应该得到加成
        let no_boost_field = SearchField::new("title").with_weight(1.0);
        let no_boost_result = FieldScorer::score_field("hello", "hello", &no_boost_field);
        assert!(result.raw_score > no_boost_result.raw_score);
    }

    #[test]
    fn test_score_field_with_boost_exact_case_insensitive() {
        let field = SearchField::new("title")
            .with_weight(1.0)
            .with_boost_exact(true);
        // 使用小写模式进行精确匹配（忽略大小写）
        let result = FieldScorer::score_field("hello", "hello", &field);

        assert_eq!(result.matched, true);
        assert!(result.raw_score > 0.0);
    }

    #[test]
    fn test_score_field_with_boost_exact_no_boost() {
        let field = SearchField::new("title")
            .with_weight(1.0)
            .with_boost_exact(true);
        let result = FieldScorer::score_field("hello world", "hello", &field);

        assert_eq!(result.matched, true);
        assert!(result.raw_score > 0.0);
    }

    #[test]
    fn test_score_field_case_insensitive() {
        let field = SearchField::new("title").with_weight(1.0);
        // 使用小写模式，触发智能大小写匹配（忽略大小写）
        let result = FieldScorer::score_field("Hello World", "world", &field);

        assert_eq!(result.matched, true);
    }

    #[test]
    fn test_score_field_empty_pattern() {
        let field = SearchField::new("title").with_weight(1.0);
        let result = FieldScorer::score_field("hello", "", &field);

        assert_eq!(result.matched, true);
        assert_eq!(result.raw_score, 0.0);
        assert_eq!(result.positions.len(), 0);
    }

    #[test]
    fn test_score_field_combined_weight_and_boost() {
        let field = SearchField::new("title")
            .with_weight(2.0)
            .with_boost_exact(true);
        let result = FieldScorer::score_field("test", "test", &field);

        assert_eq!(result.matched, true);
        assert!(result.raw_score > 0.0);
        assert!(result.weighted_score > result.raw_score); // 权重 > 1.0
    }
}

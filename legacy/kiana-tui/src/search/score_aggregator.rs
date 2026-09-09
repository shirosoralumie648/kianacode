//! 得分聚合器
//!
//! 负责聚合多个字段的得分并进行归一化

use crate::search::multi_field::{AggregationMethod, FieldMatch};

/// 得分聚合器
pub struct ScoreAggregator;

impl ScoreAggregator {
    /// 聚合多个字段的得分
    ///
    /// # Arguments
    ///
    /// * `field_matches` - 所有字段的匹配结果
    /// * `method` - 聚合方法
    ///
    /// # Returns
    ///
    /// 返回聚合后的总分数（分数越高越好）
    pub fn aggregate(field_matches: &[FieldMatch], method: AggregationMethod) -> f64 {
        match method {
            AggregationMethod::WeightedSum => Self::aggregate_weighted_sum(field_matches),
            AggregationMethod::Max => Self::aggregate_max(field_matches),
            AggregationMethod::Average => Self::aggregate_average(field_matches),
        }
    }

    /// 加权求和聚合
    ///
    /// 计算所有匹配字段的加权分数的平均值
    fn aggregate_weighted_sum(matches: &[FieldMatch]) -> f64 {
        let matched: Vec<_> = matches.iter().filter(|m| m.matched).collect();

        if matched.is_empty() {
            return 0.0;
        }

        // 分数越高越好，所以求平均
        matched.iter().map(|m| m.weighted_score).sum::<f64>() / matched.len() as f64
    }

    /// 最佳匹配聚合
    ///
    /// 返回所有匹配字段中的最大（最佳）加权分数
    fn aggregate_max(matches: &[FieldMatch]) -> f64 {
        matches
            .iter()
            .filter(|m| m.matched)
            .map(|m| m.weighted_score)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(0.0)
    }

    /// 平均加权聚合
    ///
    /// 与加权求和相同，返回平均值
    fn aggregate_average(matches: &[FieldMatch]) -> f64 {
        Self::aggregate_weighted_sum(matches)
    }

    /// 归一化分数
    ///
    /// 将分数归一化到 0-100 范围，分数越大越好
    ///
    /// # Arguments
    ///
    /// * `score` - 原始分数（越高越好）
    /// * `min_score` - 最小分数
    /// * `max_score` - 最大分数
    ///
    /// # Returns
    ///
    /// 返回归一化后的分数（0-100，越大越好）
    pub fn normalize(score: f64, min_score: f64, max_score: f64) -> f64 {
        if max_score == min_score {
            return 100.0;
        }

        // 分数越高越好，所以正向归一化
        ((score - min_score) / (max_score - min_score) * 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_field_match(field_name: &str, weighted_score: f64, matched: bool) -> FieldMatch {
        FieldMatch {
            field_name: field_name.to_string(),
            raw_score: weighted_score,
            weighted_score,
            matched,
            positions: vec![],
        }
    }

    #[test]
    fn test_aggregate_weighted_sum_single_match() {
        let matches = vec![create_field_match("title", 10.0, true)];

        let score = ScoreAggregator::aggregate(matches.as_slice(), AggregationMethod::WeightedSum);

        assert_eq!(score, 10.0);
    }

    #[test]
    fn test_aggregate_weighted_sum_multiple_matches() {
        let matches = vec![
            create_field_match("title", 10.0, true),
            create_field_match("body", 20.0, true),
        ];

        let score = ScoreAggregator::aggregate(matches.as_slice(), AggregationMethod::WeightedSum);

        assert_eq!(score, 15.0); // (10 + 20) / 2
    }

    #[test]
    fn test_aggregate_weighted_sum_with_non_matched() {
        let matches = vec![
            create_field_match("title", 10.0, true),
            create_field_match("body", 0.0, false),
            create_field_match("tags", 30.0, true),
        ];

        let score = ScoreAggregator::aggregate(matches.as_slice(), AggregationMethod::WeightedSum);

        assert_eq!(score, 20.0); // (10 + 30) / 2
    }

    #[test]
    fn test_aggregate_weighted_sum_no_matches() {
        let matches = vec![
            create_field_match("title", 0.0, false),
            create_field_match("body", 0.0, false),
        ];

        let score = ScoreAggregator::aggregate(matches.as_slice(), AggregationMethod::WeightedSum);

        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_aggregate_max_single_match() {
        let matches = vec![create_field_match("title", 10.0, true)];

        let score = ScoreAggregator::aggregate(matches.as_slice(), AggregationMethod::Max);

        assert_eq!(score, 10.0);
    }

    #[test]
    fn test_aggregate_max_multiple_matches() {
        let matches = vec![
            create_field_match("title", 10.0, true),
            create_field_match("body", 20.0, true),
            create_field_match("tags", 5.0, true),
        ];

        let score = ScoreAggregator::aggregate(matches.as_slice(), AggregationMethod::Max);

        assert_eq!(score, 20.0); // 最大（最佳）分数
    }

    #[test]
    fn test_aggregate_max_with_non_matched() {
        let matches = vec![
            create_field_match("title", 10.0, true),
            create_field_match("body", 0.0, false),
        ];

        let score = ScoreAggregator::aggregate(matches.as_slice(), AggregationMethod::Max);

        assert_eq!(score, 10.0);
    }

    #[test]
    fn test_aggregate_max_no_matches() {
        let matches = vec![create_field_match("title", 0.0, false)];

        let score = ScoreAggregator::aggregate(matches.as_slice(), AggregationMethod::Max);

        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_aggregate_average() {
        let matches = vec![
            create_field_match("title", 10.0, true),
            create_field_match("body", 20.0, true),
        ];

        let score = ScoreAggregator::aggregate(matches.as_slice(), AggregationMethod::Average);

        assert_eq!(score, 15.0);
    }

    #[test]
    fn test_normalize_min_score() {
        let normalized = ScoreAggregator::normalize(5.0, 5.0, 20.0);
        assert_eq!(normalized, 0.0);
    }

    #[test]
    fn test_normalize_max_score() {
        let normalized = ScoreAggregator::normalize(20.0, 5.0, 20.0);
        assert_eq!(normalized, 100.0);
    }

    #[test]
    fn test_normalize_mid_score() {
        let normalized = ScoreAggregator::normalize(12.5, 5.0, 20.0);
        assert_eq!(normalized, 50.0);
    }

    #[test]
    fn test_normalize_same_min_max() {
        let normalized = ScoreAggregator::normalize(10.0, 10.0, 10.0);
        assert_eq!(normalized, 100.0);
    }

    #[test]
    fn test_normalize_quarter() {
        let normalized = ScoreAggregator::normalize(8.75, 5.0, 20.0);
        assert_eq!(normalized, 25.0);
    }
}

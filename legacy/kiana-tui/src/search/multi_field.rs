//! 多字段搜索功能
//!
//! 支持同时搜索多个字段，为不同字段设置权重，聚合得分并显示匹配详情。
//!
//! # 示例
//!
//! ```rust
//! use kiana_tui::search::{SearchConfig, SearchField, MultiFieldSearchEngine, AggregationMethod};
//!
//! struct FileItem {
//!     path: String,
//!     name: String,
//!     content: String,
//! }
//!
//! let files = vec![FileItem {
//!     path: "src/main.rs".to_string(),
//!     name: "main.rs".to_string(),
//!     content: "fn main() {}".to_string(),
//! }];
//!
//! // 配置文件搜索
//! let config = SearchConfig::new(vec![
//!     SearchField::new("path").with_weight(1.0),
//!     SearchField::new("name").with_weight(2.0).with_boost_exact(true),
//!     SearchField::new("content").with_weight(0.5),
//! ])
//! .with_aggregation(AggregationMethod::WeightedSum)
//! .with_normalization(true);
//!
//! let engine = MultiFieldSearchEngine::new(config);
//!
//! // 搜索
//! let results = engine.search_items(files, "rust", |file, field| {
//!     match field {
//!         "path" => Some(&file.path),
//!         "name" => Some(&file.name),
//!         "content" => Some(&file.content),
//!         _ => None,
//!     }
//! });
//! assert!(results.len() <= 1);
//! ```

use crate::search::field_scorer::FieldScorer;
use crate::search::score_aggregator::ScoreAggregator;

/// 搜索字段配置
#[derive(Clone, Debug, PartialEq)]
pub struct SearchField {
    /// 字段名称
    pub name: String,
    /// 字段权重（1.0 = 标准）
    pub weight: f64,
    /// 精确匹配时是否加倍得分
    pub boost_exact: bool,
    /// 是否必须匹配（如果为 true，此字段不匹配则整个记录不匹配）
    pub required: bool,
}

impl SearchField {
    /// 创建新的搜索字段
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            weight: 1.0,
            boost_exact: false,
            required: false,
        }
    }

    /// 设置字段权重
    pub fn with_weight(mut self, weight: f64) -> Self {
        self.weight = weight;
        self
    }

    /// 设置精确匹配加成
    pub fn with_boost_exact(mut self, boost: bool) -> Self {
        self.boost_exact = boost;
        self
    }

    /// 设置为必须匹配的字段
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
}

/// 字段匹配结果
#[derive(Clone, Debug, PartialEq)]
pub struct FieldMatch {
    /// 字段名称
    pub field_name: String,
    /// 原始匹配分数
    pub raw_score: f64,
    /// 应用权重后的分数
    pub weighted_score: f64,
    /// 是否匹配
    pub matched: bool,
    /// 匹配位置
    pub positions: Vec<usize>,
}

/// 多字段搜索结果
#[derive(Clone, Debug, PartialEq)]
pub struct MultiFieldResult<T> {
    /// 匹配的项
    pub item: T,
    /// 总分数（分数越高越好）
    pub total_score: f64,
    /// 归一化分数（0-100，越大越好）
    pub normalized_score: f64,
    /// 每个字段的匹配详情
    pub field_matches: Vec<FieldMatch>,
    /// 匹配的字段数量
    pub matched_field_count: usize,
}

impl<T> MultiFieldResult<T> {
    /// 获取指定字段的匹配信息
    pub fn get_field_match(&self, field_name: &str) -> Option<&FieldMatch> {
        self.field_matches
            .iter()
            .find(|m| m.field_name == field_name)
    }

    /// 检查指定字段是否匹配
    pub fn has_field_match(&self, field_name: &str) -> bool {
        self.get_field_match(field_name)
            .map(|m| m.matched)
            .unwrap_or(false)
    }
}

/// 得分聚合方法
#[derive(Clone, Debug, PartialEq)]
pub enum AggregationMethod {
    /// 加权求和（默认）
    WeightedSum,
    /// 最佳匹配
    Max,
    /// 平均加权
    Average,
}

/// 搜索配置
#[derive(Clone, Debug)]
pub struct SearchConfig {
    /// 搜索字段配置
    pub fields: Vec<SearchField>,
    /// 得分聚合方法
    pub aggregation_method: AggregationMethod,
    /// 是否归一化分数
    pub normalize_scores: bool,
    /// 最小匹配分数阈值（分数越高越好，所以是最小值）
    pub min_score_threshold: f64,
}

impl SearchConfig {
    /// 创建新的搜索配置
    pub fn new(fields: Vec<SearchField>) -> Self {
        Self {
            fields,
            aggregation_method: AggregationMethod::WeightedSum,
            normalize_scores: true,
            min_score_threshold: 0.0, // 分数越高越好，所以阈值是最小值
        }
    }

    /// 设置聚合方法
    pub fn with_aggregation(mut self, method: AggregationMethod) -> Self {
        self.aggregation_method = method;
        self
    }

    /// 设置是否归一化分数
    pub fn with_normalization(mut self, normalize: bool) -> Self {
        self.normalize_scores = normalize;
        self
    }

    /// 设置分数阈值（只返回分数大于此值的结果）
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.min_score_threshold = threshold;
        self
    }

    /// 文件搜索预设配置
    pub fn for_files() -> Self {
        Self::new(vec![
            SearchField::new("path").with_weight(1.0),
            SearchField::new("name")
                .with_weight(2.0)
                .with_boost_exact(true),
            SearchField::new("content").with_weight(0.5),
        ])
        .with_aggregation(AggregationMethod::WeightedSum)
        .with_normalization(true)
    }

    /// 命令搜索预设配置
    pub fn for_commands() -> Self {
        Self::new(vec![
            SearchField::new("name")
                .with_weight(2.0)
                .with_boost_exact(true),
            SearchField::new("description").with_weight(1.0),
            SearchField::new("tags").with_weight(1.5),
        ])
        .with_aggregation(AggregationMethod::WeightedSum)
        .with_normalization(true)
    }

    /// 日志搜索预设配置
    pub fn for_logs() -> Self {
        Self::new(vec![
            SearchField::new("level").with_weight(1.5),
            SearchField::new("message").with_weight(1.0),
            SearchField::new("timestamp").with_weight(0.5),
        ])
        .with_aggregation(AggregationMethod::WeightedSum)
        .with_normalization(true)
    }
}

/// 多字段搜索引擎
pub struct MultiFieldSearchEngine {
    config: SearchConfig,
}

impl MultiFieldSearchEngine {
    /// 创建新的多字段搜索引擎
    pub fn new(config: SearchConfig) -> Self {
        Self { config }
    }

    /// 搜索单个项
    pub fn search_item<T, F>(
        &self,
        item: T,
        pattern: &str,
        field_extractor: F,
    ) -> Option<MultiFieldResult<T>>
    where
        for<'a> F: Fn(&'a T, &str) -> Option<&'a str>,
    {
        let mut field_matches = Vec::new();
        let mut required_matched = true;

        // 对每个字段进行评分
        for field_config in &self.config.fields {
            let field_value = field_extractor(&item, &field_config.name);

            let field_match = if let Some(value) = field_value {
                FieldScorer::score_field(value, pattern, field_config)
            } else {
                FieldMatch {
                    field_name: field_config.name.clone(),
                    raw_score: f64::MAX,
                    weighted_score: f64::MAX,
                    matched: false,
                    positions: vec![],
                }
            };

            // 检查必须匹配的字段
            if field_config.required && !field_match.matched {
                required_matched = false;
            }

            field_matches.push(field_match);
        }

        // 如果必须字段不匹配，返回 None
        if !required_matched {
            return None;
        }

        // 至少一个字段匹配
        let matched_count = field_matches.iter().filter(|m| m.matched).count();

        if matched_count == 0 {
            return None;
        }

        // 聚合得分
        let total_score =
            ScoreAggregator::aggregate(&field_matches, self.config.aggregation_method.clone());

        // 检查阈值（分数越高越好）
        if total_score < self.config.min_score_threshold {
            return None;
        }

        Some(MultiFieldResult {
            item,
            total_score,
            normalized_score: 0.0, // 稍后批量归一化
            field_matches,
            matched_field_count: matched_count,
        })
    }

    /// 搜索多个项
    pub fn search_items<T, F>(
        &self,
        items: impl IntoIterator<Item = T>,
        pattern: &str,
        field_extractor: F,
    ) -> Vec<MultiFieldResult<T>>
    where
        for<'a> F: Fn(&'a T, &str) -> Option<&'a str>,
    {
        let mut results: Vec<_> = items
            .into_iter()
            .filter_map(|item| self.search_item(item, pattern, &field_extractor))
            .collect();

        // 排序（分数越高越好，所以降序）
        results.sort_by(|a, b| {
            b.total_score
                .partial_cmp(&a.total_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // 归一化分数
        if self.config.normalize_scores && !results.is_empty() {
            let max_score = results.first().unwrap().total_score;
            let min_score = results.last().unwrap().total_score;

            for result in &mut results {
                result.normalized_score =
                    ScoreAggregator::normalize(result.total_score, min_score, max_score);
            }
        }

        results
    }

    /// 获取配置
    pub fn config(&self) -> &SearchConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_field_new() {
        let field = SearchField::new("title");
        assert_eq!(field.name, "title");
        assert_eq!(field.weight, 1.0);
        assert_eq!(field.boost_exact, false);
        assert_eq!(field.required, false);
    }

    #[test]
    fn test_search_field_builder() {
        let field = SearchField::new("title")
            .with_weight(2.0)
            .with_boost_exact(true)
            .required();

        assert_eq!(field.weight, 2.0);
        assert_eq!(field.boost_exact, true);
        assert_eq!(field.required, true);
    }

    #[test]
    fn test_aggregation_method() {
        let methods = vec![
            AggregationMethod::WeightedSum,
            AggregationMethod::Max,
            AggregationMethod::Average,
        ];

        for method in methods {
            let cloned = method.clone();
            assert_eq!(method, cloned);
        }
    }

    #[test]
    fn test_search_config_new() {
        let config = SearchConfig::new(vec![
            SearchField::new("title").with_weight(2.0),
            SearchField::new("body").with_weight(1.0),
        ]);

        assert_eq!(config.fields.len(), 2);
        assert_eq!(config.aggregation_method, AggregationMethod::WeightedSum);
        assert_eq!(config.normalize_scores, true);
    }

    #[test]
    fn test_search_config_builder() {
        let config = SearchConfig::new(vec![SearchField::new("test")])
            .with_aggregation(AggregationMethod::Max)
            .with_normalization(false)
            .with_threshold(50.0);

        assert_eq!(config.aggregation_method, AggregationMethod::Max);
        assert_eq!(config.normalize_scores, false);
        assert_eq!(config.min_score_threshold, 50.0);
    }

    #[test]
    fn test_search_config_presets() {
        let file_config = SearchConfig::for_files();
        assert_eq!(file_config.fields.len(), 3);
        assert_eq!(file_config.fields[0].name, "path");
        assert_eq!(file_config.fields[1].name, "name");
        assert_eq!(file_config.fields[2].name, "content");

        let cmd_config = SearchConfig::for_commands();
        assert_eq!(cmd_config.fields.len(), 3);

        let log_config = SearchConfig::for_logs();
        assert_eq!(log_config.fields.len(), 3);
    }

    #[test]
    fn test_multi_field_result_get_field_match() {
        let result = MultiFieldResult {
            item: "test",
            total_score: 10.0,
            normalized_score: 90.0,
            field_matches: vec![
                FieldMatch {
                    field_name: "title".to_string(),
                    raw_score: 5.0,
                    weighted_score: 2.5,
                    matched: true,
                    positions: vec![0, 1, 2],
                },
                FieldMatch {
                    field_name: "body".to_string(),
                    raw_score: 10.0,
                    weighted_score: 10.0,
                    matched: false,
                    positions: vec![],
                },
            ],
            matched_field_count: 1,
        };

        assert!(result.get_field_match("title").is_some());
        assert!(result.get_field_match("body").is_some());
        assert!(result.get_field_match("other").is_none());
    }

    #[test]
    fn test_multi_field_result_has_field_match() {
        let result = MultiFieldResult {
            item: "test",
            total_score: 10.0,
            normalized_score: 90.0,
            field_matches: vec![
                FieldMatch {
                    field_name: "title".to_string(),
                    raw_score: 5.0,
                    weighted_score: 2.5,
                    matched: true,
                    positions: vec![0, 1, 2],
                },
                FieldMatch {
                    field_name: "body".to_string(),
                    raw_score: 10.0,
                    weighted_score: 10.0,
                    matched: false,
                    positions: vec![],
                },
            ],
            matched_field_count: 1,
        };

        assert_eq!(result.has_field_match("title"), true);
        assert_eq!(result.has_field_match("body"), false);
        assert_eq!(result.has_field_match("other"), false);
    }
}

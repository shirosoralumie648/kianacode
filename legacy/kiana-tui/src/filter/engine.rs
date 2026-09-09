/// Filter evaluation engine
use super::ast::{CompareOp, FieldValue, FilterExpr};
use regex::Regex;
use std::collections::HashMap;

/// Trait for items that can be filtered
pub trait Filterable {
    /// Get field value by name
    fn get_field(&self, field: &str) -> Option<FieldValue>;

    /// Get all field names
    fn fields(&self) -> Vec<String>;

    /// Get all field values as strings (for Term matching)
    fn all_values(&self) -> Vec<String>;
}

/// Filter evaluation engine
pub struct FilterEngine {
    expr: FilterExpr,
    regex_cache: HashMap<String, Regex>,
}

impl FilterEngine {
    /// Create a new filter engine with the given expression
    pub fn new(expr: FilterExpr) -> Self {
        Self {
            expr,
            regex_cache: HashMap::new(),
        }
    }

    /// Check if an item matches the filter expression
    pub fn matches<T: Filterable>(&self, item: &T) -> bool {
        self.eval(&self.expr, item)
    }

    /// Evaluate an expression against an item
    fn eval<T: Filterable>(&self, expr: &FilterExpr, item: &T) -> bool {
        match expr {
            FilterExpr::And(left, right) => self.eval(left, item) && self.eval(right, item),
            FilterExpr::Or(left, right) => self.eval(left, item) || self.eval(right, item),
            FilterExpr::Not(inner) => !self.eval(inner, item),
            FilterExpr::Term(term) => {
                let term_lower = term.to_lowercase();
                item.all_values()
                    .iter()
                    .any(|v| v.to_lowercase().contains(&term_lower))
            }
            FilterExpr::FieldContains { field, value } => {
                if let Some(field_val) = item.get_field(field) {
                    field_val
                        .as_string()
                        .to_lowercase()
                        .contains(&value.to_lowercase())
                } else {
                    false
                }
            }
            FilterExpr::FieldEquals { field, value } => {
                if let Some(field_val) = item.get_field(field) {
                    field_val.as_string().to_lowercase() == value.to_lowercase()
                } else {
                    false
                }
            }
            FilterExpr::FieldRegex { field, pattern } => {
                if let Some(field_val) = item.get_field(field) {
                    self.match_regex(&field_val.as_string(), pattern)
                } else {
                    false
                }
            }
            FilterExpr::FieldCompare { field, op, value } => {
                if let Some(field_val) = item.get_field(field) {
                    self.compare(&field_val, *op, value)
                } else {
                    false
                }
            }
        }
    }

    fn match_regex(&self, text: &str, pattern: &str) -> bool {
        // Note: In a real implementation, we'd cache compiled regexes
        // For simplicity, we compile on each call here
        match Regex::new(pattern) {
            Ok(re) => re.is_match(text),
            Err(_) => false, // Invalid regex doesn't match
        }
    }

    fn compare(&self, left: &FieldValue, op: CompareOp, right: &FieldValue) -> bool {
        // Try numeric comparison first
        if let (Some(l), Some(r)) = (left.as_number(), right.as_number()) {
            match op {
                CompareOp::Greater => l > r,
                CompareOp::Less => l < r,
                CompareOp::GreaterEqual => l >= r,
                CompareOp::LessEqual => l <= r,
                CompareOp::NotEqual => (l - r).abs() > f64::EPSILON,
            }
        } else {
            // Fall back to string comparison
            let l = left.as_string();
            let r = right.as_string();
            match op {
                CompareOp::Greater => l > r,
                CompareOp::Less => l < r,
                CompareOp::GreaterEqual => l >= r,
                CompareOp::LessEqual => l <= r,
                CompareOp::NotEqual => l != r,
            }
        }
    }

    /// Get the underlying expression
    pub fn expr(&self) -> &FilterExpr {
        &self.expr
    }
}

// Helper implementation for HashMap-based items
impl Filterable for HashMap<String, FieldValue> {
    fn get_field(&self, field: &str) -> Option<FieldValue> {
        self.get(field).cloned()
    }

    fn fields(&self) -> Vec<String> {
        self.keys().cloned().collect()
    }

    fn all_values(&self) -> Vec<String> {
        self.values().map(|v| v.as_string()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestItem {
        fields: HashMap<String, FieldValue>,
    }

    impl TestItem {
        fn new() -> Self {
            Self {
                fields: HashMap::new(),
            }
        }

        fn with_field(mut self, name: &str, value: FieldValue) -> Self {
            self.fields.insert(name.to_string(), value);
            self
        }
    }

    impl Filterable for TestItem {
        fn get_field(&self, field: &str) -> Option<FieldValue> {
            self.fields.get(field).cloned()
        }

        fn fields(&self) -> Vec<String> {
            self.fields.keys().cloned().collect()
        }

        fn all_values(&self) -> Vec<String> {
            self.fields.values().map(|v| v.as_string()).collect()
        }
    }

    #[test]
    fn test_term_match() {
        let engine = FilterEngine::new(FilterExpr::Term("error".to_string()));

        let item = TestItem::new().with_field("status", FieldValue::String("error".to_string()));

        assert!(engine.matches(&item));
    }

    #[test]
    fn test_term_no_match() {
        let engine = FilterEngine::new(FilterExpr::Term("error".to_string()));

        let item = TestItem::new().with_field("status", FieldValue::String("success".to_string()));

        assert!(!engine.matches(&item));
    }

    #[test]
    fn test_field_contains() {
        let engine = FilterEngine::new(FilterExpr::FieldContains {
            field: "status".to_string(),
            value: "err".to_string(),
        });

        let item = TestItem::new().with_field("status", FieldValue::String("error".to_string()));

        assert!(engine.matches(&item));
    }

    #[test]
    fn test_field_equals() {
        let engine = FilterEngine::new(FilterExpr::FieldEquals {
            field: "status".to_string(),
            value: "error".to_string(),
        });

        let item = TestItem::new().with_field("status", FieldValue::String("error".to_string()));

        assert!(engine.matches(&item));

        let item2 = TestItem::new().with_field("status", FieldValue::String("errors".to_string()));

        assert!(!engine.matches(&item2));
    }

    #[test]
    fn test_field_compare_numbers() {
        let engine = FilterEngine::new(FilterExpr::FieldCompare {
            field: "priority".to_string(),
            op: CompareOp::Greater,
            value: FieldValue::Number(3.0),
        });

        let item = TestItem::new().with_field("priority", FieldValue::Number(5.0));

        assert!(engine.matches(&item));

        let item2 = TestItem::new().with_field("priority", FieldValue::Number(2.0));

        assert!(!engine.matches(&item2));
    }

    #[test]
    fn test_and_expression() {
        let engine = FilterEngine::new(FilterExpr::and(
            FilterExpr::FieldEquals {
                field: "status".to_string(),
                value: "error".to_string(),
            },
            FilterExpr::FieldCompare {
                field: "priority".to_string(),
                op: CompareOp::Greater,
                value: FieldValue::Number(3.0),
            },
        ));

        let item = TestItem::new()
            .with_field("status", FieldValue::String("error".to_string()))
            .with_field("priority", FieldValue::Number(5.0));

        assert!(engine.matches(&item));

        let item2 = TestItem::new()
            .with_field("status", FieldValue::String("error".to_string()))
            .with_field("priority", FieldValue::Number(2.0));

        assert!(!engine.matches(&item2));
    }

    #[test]
    fn test_or_expression() {
        let engine = FilterEngine::new(FilterExpr::or(
            FilterExpr::FieldEquals {
                field: "status".to_string(),
                value: "error".to_string(),
            },
            FilterExpr::FieldEquals {
                field: "status".to_string(),
                value: "warning".to_string(),
            },
        ));

        let item1 = TestItem::new().with_field("status", FieldValue::String("error".to_string()));

        let item2 = TestItem::new().with_field("status", FieldValue::String("warning".to_string()));

        let item3 = TestItem::new().with_field("status", FieldValue::String("info".to_string()));

        assert!(engine.matches(&item1));
        assert!(engine.matches(&item2));
        assert!(!engine.matches(&item3));
    }

    #[test]
    fn test_not_expression() {
        let engine = FilterEngine::new(FilterExpr::not(FilterExpr::Term("fixed".to_string())));

        let item1 = TestItem::new().with_field("status", FieldValue::String("error".to_string()));

        let item2 = TestItem::new().with_field("status", FieldValue::String("fixed".to_string()));

        assert!(engine.matches(&item1));
        assert!(!engine.matches(&item2));
    }

    #[test]
    fn test_complex_expression() {
        // (status=error OR status=warning) AND priority>3
        let engine = FilterEngine::new(FilterExpr::and(
            FilterExpr::or(
                FilterExpr::FieldEquals {
                    field: "status".to_string(),
                    value: "error".to_string(),
                },
                FilterExpr::FieldEquals {
                    field: "status".to_string(),
                    value: "warning".to_string(),
                },
            ),
            FilterExpr::FieldCompare {
                field: "priority".to_string(),
                op: CompareOp::Greater,
                value: FieldValue::Number(3.0),
            },
        ));

        let item1 = TestItem::new()
            .with_field("status", FieldValue::String("error".to_string()))
            .with_field("priority", FieldValue::Number(5.0));

        let item2 = TestItem::new()
            .with_field("status", FieldValue::String("warning".to_string()))
            .with_field("priority", FieldValue::Number(4.0));

        let item3 = TestItem::new()
            .with_field("status", FieldValue::String("error".to_string()))
            .with_field("priority", FieldValue::Number(2.0));

        let item4 = TestItem::new()
            .with_field("status", FieldValue::String("info".to_string()))
            .with_field("priority", FieldValue::Number(5.0));

        assert!(engine.matches(&item1));
        assert!(engine.matches(&item2));
        assert!(!engine.matches(&item3));
        assert!(!engine.matches(&item4));
    }

    #[test]
    fn test_regex_match() {
        let engine = FilterEngine::new(FilterExpr::FieldRegex {
            field: "message".to_string(),
            pattern: r"error:\s+\d+".to_string(),
        });

        let item1 =
            TestItem::new().with_field("message", FieldValue::String("error: 404".to_string()));

        let item2 = TestItem::new().with_field("message", FieldValue::String("error".to_string()));

        assert!(engine.matches(&item1));
        assert!(!engine.matches(&item2));
    }
}

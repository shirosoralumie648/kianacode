/// Abstract Syntax Tree for filter expressions

#[derive(Debug, Clone, PartialEq)]
pub enum FilterExpr {
    /// Logical AND: both expressions must match
    And(Box<FilterExpr>, Box<FilterExpr>),

    /// Logical OR: at least one expression must match
    Or(Box<FilterExpr>, Box<FilterExpr>),

    /// Logical NOT: expression must not match
    Not(Box<FilterExpr>),

    /// Field contains value (case-insensitive substring match)
    FieldContains { field: String, value: String },

    /// Field equals value (case-insensitive exact match)
    FieldEquals { field: String, value: String },

    /// Field matches regex pattern
    FieldRegex { field: String, pattern: String },

    /// Field comparison (>, <, >=, <=, !=)
    FieldCompare {
        field: String,
        op: CompareOp,
        value: FieldValue,
    },

    /// Simple term search (matches any field containing the term)
    Term(String),
}

impl FilterExpr {
    /// Create an AND expression
    pub fn and(left: FilterExpr, right: FilterExpr) -> Self {
        Self::And(Box::new(left), Box::new(right))
    }

    /// Create an OR expression
    pub fn or(left: FilterExpr, right: FilterExpr) -> Self {
        Self::Or(Box::new(left), Box::new(right))
    }

    /// Create a NOT expression
    pub fn not(expr: FilterExpr) -> Self {
        Self::Not(Box::new(expr))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Greater,
    Less,
    GreaterEqual,
    LessEqual,
    NotEqual,
}

impl CompareOp {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            ">" => Some(Self::Greater),
            "<" => Some(Self::Less),
            ">=" => Some(Self::GreaterEqual),
            "<=" => Some(Self::LessEqual),
            "!=" => Some(Self::NotEqual),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Greater => ">",
            Self::Less => "<",
            Self::GreaterEqual => ">=",
            Self::LessEqual => "<=",
            Self::NotEqual => "!=",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    String(String),
    Number(f64),
    Boolean(bool),
}

impl FieldValue {
    pub fn as_string(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Number(n) => n.to_string(),
            Self::Boolean(b) => b.to_string(),
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            Self::String(s) => s.parse().ok(),
            Self::Boolean(b) => Some(if *b { 1.0 } else { 0.0 }),
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Self::Boolean(b) => *b,
            Self::Number(n) => *n != 0.0,
            Self::String(s) => !s.is_empty() && s.to_lowercase() != "false",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_op_from_str() {
        assert_eq!(CompareOp::from_str(">"), Some(CompareOp::Greater));
        assert_eq!(CompareOp::from_str("<="), Some(CompareOp::LessEqual));
        assert_eq!(CompareOp::from_str("invalid"), None);
    }

    #[test]
    fn test_field_value_conversions() {
        let val = FieldValue::Number(42.5);
        assert_eq!(val.as_number(), Some(42.5));
        assert_eq!(val.as_string(), "42.5");

        let val = FieldValue::String("123".to_string());
        assert_eq!(val.as_number(), Some(123.0));

        let val = FieldValue::Boolean(true);
        assert!(val.as_bool());
        assert_eq!(val.as_number(), Some(1.0));
    }

    #[test]
    fn test_expr_builders() {
        let expr = FilterExpr::and(
            FilterExpr::Term("foo".to_string()),
            FilterExpr::Term("bar".to_string()),
        );

        match expr {
            FilterExpr::And(_, _) => (),
            _ => panic!("Expected And expression"),
        }
    }
}

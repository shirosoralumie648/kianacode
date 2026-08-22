/// Parser for filter expressions
///
/// Grammar:
/// expr     := or_expr
/// or_expr  := and_expr (OR and_expr)*
/// and_expr := not_expr (AND not_expr | not_expr)*
/// not_expr := NOT primary | primary
/// primary  := field_op | term | (expr)
/// field_op := IDENT (: | = | ~ | COMPARE) VALUE
/// term     := VALUE
use super::ast::{CompareOp, FieldValue, FilterExpr};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    UnexpectedToken {
        expected: String,
        got: String,
        position: usize,
    },
    UnexpectedEof {
        expected: String,
    },
    InvalidOperator {
        operator: String,
        position: usize,
    },
    InvalidNumber {
        value: String,
        position: usize,
    },
    UnclosedParenthesis {
        position: usize,
    },
    EmptyExpression,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedToken {
                expected,
                got,
                position,
            } => {
                write!(
                    f,
                    "Unexpected token at position {}: expected {}, got {}",
                    position, expected, got
                )
            }
            Self::UnexpectedEof { expected } => {
                write!(f, "Unexpected end of input: expected {}", expected)
            }
            Self::InvalidOperator { operator, position } => {
                write!(
                    f,
                    "Invalid operator '{}' at position {}",
                    operator, position
                )
            }
            Self::InvalidNumber { value, position } => {
                write!(f, "Invalid number '{}' at position {}", value, position)
            }
            Self::UnclosedParenthesis { position } => {
                write!(f, "Unclosed parenthesis at position {}", position)
            }
            Self::EmptyExpression => {
                write!(f, "Empty expression")
            }
        }
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    // Literals
    Ident(String),
    String(String),
    Number(f64),

    // Operators
    And,
    Or,
    Not,

    // Field operators
    Colon,     // :
    Equals,    // =
    Tilde,     // ~
    Greater,   // >
    Less,      // <
    GreaterEq, // >=
    LessEq,    // <=
    NotEq,     // !=

    // Grouping
    LParen,
    RParen,

    Eof,
}

struct Lexer {
    input: Vec<char>,
    position: usize,
}

impl Lexer {
    fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            position: 0,
        }
    }

    fn current(&self) -> Option<char> {
        if self.position < self.input.len() {
            Some(self.input[self.position])
        } else {
            None
        }
    }

    fn peek(&self, offset: usize) -> Option<char> {
        let pos = self.position + offset;
        if pos < self.input.len() {
            Some(self.input[pos])
        } else {
            None
        }
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.current();
        if ch.is_some() {
            self.position += 1;
        }
        ch
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.current() {
            if ch.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn read_ident(&mut self) -> String {
        let mut ident = String::new();
        while let Some(ch) = self.current() {
            if ch.is_alphanumeric() || ch == '_' || ch == '-' {
                ident.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        ident
    }

    fn read_string(&mut self, quote: char) -> String {
        self.advance(); // skip opening quote
        let mut s = String::new();
        while let Some(ch) = self.current() {
            if ch == quote {
                self.advance(); // skip closing quote
                break;
            } else if ch == '\\' {
                self.advance();
                if let Some(escaped) = self.advance() {
                    s.push(escaped);
                }
            } else {
                s.push(ch);
                self.advance();
            }
        }
        s
    }

    fn read_number(&mut self) -> Result<f64, ParseError> {
        let start = self.position;
        let mut num_str = String::new();

        while let Some(ch) = self.current() {
            if ch.is_ascii_digit() || ch == '.' || ch == '-' {
                num_str.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        num_str.parse().map_err(|_| ParseError::InvalidNumber {
            value: num_str,
            position: start,
        })
    }

    fn next_token(&mut self) -> Result<Token, ParseError> {
        self.skip_whitespace();

        let Some(ch) = self.current() else {
            return Ok(Token::Eof);
        };

        let start_pos = self.position;

        match ch {
            '(' => {
                self.advance();
                Ok(Token::LParen)
            }
            ')' => {
                self.advance();
                Ok(Token::RParen)
            }
            ':' => {
                self.advance();
                Ok(Token::Colon)
            }
            '~' => {
                self.advance();
                Ok(Token::Tilde)
            }
            '=' => {
                self.advance();
                Ok(Token::Equals)
            }
            '>' => {
                self.advance();
                if self.current() == Some('=') {
                    self.advance();
                    Ok(Token::GreaterEq)
                } else {
                    Ok(Token::Greater)
                }
            }
            '<' => {
                self.advance();
                if self.current() == Some('=') {
                    self.advance();
                    Ok(Token::LessEq)
                } else {
                    Ok(Token::Less)
                }
            }
            '!' => {
                self.advance();
                if self.current() == Some('=') {
                    self.advance();
                    Ok(Token::NotEq)
                } else {
                    Err(ParseError::InvalidOperator {
                        operator: "!".to_string(),
                        position: start_pos,
                    })
                }
            }
            '"' | '\'' => {
                let s = self.read_string(ch);
                Ok(Token::String(s))
            }
            _ if ch.is_ascii_digit() || ch == '-' => {
                let num = self.read_number()?;
                Ok(Token::Number(num))
            }
            _ if ch.is_alphabetic() || ch == '_' => {
                let ident = self.read_ident();
                match ident.to_uppercase().as_str() {
                    "AND" => Ok(Token::And),
                    "OR" => Ok(Token::Or),
                    "NOT" => Ok(Token::Not),
                    _ => Ok(Token::Ident(ident)),
                }
            }
            _ => {
                self.advance();
                Ok(Token::Ident(ch.to_string()))
            }
        }
    }
}

pub struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn new(input: &str) -> Result<Self, ParseError> {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();

        loop {
            let token = lexer.next_token()?;
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        Ok(Self {
            tokens,
            position: 0,
        })
    }

    fn current(&self) -> &Token {
        &self.tokens[self.position]
    }

    fn advance(&mut self) -> &Token {
        if self.position < self.tokens.len() - 1 {
            self.position += 1;
        }
        &self.tokens[self.position]
    }

    fn parse(&mut self) -> Result<FilterExpr, ParseError> {
        if *self.current() == Token::Eof {
            return Err(ParseError::EmptyExpression);
        }
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<FilterExpr, ParseError> {
        let mut left = self.parse_and()?;

        while *self.current() == Token::Or {
            self.advance();
            let right = self.parse_and()?;
            left = FilterExpr::or(left, right);
        }

        Ok(left)
    }

    fn parse_and(&mut self) -> Result<FilterExpr, ParseError> {
        let mut left = self.parse_not()?;

        loop {
            match self.current() {
                Token::And => {
                    self.advance();
                    let right = self.parse_not()?;
                    left = FilterExpr::and(left, right);
                }
                Token::Ident(_) | Token::String(_) | Token::LParen | Token::Not => {
                    // Implicit AND
                    let right = self.parse_not()?;
                    left = FilterExpr::and(left, right);
                }
                _ => break,
            }
        }

        Ok(left)
    }

    fn parse_not(&mut self) -> Result<FilterExpr, ParseError> {
        if *self.current() == Token::Not {
            self.advance();
            let expr = self.parse_primary()?;
            Ok(FilterExpr::not(expr))
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<FilterExpr, ParseError> {
        match self.current().clone() {
            Token::LParen => {
                self.advance();
                let expr = self.parse_or()?;
                if *self.current() != Token::RParen {
                    return Err(ParseError::UnclosedParenthesis {
                        position: self.position,
                    });
                }
                self.advance();
                Ok(expr)
            }
            Token::Ident(field) => {
                self.advance();
                self.parse_field_op(field)
            }
            Token::String(term) => {
                self.advance();
                Ok(FilterExpr::Term(term))
            }
            Token::Number(n) => {
                self.advance();
                Ok(FilterExpr::Term(n.to_string()))
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "identifier, string, or '('".to_string(),
                got: format!("{:?}", self.current()),
                position: self.position,
            }),
        }
    }

    fn parse_field_op(&mut self, field: String) -> Result<FilterExpr, ParseError> {
        match self.current() {
            Token::Colon => {
                self.advance();
                let value = self.parse_value()?;
                Ok(FilterExpr::FieldContains { field, value })
            }
            Token::Equals => {
                self.advance();
                let value = self.parse_value()?;
                Ok(FilterExpr::FieldEquals { field, value })
            }
            Token::Tilde => {
                self.advance();
                let pattern = self.parse_value()?;
                Ok(FilterExpr::FieldRegex { field, pattern })
            }
            Token::Greater | Token::Less | Token::GreaterEq | Token::LessEq | Token::NotEq => {
                let op = match self.current() {
                    Token::Greater => CompareOp::Greater,
                    Token::Less => CompareOp::Less,
                    Token::GreaterEq => CompareOp::GreaterEqual,
                    Token::LessEq => CompareOp::LessEqual,
                    Token::NotEq => CompareOp::NotEqual,
                    _ => unreachable!(),
                };
                self.advance();
                let value = self.parse_field_value()?;
                Ok(FilterExpr::FieldCompare { field, op, value })
            }
            _ => {
                // No operator, treat as a term
                Ok(FilterExpr::Term(field))
            }
        }
    }

    fn parse_value(&mut self) -> Result<String, ParseError> {
        match self.current().clone() {
            Token::String(s) => {
                self.advance();
                Ok(s)
            }
            Token::Ident(s) => {
                self.advance();
                Ok(s)
            }
            Token::Number(n) => {
                self.advance();
                Ok(n.to_string())
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "value".to_string(),
                got: format!("{:?}", self.current()),
                position: self.position,
            }),
        }
    }

    fn parse_field_value(&mut self) -> Result<FieldValue, ParseError> {
        match self.current().clone() {
            Token::String(s) => {
                self.advance();
                Ok(FieldValue::String(s))
            }
            Token::Ident(s) => {
                self.advance();
                match s.to_lowercase().as_str() {
                    "true" => Ok(FieldValue::Boolean(true)),
                    "false" => Ok(FieldValue::Boolean(false)),
                    _ => Ok(FieldValue::String(s)),
                }
            }
            Token::Number(n) => {
                self.advance();
                Ok(FieldValue::Number(n))
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "value".to_string(),
                got: format!("{:?}", self.current()),
                position: self.position,
            }),
        }
    }
}

/// Parse a filter expression string into an AST
pub fn parse_filter(input: &str) -> Result<FilterExpr, ParseError> {
    let mut parser = Parser::new(input)?;
    parser.parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_term() {
        let expr = parse_filter("error").unwrap();
        assert_eq!(expr, FilterExpr::Term("error".to_string()));
    }

    #[test]
    fn test_field_contains() {
        let expr = parse_filter("status:failed").unwrap();
        assert_eq!(
            expr,
            FilterExpr::FieldContains {
                field: "status".to_string(),
                value: "failed".to_string(),
            }
        );
    }

    #[test]
    fn test_field_equals() {
        let expr = parse_filter("status=error").unwrap();
        assert_eq!(
            expr,
            FilterExpr::FieldEquals {
                field: "status".to_string(),
                value: "error".to_string(),
            }
        );
    }

    #[test]
    fn test_field_compare() {
        let expr = parse_filter("priority>3").unwrap();
        match expr {
            FilterExpr::FieldCompare { field, op, value } => {
                assert_eq!(field, "priority");
                assert_eq!(op, CompareOp::Greater);
                assert_eq!(value, FieldValue::Number(3.0));
            }
            _ => panic!("Expected FieldCompare"),
        }
    }

    #[test]
    fn test_and_expression() {
        let expr = parse_filter("error AND warning").unwrap();
        match expr {
            FilterExpr::And(left, right) => {
                assert_eq!(*left, FilterExpr::Term("error".to_string()));
                assert_eq!(*right, FilterExpr::Term("warning".to_string()));
            }
            _ => panic!("Expected And"),
        }
    }

    #[test]
    fn test_or_expression() {
        let expr = parse_filter("status=error OR status=warning").unwrap();
        match expr {
            FilterExpr::Or(_, _) => (),
            _ => panic!("Expected Or"),
        }
    }

    #[test]
    fn test_not_expression() {
        let expr = parse_filter("NOT fixed").unwrap();
        match expr {
            FilterExpr::Not(inner) => {
                assert_eq!(*inner, FilterExpr::Term("fixed".to_string()));
            }
            _ => panic!("Expected Not"),
        }
    }

    #[test]
    fn test_complex_expression() {
        let expr = parse_filter("(status=error OR status=warning) AND priority>3").unwrap();
        match expr {
            FilterExpr::And(_, _) => (),
            _ => panic!("Expected And at root"),
        }
    }

    #[test]
    fn test_implicit_and() {
        let expr = parse_filter("error warning").unwrap();
        match expr {
            FilterExpr::And(left, right) => {
                assert_eq!(*left, FilterExpr::Term("error".to_string()));
                assert_eq!(*right, FilterExpr::Term("warning".to_string()));
            }
            _ => panic!("Expected implicit And"),
        }
    }

    #[test]
    fn test_empty_expression() {
        let result = parse_filter("");
        assert!(result.is_err());
    }

    #[test]
    fn test_unclosed_paren() {
        let result = parse_filter("(error");
        assert!(result.is_err());
    }
}

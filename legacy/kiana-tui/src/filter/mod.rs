/// Filter system for tables and lists
///
/// Provides expression parsing, evaluation engine, and preset management.
pub mod ast;
pub mod engine;
pub mod field;
pub mod parser;
pub mod preset;

pub use ast::{CompareOp, FieldValue, FilterExpr};
pub use engine::{FilterEngine, Filterable};
pub use field::{FieldDefinition, FieldRegistry, FieldType};
pub use parser::{parse_filter, ParseError};
pub use preset::{FilterPreset, PresetManager};

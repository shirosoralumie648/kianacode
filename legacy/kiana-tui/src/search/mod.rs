pub mod field_scorer;
pub mod fuzzy;
pub mod highlight;
pub mod multi_field;
pub mod preview;
pub mod regex;
pub mod score_aggregator;

pub use field_scorer::FieldScorer;
pub use fuzzy::calculate_match_score;
pub use highlight::{HighlightConfig, HighlightedLine, HighlightedSpan, RegexHighlighter};
pub use multi_field::{
    AggregationMethod, FieldMatch, MultiFieldResult, MultiFieldSearchEngine, SearchConfig,
    SearchField,
};
pub use preview::{LineMatch, PreviewConfig, PreviewLine, SearchPreview, SearchPreviewRenderer};
pub use regex::{Capture, RegexError, RegexMatch, RegexSearch};
pub use score_aggregator::ScoreAggregator;

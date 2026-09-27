//! SW-12 read-only Core facade for typed child results.

use kiana_domain::{validate_typed_child_result, TypedChildResult};

pub fn validate_child_result(result: &TypedChildResult) -> Result<(), &'static str> {
    validate_typed_child_result(result)
}

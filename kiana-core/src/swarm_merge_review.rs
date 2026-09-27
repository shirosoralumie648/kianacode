//! SW-14 Core read-only independent merge review facade.

use kiana_domain::{
    validate_swarm_merge_receipt, validate_swarm_merge_review, SwarmMergeReceipt, SwarmMergeReview,
};

pub fn validate_swarm_review(review: &SwarmMergeReview) -> Result<(), String> {
    validate_swarm_merge_review(review)
}

pub fn validate_swarm_receipt(receipt: &SwarmMergeReceipt) -> Result<(), String> {
    validate_swarm_merge_receipt(receipt)
}

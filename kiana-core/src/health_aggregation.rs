//! Read-only Core facade for conservative deployment health aggregation.

use kiana_domain::{HealthAggregationInput, HealthAggregationReport};

pub fn aggregate_health(input: &HealthAggregationInput) -> Result<HealthAggregationReport, String> {
    HealthAggregationReport::evaluate(input)
}

pub fn validate_health_aggregation(
    input: &HealthAggregationInput,
    report: &HealthAggregationReport,
) -> Result<(), String> {
    report.validate_against(input)
}

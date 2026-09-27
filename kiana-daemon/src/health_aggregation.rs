//! DaemonHost route for the Core health aggregation projection.

use kiana_domain::{HealthAggregationInput, HealthAggregationReport, HealthProbeKind};
use kiana_ports::PortError;

pub(crate) fn evaluate(
    input: &HealthAggregationInput,
) -> Result<HealthAggregationReport, PortError> {
    kiana_core::aggregate_health(input).map_err(PortError::Failed)
}

pub(crate) fn evaluate_probe(
    mut input: HealthAggregationInput,
    probe: HealthProbeKind,
) -> Result<HealthAggregationReport, PortError> {
    input.probe = probe;
    input.input_digest = input.digest();
    evaluate(&input)
}

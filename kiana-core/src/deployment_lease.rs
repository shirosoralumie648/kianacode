//! Read-only Core facade for the deployment operation lease CAS contract.

use kiana_domain::OperationLeaseCas;

pub fn validate_operation_lease_cas(cas: &OperationLeaseCas) -> Result<(), String> {
    cas.validate()
}

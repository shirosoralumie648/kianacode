//! Core read-only facade for the ER-31 replay-only fault matrix.

use kiana_domain::Er31FaultMatrix;

pub fn validate_er31_fault_matrix(matrix: &Er31FaultMatrix) -> Result<(), String> {
    matrix.validate()
}

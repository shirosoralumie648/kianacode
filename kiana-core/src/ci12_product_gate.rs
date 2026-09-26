//! Core read-only facade for CI-12 product-chain deny-first evidence.

use kiana_domain::Ci12ProductGate;

pub fn validate_ci12_product_gate(gate: &Ci12ProductGate) -> Result<(), String> {
    gate.validate()
}

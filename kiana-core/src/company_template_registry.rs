//! Core adapter for CompanyOS template/configuration governance.
//!
//! The adapter validates and records immutable template facts. It does not execute a process,
//! interpret prompt annotations, load packages or grant tools; the existing ControlPlane and
//! Broker remain the only effect and authority boundaries.

use kiana_domain::{
    ActiveTemplatePin, CompanyTemplateConfigProposal, CompanyTemplateRegistry,
    CompanyTemplateRollbackRecord, CompanyTemplateVersion,
};

pub(crate) fn install_company_template(
    registry: &mut CompanyTemplateRegistry,
    template: CompanyTemplateVersion,
) -> Result<(), &'static str> {
    registry.install_template(template)
}

pub(crate) fn propose_company_template_upgrade(
    registry: &mut CompanyTemplateRegistry,
    proposal: CompanyTemplateConfigProposal,
) -> Result<(), &'static str> {
    registry.propose_upgrade(proposal)
}

pub(crate) fn approve_company_template_upgrade(
    registry: &mut CompanyTemplateRegistry,
    proposal_id: &str,
    approval_ref: impl Into<String>,
    acceptance_ref: impl Into<String>,
) -> Result<(), &'static str> {
    registry.approve_upgrade(proposal_id, approval_ref, acceptance_ref)
}

pub(crate) fn migrate_company_process_template(
    registry: &mut CompanyTemplateRegistry,
    process_id: &str,
    proposal_id: &str,
) -> Result<ActiveTemplatePin, &'static str> {
    registry.migrate_process(process_id, proposal_id)
}

pub(crate) fn rollback_company_template(
    registry: &mut CompanyTemplateRegistry,
    record: CompanyTemplateRollbackRecord,
) -> Result<(), &'static str> {
    registry.rollback_template(record)
}

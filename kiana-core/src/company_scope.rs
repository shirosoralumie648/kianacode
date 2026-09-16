//! ControlPlane-side workspace scope guard for Company commands.
//!
//! Stable organization/project bindings live in `kiana-domain`; this module only validates the
//! request's workspace spelling before the existing Company EventStore stream is consulted.
//! It deliberately does not infer an OrganizationId or ProjectId from a path.

use std::path::{Component, Path};

pub(crate) fn validate_workspace_root(root: &str) -> Result<(), &'static str> {
    let path = Path::new(root.trim());
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("company_workspace_root_invalid");
    }
    Ok(())
}

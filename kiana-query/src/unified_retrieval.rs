//! Adapter exposing the domain unified retrieval algorithm through RetrievalPort.

use kiana_domain::{
    rank_retrieval, RetrievalItem, RetrievalProfile, RetrievalRequest, RetrievalResult,
};
use kiana_ports::{PortError, RetrievalPort};

#[derive(Clone, Debug)]
pub struct UnifiedRetrievalPort {
    profile: RetrievalProfile,
}

impl UnifiedRetrievalPort {
    pub fn new(profile: RetrievalProfile) -> Result<Self, String> {
        profile.validate()?;
        Ok(Self { profile })
    }

    pub fn profile(&self) -> &RetrievalProfile {
        &self.profile
    }
}

impl Default for UnifiedRetrievalPort {
    fn default() -> Self {
        Self {
            profile: RetrievalProfile::unified(),
        }
    }
}

impl RetrievalPort for UnifiedRetrievalPort {
    fn retrieve(
        &self,
        request: &RetrievalRequest,
        items: &[RetrievalItem],
        limit: usize,
    ) -> Result<RetrievalResult, PortError> {
        rank_retrieval(&self.profile, request, items, limit)
            .map_err(|error| PortError::Failed(format!("retrieval:{error}")))
    }
}

pub fn rank_unified(
    request: &RetrievalRequest,
    items: &[RetrievalItem],
    limit: usize,
) -> Result<RetrievalResult, String> {
    rank_retrieval(&RetrievalProfile::unified(), request, items, limit)
}

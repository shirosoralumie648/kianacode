//! SW-07 read-only Core facade for fresh child materialization.

use kiana_domain::{
    materialize_swarm_child, SwarmChildMaterializationReceipt, SwarmChildMaterializationRequest,
};

pub fn materialize_child(
    request: &SwarmChildMaterializationRequest,
) -> Result<SwarmChildMaterializationReceipt, &'static str> {
    materialize_swarm_child(request)
}

//! AUT-15 read-only Core facade for worker observation binding.

use kiana_domain::{
    validate_automation_observation, AutomationDispatchIntent, AutomationObservation,
};

pub fn observe_automation_dispatch(
    intent: &AutomationDispatchIntent,
    observation: &AutomationObservation,
) -> Result<AutomationDispatchIntent, &'static str> {
    validate_automation_observation(intent, observation)
}

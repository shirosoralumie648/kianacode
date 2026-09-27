//! AUT-16 read-only Core facade for retry classification.

use kiana_domain::{classify_automation_retry, AutomationRetryDecision, AutomationRetryInput};

pub fn classify_retry(
    input: &AutomationRetryInput,
) -> Result<AutomationRetryDecision, &'static str> {
    classify_automation_retry(input)
}

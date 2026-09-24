//! Provider-side UTC RPM/TPM reservation adapter.
//!
//! This is a bounded in-process accounting aid behind the existing `ProviderGateway`. It never
//! authorizes a model call or starts a request; the ControlPlane permit and the transport boundary
//! remain required. Aliases share the same `CapacityWindow` through `Connection` construction.

use kiana_domain::ProviderCapacityPolicy;
use std::sync::{Mutex, PoisonError};

const WINDOW_MS: u64 = 60_000;

#[derive(Clone, Copy, Debug, Default)]
struct WindowState {
    start_unix_ms: u64,
    end_unix_ms: u64,
    requests: u64,
    tokens: u64,
}

#[derive(Debug, Default)]
pub(crate) struct CapacityWindow {
    state: Mutex<WindowState>,
}

impl CapacityWindow {
    pub(crate) fn reserve(
        &self,
        policy: &ProviderCapacityPolicy,
        now_unix_ms: u64,
        requested_tokens: u64,
    ) -> Result<(), String> {
        policy.validate()?;
        if now_unix_ms == 0 || requested_tokens == 0 {
            return Err("provider_capacity_window_request_invalid".to_owned());
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.end_unix_ms != 0 && now_unix_ms < state.start_unix_ms {
            return Err("provider_capacity_clock_rollback".to_owned());
        }
        let start = (now_unix_ms / WINDOW_MS)
            .checked_mul(WINDOW_MS)
            .ok_or_else(|| "provider_capacity_window_overflow".to_owned())?;
        let end = start
            .checked_add(WINDOW_MS)
            .ok_or_else(|| "provider_capacity_window_overflow".to_owned())?;
        if state.end_unix_ms == 0 || now_unix_ms >= state.end_unix_ms {
            state = WindowState {
                start_unix_ms: start,
                end_unix_ms: end,
                requests: 0,
                tokens: 0,
            };
        }
        if state.requests >= policy.requests_per_minute
            || requested_tokens > policy.tokens_per_minute.saturating_sub(state.tokens)
        {
            return Err("provider_capacity_quota_exhausted".to_owned());
        }
        state.requests = state
            .requests
            .checked_add(1)
            .ok_or_else(|| "provider_capacity_requests_overflow".to_owned())?;
        state.tokens = state
            .tokens
            .checked_add(requested_tokens)
            .ok_or_else(|| "provider_capacity_tokens_overflow".to_owned())?;
        Ok(())
    }

    pub(crate) fn snapshot(&self) -> (u64, u64, u64, u64) {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        (
            state.start_unix_ms,
            state.end_unix_ms,
            state.requests,
            state.tokens,
        )
    }
}

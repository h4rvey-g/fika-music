use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use crate::source_runtime::SourceCancellationToken;

const MAX_PENDING_CANCELLATIONS: usize = 256;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SourceRequestRegistryError {
    #[error("source request registry lock was poisoned")]
    Poisoned,
    #[error("source request id is already active: {0}")]
    Duplicate(String),
}

#[derive(Default)]
struct RegistryState {
    active: BTreeMap<String, SourceCancellationToken>,
    pending_cancellations: VecDeque<String>,
}

#[derive(Default)]
pub(crate) struct SourceRequestRegistry {
    state: Mutex<RegistryState>,
}

impl SourceRequestRegistry {
    pub(crate) fn register(
        &self,
        request_id: Option<&str>,
    ) -> Result<SourceCancellationToken, SourceRequestRegistryError> {
        let cancellation = SourceCancellationToken::default();
        let Some(request_id) = normalized_request_id(request_id) else {
            return Ok(cancellation);
        };
        let mut state = self
            .state
            .lock()
            .map_err(|_| SourceRequestRegistryError::Poisoned)?;
        if state.active.contains_key(request_id) {
            return Err(SourceRequestRegistryError::Duplicate(request_id.to_owned()));
        }
        if let Some(index) = state
            .pending_cancellations
            .iter()
            .position(|pending| pending == request_id)
        {
            state.pending_cancellations.remove(index);
            cancellation.cancel();
        }
        state
            .active
            .insert(request_id.to_owned(), cancellation.clone());
        Ok(cancellation)
    }

    pub(crate) fn unregister(&self, request_id: Option<&str>) {
        let Some(request_id) = normalized_request_id(request_id) else {
            return;
        };
        if let Ok(mut state) = self.state.lock() {
            state.active.remove(request_id);
        }
    }

    pub(crate) fn cancel(&self, request_id: &str) -> Result<bool, SourceRequestRegistryError> {
        let request_id = request_id.trim();
        if request_id.is_empty() {
            return Ok(false);
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| SourceRequestRegistryError::Poisoned)?;
        if let Some(cancellation) = state.active.get(request_id) {
            cancellation.cancel();
            return Ok(true);
        }
        if !state
            .pending_cancellations
            .iter()
            .any(|pending| pending == request_id)
        {
            state.pending_cancellations.push_back(request_id.to_owned());
            while state.pending_cancellations.len() > MAX_PENDING_CANCELLATIONS {
                state.pending_cancellations.pop_front();
            }
        }
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.active.is_empty())
            .unwrap_or(false)
    }
}

fn normalized_request_id(request_id: Option<&str>) -> Option<&str> {
    request_id
        .map(str::trim)
        .filter(|request_id| !request_id.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_before_registration_is_applied_to_the_request() {
        let registry = SourceRequestRegistry::default();
        registry
            .cancel("request-1")
            .expect("cancellation should be recorded");

        let cancellation = registry
            .register(Some("request-1"))
            .expect("request should register");

        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn pending_cancellations_are_bounded() {
        let registry = SourceRequestRegistry::default();
        for index in 0..MAX_PENDING_CANCELLATIONS + 1 {
            registry
                .cancel(&format!("request-{index}"))
                .expect("cancellation should be recorded");
        }

        let oldest = registry
            .register(Some("request-0"))
            .expect("oldest request should register");

        assert!(!oldest.is_cancelled());
    }
}

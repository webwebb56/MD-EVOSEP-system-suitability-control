//! File finalization state machine.
//!
//! Tracks files through the finalization process:
//! DETECTED → STABILIZING → READY → PROCESSING → DONE

#![allow(dead_code)]

use crate::types::FinalizationState;

/// Finalizer tracks a single file through the finalization process.
#[derive(Debug)]
pub struct Finalizer {
    state: FinalizationState,
}

impl Finalizer {
    pub fn new() -> Self {
        Self {
            state: FinalizationState::Detected,
        }
    }

    pub fn state(&self) -> FinalizationState {
        self.state
    }

    pub fn transition_to(&mut self, new_state: FinalizationState) {
        self.state = new_state;
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            FinalizationState::Done | FinalizationState::Failed
        )
    }
}

impl Default for Finalizer {
    fn default() -> Self {
        Self::new()
    }
}

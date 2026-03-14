//! Per-model request counter with interior mutability.
//!
//! Kept in-memory only (Bedrock simulation is stateless w.r.t SQLite).

use std::collections::HashMap;
use std::sync::Mutex;

use super::config::BedrockSimConfig;

/// Thread-safe per-model request counters used by the scenario engine.
pub struct BedrockState {
    pub config: BedrockSimConfig,
    counters: Mutex<HashMap<String, u64>>,
}

impl BedrockState {
    pub fn new(config: BedrockSimConfig) -> Self {
        Self { config, counters: Mutex::new(HashMap::new()) }
    }

    /// Increment the counter for `model_id` and return the **new** count.
    pub fn increment(&self, model_id: &str) -> u64 {
        let mut lock = self.counters.lock().unwrap();
        let slot = lock.entry(model_id.to_string()).or_insert(0);
        *slot += 1;
        *slot
    }

    /// Return the current counter without modifying it.
    pub fn get_count(&self, model_id: &str) -> u64 {
        let lock = self.counters.lock().unwrap();
        lock.get(model_id).copied().unwrap_or(0)
    }

    /// Reset counters.  `None` resets all models; `Some(id)` resets one.
    pub fn reset(&self, model_id: Option<&str>) {
        let mut lock = self.counters.lock().unwrap();
        match model_id {
            None => lock.clear(),
            Some(id) => { lock.remove(id); }
        }
    }
}

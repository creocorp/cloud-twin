//! AWS Bedrock simulation module.
//!
//! Exposes a `BedrockState` that is stored in `AppState` and a `router()` function
//! that mounts the three Bedrock REST endpoints onto an axum Router.

mod config;
mod generator;
mod handlers;
mod scenario_engine;
mod state;
mod streaming;

pub use config::BedrockSimConfig;
pub use state::BedrockState;

use std::sync::Arc;
use axum::Router;
use crate::AppState;

/// Build the Bedrock REST router.
///
/// Mount **before** `aws::s3::router()` so that `/foundation-models` is
/// registered as a static path prior to S3's `/:bucket` wildcard.
pub fn router() -> Router<Arc<AppState>> {
    handlers::router()
}

//! Azure Service Bus feature module.

mod handlers;
mod models;
mod service;

use std::sync::Arc;
use axum::Router;
use crate::AppState;

/// Build the router mounted under `/azure` for Service Bus endpoints.
pub fn router() -> Router<Arc<AppState>> {
    handlers::router()
}

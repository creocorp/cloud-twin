//! Azure Service Bus feature module.

mod handlers;
mod models;
mod service;

use crate::AppState;
use axum::Router;
use std::sync::Arc;

/// Build the router mounted under `/azure` for Service Bus endpoints.
pub fn router() -> Router<Arc<AppState>> {
    handlers::router()
}

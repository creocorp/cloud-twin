//! GCP Pub/Sub feature module.

mod handlers;
mod models;
pub mod schema;
mod service;
#[cfg(test)]
mod tests;

use crate::AppState;
use axum::Router;
use std::sync::Arc;

/// Build the router mounted under `/gcp` for Pub/Sub endpoints.
pub fn router() -> Router<Arc<AppState>> {
    handlers::router()
}

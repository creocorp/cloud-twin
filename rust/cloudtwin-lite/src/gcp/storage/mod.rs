//! GCP Cloud Storage feature module.

mod handlers;
mod models;
mod service;

use std::sync::Arc;
use axum::Router;
use crate::AppState;

/// Build the router mounted under `/gcp` for Cloud Storage endpoints.
pub fn router() -> Router<Arc<AppState>> {
    handlers::router()
}

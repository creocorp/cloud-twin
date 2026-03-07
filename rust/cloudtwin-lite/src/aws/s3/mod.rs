//! S3 feature module mounted under the AWS provider.

mod handlers;
mod models;
mod service;

use std::sync::Arc;
use axum::Router;
use crate::AppState;

/// Build the REST router for S3-style bucket and object routes.
pub fn router() -> Router<Arc<AppState>> {
    handlers::router()
}

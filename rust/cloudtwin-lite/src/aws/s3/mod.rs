//! S3 feature module mounted under the AWS provider.

mod handlers;
mod models;
mod service;

use crate::AppState;
use axum::Router;
use std::sync::Arc;

/// Build the REST router for S3-style bucket and object routes.
pub fn router() -> Router<Arc<AppState>> {
    handlers::router()
}

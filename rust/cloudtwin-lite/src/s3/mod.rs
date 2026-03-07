//! Legacy top-level S3 module.
//!
//! This predates the move to `aws::s3`. It is not the router mounted by the
//! current `main.rs`, but it is kept here as source history during the refactor.

mod handlers;
mod models;
mod service;

use std::sync::Arc;
use axum::Router;
use crate::AppState;

/// Build the older standalone S3 router.
pub fn router() -> Router<Arc<AppState>> {
    handlers::router()
}

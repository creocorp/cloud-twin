//! Azure provider namespace and mount point.
//!
//! This module is intentionally thin: it just groups Azure services and returns
//! a single router mounted at `/azure` by `main.rs`.

pub mod blob;
pub mod servicebus;

use crate::AppState;
use axum::Router;
use std::sync::Arc;

/// Returns a router containing all Azure services, to be mounted at `/azure`.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .merge(blob::router())
        .merge(servicebus::router())
}

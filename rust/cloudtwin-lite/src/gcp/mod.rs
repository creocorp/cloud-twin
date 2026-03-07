//! GCP provider namespace and mount point.
//!
//! Like `azure::mod`, this is mostly composition glue: each service builds its
//! own router and this module merges them into one `/gcp` subtree.

pub mod pubsub;
pub mod storage;

use crate::AppState;
use axum::Router;
use std::sync::Arc;

/// Returns a router containing all GCP services, to be mounted at `/gcp`.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .merge(storage::router())
        .merge(pubsub::router())
}

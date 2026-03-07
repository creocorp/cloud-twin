pub mod pubsub;
pub mod storage;

use std::sync::Arc;
use axum::Router;
use crate::AppState;

/// Returns a router containing all GCP services, to be mounted at `/gcp`.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .merge(storage::router())
        .merge(pubsub::router())
}

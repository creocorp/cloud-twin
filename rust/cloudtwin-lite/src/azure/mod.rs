pub mod blob;
pub mod servicebus;

use std::sync::Arc;
use axum::Router;
use crate::AppState;

/// Returns a router containing all Azure services, to be mounted at `/azure`.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .merge(blob::router())
        .merge(servicebus::router())
}

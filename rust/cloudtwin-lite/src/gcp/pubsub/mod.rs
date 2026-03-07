mod handlers;
mod models;
mod service;

use std::sync::Arc;
use axum::Router;
use crate::AppState;

pub fn router() -> Router<Arc<AppState>> {
    handlers::router()
}

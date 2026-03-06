mod config;
mod db;
mod s3;

use std::sync::Arc;

use anyhow::Result;
use axum::{routing::get, Json, Router};
use serde_json::json;
use tracing::info;
use tracing_subscriber::EnvFilter;

use config::Config;
use db::Database;

/// Shared application state injected into every handler.
pub struct AppState {
    pub db: Database,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cfg = Config::from_env();

    let db = Database::open(&cfg.db_path).await?;
    db.migrate().await?;

    let state = Arc::new(AppState { db });

    let app = Router::new()
        .route("/_health", get(health))
        .merge(s3::router(state));

    let addr = format!("0.0.0.0:{}", cfg.port);
    info!("CloudTwin Lite listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "service": "cloudtwin-lite" }))
}

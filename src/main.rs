//! Crate entry point for CloudTwin.
//!
//! If you are coming from C# or Python, this file plays the role of a very small
//! `Program.cs`. It loads config, builds shared state and the router (see
//! `app.rs`), applies any startup seed, and starts the HTTP and SMTP listeners.

mod app;
mod aws;
mod azure;
mod config;
mod control;
mod dashboard;
mod db;
mod gcp;
mod mailbox;
mod proto;
mod seed;
mod telemetry;
#[cfg(test)]
mod testutil;
mod xml;

use anyhow::Result;
use tracing::info;
use tracing_subscriber::EnvFilter;

pub use app::AppState;
use config::Config;

#[tokio::main]
async fn main() -> Result<()> {
    // Configure structured logging first so startup failures are visible.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Resolve process configuration once at startup — reads cloudtwin.yml then
    // overlays any CLOUDTWIN_* env vars.
    let cfg = Config::load();

    // Open SQLite and ensure every table exists before we accept traffic.
    let state = AppState::new(cfg.clone()).await?;

    if let Some(doc) = &cfg.seed {
        match seed::apply(&state, doc).await {
            Ok(()) => info!("Applied startup seed"),
            Err(e) => tracing::error!("Startup seed failed: {e:#}"),
        }
    }

    // Spawn the standalone SMTP server (capture mailbox + SES receiving). It
    // runs independently of the HTTP server.
    if cfg.mailbox_enabled {
        let smtp_addr = format!("0.0.0.0:{}", cfg.smtp_port);
        tokio::spawn(mailbox::smtp::serve(state.clone(), smtp_addr));
    }

    let app = app::build_router(state);

    let addr = format!("0.0.0.0:{}", cfg.port);
    info!("CloudTwin listening on http://{addr}");
    info!("  AWS   → http://{addr}/          (S3, SES, SNS, SQS, DynamoDB, SecretsManager, KMS, Cognito)");
    info!("  Azure → http://{addr}/azure/    (Blob Storage, Service Bus)");
    info!("  GCP   → http://{addr}/gcp/      (Cloud Storage, Pub/Sub)");
    if cfg.mailbox_enabled {
        info!(
            "  Mail  → smtp://0.0.0.0:{}   (SMTP capture mailbox + SES receiving)",
            cfg.smtp_port
        );
    }

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

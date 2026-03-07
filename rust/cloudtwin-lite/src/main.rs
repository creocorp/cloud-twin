//! Crate entry point for CloudTwin Lite.
//!
//! If you are coming from C# or Python, this file plays the role of a very small
//! `Program.cs` plus a bit of `Startup` / FastAPI app wiring. It assembles the
//! shared state, mounts provider routers, and exposes the one AWS front door at
//! `/` that fan-outs into protocol-specific handlers.

mod aws;
mod azure;
mod config;
mod db;
mod gcp;
mod proto;

use std::sync::Arc;

use anyhow::Result;
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use tracing::info;
use tracing_subscriber::EnvFilter;

use config::Config;
use db::Database;

/// Shared application state injected into every handler.
///
/// In C# terms this is close to a scoped dependency bag that controllers can
/// resolve from DI. In Python terms it is similar to an app context object that
/// handlers receive explicitly.
pub struct AppState {
    pub db: Database,
    pub cfg: Config,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Configure structured logging first so startup failures are visible.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Resolve process configuration once at startup and clone cheap values into
    // the shared state. Rust makes that explicit instead of hiding it behind a
    // global singleton.
    let cfg = Config::from_env();

    // Open SQLite and ensure every table exists before we accept traffic.
    let db = Database::open(&cfg.db_path).await?;
    db.migrate().await?;

    let state = Arc::new(AppState {
        db,
        cfg: cfg.clone(),
    });

    // Build the router:
    // – Sub-routers that already consumed state via with_state() (Router<()>)
    //   are merged with .merge(); axum converts Router<()> → Router<S> automatically.
    // – azure:: and gcp:: routers do NOT call with_state() internally, so they
    //   return Router<Arc<AppState>> and can be used with .nest().
    // – A single .with_state(state) at the end satisfies all remaining extractors.
    let app = Router::new()
        .route("/_health", get(health))
        // AWS single-endpoint dispatcher  (SES v1/SNS query-protocol;
        //                                  SQS/DynamoDB/SecretsManager JSON-protocol)
        .route("/", post(aws_post))
        // SES v2 REST   (/v2/email/...)
        .merge(aws::ses::router_v2())
        // S3 REST        (/:bucket  and  /:bucket/*key)
        .merge(aws::s3::router())
        // Azure services (/azure/...)
        .nest("/azure", azure::router())
        // GCP services   (/gcp/...)
        .nest("/gcp", gcp::router())
        // Provide shared state to every handler that uses it via State<Arc<AppState>>
        .with_state(state);

    let addr = format!("0.0.0.0:{}", cfg.port);
    info!("CloudTwin Lite listening on http://{addr}");
    info!("  AWS   → http://{addr}/          (S3, SES, SNS, SQS, DynamoDB, SecretsManager)");
    info!("  Azure → http://{addr}/azure/    (Blob Storage, Service Bus)");
    info!("  GCP   → http://{addr}/gcp/      (Cloud Storage, Pub/Sub)");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    // Keep the health payload intentionally tiny so probes stay cheap.
    Json(json!({
        "status":  "ok",
        "service": "cloudtwin-lite",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// `POST /` — routes all AWS protocol requests to the correct service handler.
///
/// * `Content-Type: application/x-amz-json-1.0` + `X-Amz-Target` header
///   → JSON protocol (SQS, DynamoDB, SecretsManager)
/// * anything else (form-urlencoded) → Query protocol (SES v1, SNS)
async fn aws_post(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    // AWS is the only provider here that multiplexes many services through one
    // HTTP endpoint. `proto::AwsPayload` is the thin decoder that tells us
    // whether this request is AWS Query or AWS JSON protocol.
    match proto::AwsPayload::parse(&headers, &body) {
        proto::AwsPayload::Json { target, body } => {
            if target.starts_with("AmazonSQS.") {
                aws::sqs::handle_json(&state, &target, body).await
            } else if target.starts_with("DynamoDB_") {
                aws::dynamodb::handle_json(&state, &target, body).await
            } else if target.starts_with("secretsmanager.") {
                aws::secretsmanager::handle_json(&state, &target, body).await
            } else {
                proto::json_error_response(
                    StatusCode::BAD_REQUEST,
                    "InvalidAction",
                    &format!("Unknown X-Amz-Target: {target}"),
                )
            }
        }
        proto::AwsPayload::Query(params) => {
            let action = params
                .get("Action")
                .map(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            if aws::ses::QUERY_ACTIONS.contains(&action.as_str()) {
                aws::ses::handle_query(&state, &action, &params).await
            } else if aws::sns::QUERY_ACTIONS.contains(&action.as_str()) {
                aws::sns::handle_query(&state, &action, &params).await
            } else {
                proto::xml_error_response(
                    StatusCode::BAD_REQUEST,
                    "InvalidAction",
                    &format!("Unknown Action: {action}"),
                )
            }
        }
    }
}

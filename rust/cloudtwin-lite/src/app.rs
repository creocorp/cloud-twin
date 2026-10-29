//! Application assembly: shared state, router, and the AWS `POST /` dispatcher.
//!
//! `main.rs` only loads config and serves; everything needed to build a working
//! app lives here so in-process tests use exactly the same wiring.

use std::sync::Arc;

use anyhow::Result;
use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, State},
    http::{header, HeaderMap, Request, StatusCode, Uri},
    response::{Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use tracing::info;

use crate::config::Config;
use crate::db::Database;
use crate::{aws, azure, control, dashboard, gcp, proto};

/// Shared application state injected into every handler.
///
/// In C# terms this is close to a scoped dependency bag that controllers can
/// resolve from DI. In Python terms it is similar to an app context object that
/// handlers receive explicitly.
pub struct AppState {
    pub db: Database,
    pub cfg: Config,
    pub bedrock: aws::bedrock::BedrockState,
    /// Outbound HTTP client (SNS HTTP/S subscriptions).
    pub http: reqwest::Client,
}

impl AppState {
    /// Open and migrate the database, then build state from `cfg`.
    pub async fn new(cfg: Config) -> Result<Arc<Self>> {
        let db = Database::open(&cfg.db_path).await?;
        db.migrate().await?;
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        Ok(Arc::new(AppState {
            db,
            bedrock: aws::bedrock::BedrockState::new(cfg.bedrock.clone()),
            cfg,
            http,
        }))
    }
}

/// Build the full HTTP application for `state`.
pub fn build_router(state: Arc<AppState>) -> Router {
    // Static routes always win over S3's `/:bucket` params in axum's matcher, so
    // registration order only matters for readability here.
    let mut app = Router::new()
        .route("/_health", get(health))
        // AWS single-endpoint dispatcher (Query and JSON protocols)
        .route("/", post(aws_post))
        // Test control plane (/_cloudtwin/reset, /_cloudtwin/seed, ...)
        .nest("/_cloudtwin", control::router())
        // SES v2 REST   (/v2/email/...)
        .merge(aws::ses::router_v2())
        // Cognito OIDC + hosted UI (/{userPoolId}/.well-known/..., /oauth2/...)
        .merge(aws::cognito::router())
        // Bedrock REST   (/foundation-models  and  /model/:id/invoke*)
        .merge(aws::bedrock::router())
        // S3 REST        (/:bucket  and  /:bucket/*key)
        .merge(aws::s3::router())
        // Azure services (/azure/...)
        .nest("/azure", azure::router())
        // GCP services   (/gcp/...)
        .nest("/gcp", gcp::router())
        // Dashboard API  (/api/dashboard/...)
        .nest("/api/dashboard", dashboard::router());

    if let Some(static_dir) = &state.cfg.dashboard_static_path {
        use tower_http::services::{ServeDir, ServeFile};
        let index = format!("{static_dir}/index.html");
        app = app
            .route(
                "/dashboard",
                get(|| async { Redirect::permanent("/dashboard/static/index.html") }),
            )
            .route(
                "/dashboard/",
                get(|| async { Redirect::permanent("/dashboard/static/index.html") }),
            )
            .nest_service(
                "/dashboard/static",
                ServeDir::new(static_dir).not_found_service(ServeFile::new(index)),
            );
        info!(
            "Dashboard enabled — open http://localhost:{}/dashboard",
            state.cfg.port
        );
    }

    let suffix = state.cfg.s3.virtual_host_suffix.to_ascii_lowercase();
    // S3 objects and multipart parts routinely exceed axum's 2 MB default.
    let inner = app
        .layer(DefaultBodyLimit::disable())
        .with_state(state);

    // Virtual-hosted-style S3 must be rewritten *before* routing, so the router
    // is wrapped as a fallback service rather than given a `.layer()`.
    let rewritten = tower::util::MapRequest::new(inner, move |req: Request<Body>| {
        rewrite_virtual_host(req, &suffix)
    });
    Router::new().fallback_service(rewritten)
}

/// Rewrite `{bucket}.{suffix}/key` requests to path style `/{bucket}/key`.
fn rewrite_virtual_host(mut req: Request<Body>, suffix: &str) -> Request<Body> {
    if suffix.is_empty() {
        return req;
    }
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|h| h.to_string())
        .or_else(|| req.uri().host().map(|h| h.to_string()));
    let Some(host) = host else { return req };
    let hostname = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(&host);
    let hostname = hostname.to_ascii_lowercase();
    let Some(bucket) = hostname.strip_suffix(&format!(".{suffix}")) else {
        return req;
    };
    if bucket.is_empty() {
        return req;
    }
    let pq = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "/".into());
    let new = if pq == "/" || pq.starts_with("/?") {
        format!("/{bucket}{}", &pq[1..])
    } else {
        format!("/{bucket}{pq}")
    };
    if let Ok(uri) = new.parse::<Uri>() {
        *req.uri_mut() = uri;
    }
    req
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "status":  "ok",
        "service": "cloudtwin-lite",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// `POST /` — routes all AWS protocol requests to the correct service handler.
///
/// * `Content-Type: application/x-amz-json-*` + `X-Amz-Target` header
///   → JSON protocol (SQS, DynamoDB, Secrets Manager, KMS, Cognito)
/// * anything else (form-urlencoded) → Query protocol (SES v1, SNS)
async fn aws_post(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    match proto::AwsPayload::parse(&headers, &body) {
        proto::AwsPayload::Json { target, body } => {
            if target.starts_with("AmazonSQS.") {
                aws::sqs::handle_json(&state, &target, body).await
            } else if target.starts_with("DynamoDB_") {
                aws::dynamodb::handle_json(&state, &target, body).await
            } else if target.starts_with("secretsmanager.") {
                aws::secretsmanager::handle_json(&state, &target, body).await
            } else if target.starts_with("TrentService.") {
                aws::kms::handle_json(&state, &target, body).await
            } else if target.starts_with("AWSCognitoIdentityProviderService.") {
                aws::cognito::handle_json(&state, &target, body).await
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rewrite(host: &str, uri: &str) -> String {
        let req = Request::builder()
            .uri(uri)
            .header(header::HOST, host)
            .body(Body::empty())
            .unwrap();
        rewrite_virtual_host(req, "s3.localhost").uri().to_string()
    }

    #[test]
    fn virtual_host_requests_are_rewritten_to_path_style() {
        assert_eq!(rewrite("mail.s3.localhost:4793", "/a/b.txt"), "/mail/a/b.txt");
        assert_eq!(rewrite("my.bucket.s3.localhost", "/?list-type=2"), "/my.bucket?list-type=2");
        assert_eq!(rewrite("mail.s3.localhost", "/"), "/mail");
        assert_eq!(rewrite("localhost:4793", "/mail/a"), "/mail/a");
        assert_eq!(rewrite("s3.localhost", "/mail/a"), "/mail/a");
    }
}

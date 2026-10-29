//! Test-control endpoints under `/_cloudtwin`.
//!
//! These are CloudTwin-specific (not AWS APIs). Integration suites call
//! `POST /_cloudtwin/reset` between tests to start from a clean state and
//! `POST /_cloudtwin/seed` to provision fixtures.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};

use crate::{aws, seed, AppState};

/// Table-name prefixes owned by each resettable service.
const SERVICE_TABLES: &[(&str, &[&str])] = &[
    ("s3", &["s3_"]),
    ("ses", &["ses_"]),
    ("sqs", &["sqs_"]),
    ("sns", &["sns_"]),
    ("kms", &["kms_"]),
    ("cognito", &["cognito_"]),
    ("dynamodb", &["dynamo_"]),
    ("secretsmanager", &["secrets"]),
    ("mailbox", &["mailbox_"]),
    ("events", &["events"]),
    ("azure", &["azure_", "asb_"]),
    ("gcp", &["gcs_", "pubsub_"]),
];

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/reset", post(reset))
        .route("/seed", post(seed_handler))
        .nest("/ses", aws::ses::control_router())
}

fn parse_body(body: &Bytes) -> Value {
    if body.is_empty() {
        return json!({});
    }
    serde_json::from_slice(body).unwrap_or_else(|_| json!({}))
}

/// `POST /_cloudtwin/reset` — body (optional):
/// `{"services": ["ses", "s3", "sqs"], "reseed": true}`.
///
/// Omitting `services` clears every service. With `reseed` (default `true`)
/// the startup seed document is re-applied afterwards.
async fn reset(State(state): State<Arc<AppState>>, body: Bytes) -> Response {
    let req = parse_body(&body);
    let services: Option<Vec<String>> = req.get("services").and_then(|v| v.as_array()).map(|a| {
        a.iter()
            .filter_map(|s| s.as_str().map(|s| s.to_ascii_lowercase()))
            .collect()
    });
    if let Some(list) = &services {
        if let Some(bad) = list
            .iter()
            .find(|s| !SERVICE_TABLES.iter().any(|(name, _)| name == s))
        {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"message": format!("Unknown service: {bad}")})),
            )
                .into_response();
        }
    }
    let prefixes: Vec<&'static str> = SERVICE_TABLES
        .iter()
        .filter(|(name, _)| services.as_ref().is_none_or(|l| l.iter().any(|s| s == name)))
        .flat_map(|(_, p)| p.iter().copied())
        .collect();

    let cleared = state
        .db
        .call(move |conn| {
            let tables: Vec<String> = {
                let mut stmt = conn.prepare(
                    "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
                )?;
                let rows = stmt
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows
            };
            let tx = conn.transaction()?;
            let mut cleared = Vec::new();
            for t in tables {
                if t != "cloudtwin_meta" && prefixes.iter().any(|p| t.starts_with(p)) {
                    tx.execute(&format!("DELETE FROM \"{t}\""), [])?;
                    cleared.push(t);
                }
            }
            tx.commit()?;
            Ok(cleared)
        })
        .await;

    let cleared = match cleared {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"message": e.to_string()})),
            )
                .into_response()
        }
    };

    let reseed = req.get("reseed").and_then(|v| v.as_bool()).unwrap_or(true);
    if reseed {
        if let Some(doc) = state.cfg.seed.clone() {
            if let Err(e) = seed::apply(&state, &doc).await {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"message": format!("reseed failed: {e}")})),
                )
                    .into_response();
            }
        }
    }
    Json(json!({"cleared": cleared, "reseeded": reseed && state.cfg.seed.is_some()})).into_response()
}

/// `POST /_cloudtwin/seed` — apply a seed document (same shape as the YAML
/// `seed:` block). Seeding is idempotent.
async fn seed_handler(State(state): State<Arc<AppState>>, body: Bytes) -> Response {
    let doc = parse_body(&body);
    match seed::apply(&state, &doc).await {
        Ok(()) => Json(json!({"seeded": true})).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"message": e.to_string()})),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use crate::testutil;
    use serde_json::json;

    #[tokio::test]
    async fn reset_clears_selected_services_only() {
        let t = testutil::TestApp::new().await;
        t.call(testutil::rest("PUT", "/keep-bucket", None)).await;
        t.call(testutil::aws_json("AmazonSQS.CreateQueue", json!({"QueueName": "q1"})))
            .await;

        let r = t
            .call(testutil::rest("POST", "/_cloudtwin/reset", Some(json!({"services": ["sqs"]}))))
            .await;
        assert_eq!(r.status, 200, "{}", r.text());

        let queues = t
            .call(testutil::aws_json("AmazonSQS.ListQueues", json!({})))
            .await
            .json();
        assert!(queues["QueueUrls"].as_array().map(|a| a.is_empty()).unwrap_or(true));
        let head = t.call(testutil::rest("HEAD", "/keep-bucket", None)).await;
        assert_eq!(head.status, 200);

        let bad = t
            .call(testutil::rest("POST", "/_cloudtwin/reset", Some(json!({"services": ["nope"]}))))
            .await;
        assert_eq!(bad.status, 400);
    }

    #[tokio::test]
    async fn seed_endpoint_provisions_resources() {
        let t = testutil::TestApp::new().await;
        let r = t
            .call(testutil::rest(
                "POST",
                "/_cloudtwin/seed",
                Some(json!({"s3": {"buckets": ["seeded"]}, "sqs": {"queues": ["seeded-q"]}})),
            ))
            .await;
        assert_eq!(r.status, 200, "{}", r.text());
        assert_eq!(t.call(testutil::rest("HEAD", "/seeded", None)).await.status, 200);
        let url = t
            .call(testutil::aws_json("AmazonSQS.GetQueueUrl", json!({"QueueName": "seeded-q"})))
            .await;
        assert_eq!(url.status, 200);
    }
}

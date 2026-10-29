//! GCP Pub/Sub HTTP handlers (Pub/Sub REST API v1).
//!
//! Routes match what the google-cloud-pubsub REST transports send (mounted at
//! `/gcp`; `app.rs` maps root `/v1/projects/...` paths onto them):
//!
//! ```text
//! PUT    /v1/projects/{p}/topics/{t}                     create topic
//! GET    /v1/projects/{p}/topics/{t}                     get topic
//! DELETE /v1/projects/{p}/topics/{t}                     delete topic
//! GET    /v1/projects/{p}/topics                         list topics
//! GET    /v1/projects/{p}/topics/{t}/subscriptions       list topic subscriptions
//! POST   /v1/projects/{p}/topics/{t}:publish             publish
//! PUT    /v1/projects/{p}/subscriptions/{s}              create subscription
//! GET    /v1/projects/{p}/subscriptions/{s}              get subscription
//! DELETE /v1/projects/{p}/subscriptions/{s}              delete subscription
//! GET    /v1/projects/{p}/subscriptions                  list subscriptions
//! POST   /v1/projects/{p}/subscriptions/{s}:pull         pull
//! POST   /v1/projects/{p}/subscriptions/{s}:acknowledge  acknowledge
//! POST   /v1/projects/{p}/subscriptions/{s}:modifyAckDeadline
//! ```

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, put},
    Json, Router,
};
use serde_json::{json, Value};

use super::models::{rfc3339_z, PubsubSubscription, PubsubTopic};
use super::service::{OutgoingMessage, PubsubError, PubsubService};
use crate::telemetry;
use crate::AppState;

fn svc(state: &Arc<AppState>) -> PubsubService {
    PubsubService::new(state.db.clone(), state.cfg.gcp_project.clone())
}

fn topic_path(project: &str, topic: &str) -> String {
    format!("projects/{project}/topics/{topic}")
}

fn sub_path(project: &str, sub: &str) -> String {
    format!("projects/{project}/subscriptions/{sub}")
}

/// Router is mounted at `/gcp` in main.
///
/// GCP uses the `:action` suffix convention (e.g. `/topics/my-topic:publish`).
/// Axum cannot express a parameter and a literal in one segment, so a single
/// POST handler per resource type dispatches on the suffix.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // Topics
        .route("/v1/projects/:project/topics", get(list_topics))
        .route(
            "/v1/projects/:project/topics/:topic",
            put(create_topic)
                .get(get_topic)
                .delete(delete_topic)
                .post(topic_action),
        )
        .route(
            "/v1/projects/:project/topics/:topic/subscriptions",
            get(list_topic_subscriptions),
        )
        // Subscriptions
        .route(
            "/v1/projects/:project/subscriptions",
            get(list_subscriptions),
        )
        .route(
            "/v1/projects/:project/subscriptions/:sub",
            put(create_subscription)
                .get(get_subscription)
                .delete(delete_subscription)
                .post(subscription_action),
        )
}

// ── Response helpers ──────────────────────────────────────────────────────────

/// Google RPC-style JSON error envelope.
fn rpc_error(status: StatusCode, rpc_status: &str, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({
            "error": { "code": status.as_u16(), "message": message.into(), "status": rpc_status }
        })),
    )
        .into_response()
}

fn error_response(e: PubsubError) -> Response {
    match e {
        PubsubError::NotFound(m) => rpc_error(StatusCode::NOT_FOUND, "NOT_FOUND", m),
        PubsubError::AlreadyExists(m) => rpc_error(StatusCode::CONFLICT, "ALREADY_EXISTS", m),
        PubsubError::InvalidArgument(m) => {
            rpc_error(StatusCode::BAD_REQUEST, "INVALID_ARGUMENT", m)
        }
        PubsubError::Internal(m) => rpc_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", m),
    }
}

fn empty_ok() -> Response {
    Json(json!({})).into_response()
}

fn parse_body(body: &[u8]) -> Value {
    serde_json::from_slice(body).unwrap_or_else(|_| json!({}))
}

fn topic_to_json(t: &PubsubTopic) -> Value {
    json!({ "name": t.name })
}

fn sub_to_json(s: &PubsubSubscription) -> Value {
    json!({
        "name": s.name,
        "topic": s.topic,
        "pushConfig": {},
        "ackDeadlineSeconds": s.ack_deadline_seconds,
        "messageRetentionDuration": "604800s",
    })
}

fn ack_ids(body: &Value) -> Vec<String> {
    body.get("ackIds")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Protobuf JSON encodes int32 as a number but also accepts strings.
fn json_i64(v: Option<&Value>) -> Option<i64> {
    v.and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    })
}

// ── Action dispatchers ────────────────────────────────────────────────────────

/// Dispatch `POST /v1/projects/{p}/topics/{topic}:{action}`.
async fn topic_action(
    State(state): State<Arc<AppState>>,
    Path((project, segment)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    match segment.rsplit_once(':') {
        Some((topic, "publish")) => {
            publish(&state, &topic_path(&project, topic), parse_body(&body)).await
        }
        _ => rpc_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            format!("Unknown topic method: {segment}"),
        ),
    }
}

/// Dispatch `POST /v1/projects/{p}/subscriptions/{sub}:{action}`.
async fn subscription_action(
    State(state): State<Arc<AppState>>,
    Path((project, segment)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let body = parse_body(&body);
    let Some((sub, action)) = segment.rsplit_once(':') else {
        return rpc_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            format!("Unknown subscription method: {segment}"),
        );
    };
    let sub = sub_path(&project, sub);
    match action {
        "pull" => pull(&state, &sub, body).await,
        "acknowledge" => match svc(&state).acknowledge(&sub, ack_ids(&body)).await {
            Ok(()) => {
                telemetry::emit(
                    &state.db,
                    "gcp",
                    "pubsub",
                    "acknowledge",
                    &json!({"subscription": sub, "count": ack_ids(&body).len()}).to_string(),
                )
                .await;
                empty_ok()
            }
            Err(e) => error_response(e),
        },
        "modifyAckDeadline" => {
            let seconds = json_i64(body.get("ackDeadlineSeconds")).unwrap_or(0);
            match svc(&state)
                .modify_ack_deadline(&sub, ack_ids(&body), seconds)
                .await
            {
                Ok(()) => empty_ok(),
                Err(e) => error_response(e),
            }
        }
        _ => rpc_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            format!("Unknown subscription method: {action}"),
        ),
    }
}

// ── Topic handlers ────────────────────────────────────────────────────────────

async fn create_topic(
    State(state): State<Arc<AppState>>,
    Path((project, topic)): Path<(String, String)>,
) -> Response {
    let name = topic_path(&project, &topic);
    match svc(&state).create_topic(&name).await {
        Ok(t) => {
            telemetry::emit(
                &state.db,
                "gcp",
                "pubsub",
                "create_topic",
                &json!({"topic": name}).to_string(),
            )
            .await;
            Json(topic_to_json(&t)).into_response()
        }
        Err(e) => error_response(e),
    }
}

async fn get_topic(
    State(state): State<Arc<AppState>>,
    Path((project, topic)): Path<(String, String)>,
) -> Response {
    match svc(&state).get_topic(&topic_path(&project, &topic)).await {
        Ok(t) => Json(topic_to_json(&t)).into_response(),
        Err(e) => error_response(e),
    }
}

async fn list_topics(State(state): State<Arc<AppState>>, Path(project): Path<String>) -> Response {
    match svc(&state).list_topics(Some(&project)).await {
        Ok(ts) => {
            let items: Vec<_> = ts.iter().map(topic_to_json).collect();
            Json(json!({ "topics": items })).into_response()
        }
        Err(e) => error_response(e),
    }
}

async fn delete_topic(
    State(state): State<Arc<AppState>>,
    Path((project, topic)): Path<(String, String)>,
) -> Response {
    let name = topic_path(&project, &topic);
    match svc(&state).delete_topic(&name).await {
        Ok(()) => {
            telemetry::emit(
                &state.db,
                "gcp",
                "pubsub",
                "delete_topic",
                &json!({"topic": name}).to_string(),
            )
            .await;
            empty_ok()
        }
        Err(e) => error_response(e),
    }
}

async fn list_topic_subscriptions(
    State(state): State<Arc<AppState>>,
    Path((project, topic)): Path<(String, String)>,
) -> Response {
    match svc(&state)
        .topic_subscriptions(&topic_path(&project, &topic))
        .await
    {
        Ok(subs) => Json(json!({ "subscriptions": subs })).into_response(),
        Err(e) => error_response(e),
    }
}

// ── Publish ───────────────────────────────────────────────────────────────────

async fn publish(state: &Arc<AppState>, topic: &str, body: Value) -> Response {
    let msgs: Vec<OutgoingMessage> = body
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|m| OutgoingMessage {
                    data: m
                        .get("data")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    attributes: m
                        .get("attributes")
                        .filter(|v| v.is_object())
                        .cloned()
                        .unwrap_or_else(|| json!({})),
                })
                .collect()
        })
        .unwrap_or_default();
    let count = msgs.len();
    match svc(state).publish(topic, msgs).await {
        Ok(ids) => {
            telemetry::emit(
                &state.db,
                "gcp",
                "pubsub",
                "publish",
                &json!({"topic": topic, "count": count}).to_string(),
            )
            .await;
            Json(json!({ "messageIds": ids })).into_response()
        }
        Err(e) => error_response(e),
    }
}

// ── Subscription handlers ─────────────────────────────────────────────────────

async fn create_subscription(
    State(state): State<Arc<AppState>>,
    Path((project, sub)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let body = parse_body(&body);
    let name = sub_path(&project, &sub);
    let topic = body.get("topic").and_then(|v| v.as_str()).unwrap_or("");
    let deadline = json_i64(body.get("ackDeadlineSeconds"));
    match svc(&state)
        .create_subscription(&name, topic, deadline)
        .await
    {
        Ok(s) => {
            telemetry::emit(
                &state.db,
                "gcp",
                "pubsub",
                "create_subscription",
                &json!({"subscription": name, "topic": s.topic}).to_string(),
            )
            .await;
            Json(sub_to_json(&s)).into_response()
        }
        Err(e) => error_response(e),
    }
}

async fn get_subscription(
    State(state): State<Arc<AppState>>,
    Path((project, sub)): Path<(String, String)>,
) -> Response {
    match svc(&state)
        .get_subscription(&sub_path(&project, &sub))
        .await
    {
        Ok(s) => Json(sub_to_json(&s)).into_response(),
        Err(e) => error_response(e),
    }
}

async fn list_subscriptions(
    State(state): State<Arc<AppState>>,
    Path(project): Path<String>,
) -> Response {
    match svc(&state).list_subscriptions(Some(&project)).await {
        Ok(subs) => {
            let items: Vec<_> = subs.iter().map(sub_to_json).collect();
            Json(json!({ "subscriptions": items })).into_response()
        }
        Err(e) => error_response(e),
    }
}

async fn delete_subscription(
    State(state): State<Arc<AppState>>,
    Path((project, sub)): Path<(String, String)>,
) -> Response {
    let name = sub_path(&project, &sub);
    match svc(&state).delete_subscription(&name).await {
        Ok(()) => {
            telemetry::emit(
                &state.db,
                "gcp",
                "pubsub",
                "delete_subscription",
                &json!({"subscription": name}).to_string(),
            )
            .await;
            empty_ok()
        }
        Err(e) => error_response(e),
    }
}

// ── Pull ──────────────────────────────────────────────────────────────────────

async fn pull(state: &Arc<AppState>, sub: &str, body: Value) -> Response {
    let max = json_i64(body.get("maxMessages")).unwrap_or(10);
    match svc(state).pull(sub, max).await {
        Ok(pairs) => {
            let messages: Vec<_> = pairs
                .iter()
                .map(|(ack, msg)| {
                    let attributes = serde_json::from_str::<Value>(&msg.attributes)
                        .ok()
                        .filter(|v| v.is_object())
                        .unwrap_or_else(|| json!({}));
                    json!({
                        "ackId": ack.ack_id,
                        "message": {
                            "messageId":   msg.message_id,
                            "data":        msg.data,
                            "attributes":  attributes,
                            "publishTime": rfc3339_z(&msg.created_at),
                        },
                    })
                })
                .collect();
            Json(json!({ "receivedMessages": messages })).into_response()
        }
        Err(e) => error_response(e),
    }
}

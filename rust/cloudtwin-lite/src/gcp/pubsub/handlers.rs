//! GCP Pub/Sub HTTP handlers.
//!
//! These map Google-style REST endpoints onto the provider service methods and
//! keep the request/response JSON glue out of `service.rs`.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};

use super::service::PubsubService;
use crate::AppState;

fn svc(state: &Arc<AppState>) -> PubsubService {
    PubsubService::new(state.db.clone(), state.cfg.gcp_project.clone())
}

/// Router is mounted at `/gcp` in main.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // Topics
        .route(
            "/v1/:project/topics/:topic",
            put(create_topic).get(get_topic).delete(delete_topic),
        )
        .route("/v1/:project/topics", get(list_topics))
        .route("/v1/:project/topics/:topic:publish", post(publish))
        // Subscriptions
        .route(
            "/v1/:project/subscriptions/:sub",
            put(create_subscription)
                .get(get_subscription)
                .delete(delete_subscription),
        )
        .route("/v1/:project/subscriptions", get(list_subscriptions))
        .route("/v1/:project/subscriptions/:sub:pull", post(pull))
        .route(
            "/v1/:project/subscriptions/:sub:acknowledge",
            post(acknowledge),
        )
}

// ── Topic handlers ────────────────────────────────────────────────────────────

fn topic_to_json(t: &super::models::PubsubTopic) -> serde_json::Value {
    serde_json::json!({ "name": t.name })
}

fn sub_to_json(s: &super::models::PubsubSubscription) -> serde_json::Value {
    serde_json::json!({ "name": s.name, "topic": s.topic })
}

async fn create_topic(
    State(state): State<Arc<AppState>>,
    Path((_project, topic)): Path<(String, String)>,
) -> Response {
    match svc(&state).create_topic(&topic).await {
        Ok(t) => (StatusCode::OK, Json(topic_to_json(&t))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn get_topic(
    State(state): State<Arc<AppState>>,
    Path((_project, topic)): Path<(String, String)>,
) -> Response {
    match svc(&state).get_topic(&topic).await {
        Ok(Some(t)) => (StatusCode::OK, Json(topic_to_json(&t))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Topic not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn list_topics(State(state): State<Arc<AppState>>, Path(_project): Path<String>) -> Response {
    match svc(&state).list_topics().await {
        Ok(ts) => {
            let items: Vec<_> = ts.iter().map(topic_to_json).collect();
            (StatusCode::OK, Json(serde_json::json!({ "topics": items }))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn delete_topic(
    State(state): State<Arc<AppState>>,
    Path((_project, topic)): Path<(String, String)>,
) -> StatusCode {
    match svc(&state).delete_topic(&topic).await {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// ── Publish ───────────────────────────────────────────────────────────────────

async fn publish(
    State(state): State<Arc<AppState>>,
    Path((_project, topic)): Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let msgs: Vec<(String, String)> = body
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|m| {
                    let data = m
                        .get("data")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let attrs = m
                        .get("attributes")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "{}".to_string());
                    (data, attrs)
                })
                .collect()
        })
        .unwrap_or_default();

    match svc(&state).publish(&topic, msgs).await {
        Ok(ids) => {
            let results: Vec<_> = ids
                .iter()
                .map(|id| serde_json::json!({ "messageId": id }))
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({ "messageIds": results })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ── Subscription handlers ─────────────────────────────────────────────────────

async fn create_subscription(
    State(state): State<Arc<AppState>>,
    Path((_project, sub)): Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let topic = body.get("topic").and_then(|v| v.as_str()).unwrap_or("");
    match svc(&state).create_subscription(&sub, topic).await {
        Ok(s) => (StatusCode::OK, Json(sub_to_json(&s))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn get_subscription(
    State(state): State<Arc<AppState>>,
    Path((_project, sub)): Path<(String, String)>,
) -> Response {
    match svc(&state).get_subscription(&sub).await {
        Ok(Some(s)) => (StatusCode::OK, Json(sub_to_json(&s))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Subscription not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn list_subscriptions(
    State(state): State<Arc<AppState>>,
    Path(_project): Path<String>,
) -> Response {
    match svc(&state).list_subscriptions().await {
        Ok(subs) => {
            let items: Vec<_> = subs.iter().map(sub_to_json).collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({ "subscriptions": items })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn delete_subscription(
    State(state): State<Arc<AppState>>,
    Path((_project, sub)): Path<(String, String)>,
) -> StatusCode {
    match svc(&state).delete_subscription(&sub).await {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// ── Pull & Acknowledge ────────────────────────────────────────────────────────

async fn pull(
    State(state): State<Arc<AppState>>,
    Path((_project, sub)): Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let max = body
        .get("maxMessages")
        .and_then(|v| v.as_i64())
        .unwrap_or(10);
    match svc(&state).pull(&sub, max).await {
        Ok(pairs) => {
            let messages: Vec<_> = pairs
                .iter()
                .map(|(ack, msg)| {
                    serde_json::json!({
                        "ackId": ack.ack_id,
                        "message": {
                            "messageId":  msg.message_id,
                            "data":       msg.data,
                            "attributes": serde_json::from_str::<serde_json::Value>(&msg.attributes)
                                .unwrap_or(serde_json::Value::Object(Default::default())),
                            "publishTime": msg.created_at,
                        }
                    })
                })
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({ "receivedMessages": messages })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn acknowledge(
    State(state): State<Arc<AppState>>,
    Path((_project, sub)): Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> StatusCode {
    let ack_ids: Vec<String> = body
        .get("ackIds")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    match svc(&state).acknowledge(&sub, ack_ids).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

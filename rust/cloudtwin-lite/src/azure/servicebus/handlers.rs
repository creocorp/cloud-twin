//! Azure Service Bus HTTP handlers.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};

use super::service::ServiceBusService;
use crate::telemetry;
use crate::AppState;

fn svc(state: &Arc<AppState>) -> ServiceBusService {
    ServiceBusService::new(state.db.clone(), state.cfg.azure_namespace.clone())
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({ "error": msg }))).into_response()
}

/// Router is merged into the /azure prefix in main.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // Queues
        .route("/:namespace/queues", get(list_queues))
        .route(
            "/:namespace/queues/:queue",
            put(create_queue).delete(delete_queue),
        )
        .route(
            "/:namespace/queues/:queue/messages",
            post(send_message).get(receive_messages),
        )
        .route(
            "/:namespace/queues/:queue/messages/:lock_token",
            delete(complete_message),
        )
        .route(
            "/:namespace/queues/:queue/messages/:lock_token/abandon",
            post(abandon_message),
        )
        // Topics
        .route("/:namespace/topics", get(list_topics))
        .route(
            "/:namespace/topics/:topic",
            put(create_topic).delete(delete_topic),
        )
        .route(
            "/:namespace/topics/:topic/messages",
            post(send_topic_message),
        )
        // Subscriptions
        .route(
            "/:namespace/topics/:topic/subscriptions",
            get(list_subscriptions),
        )
        .route(
            "/:namespace/topics/:topic/subscriptions/:sub",
            put(create_subscription),
        )
        .route(
            "/:namespace/topics/:topic/subscriptions/:sub/messages",
            get(receive_sub_messages),
        )
        .route(
            "/:namespace/topics/:topic/subscriptions/:sub/messages/:lock_token",
            delete(complete_sub_message),
        )
}

// ── Queue handlers ────────────────────────────────────────────────────────────

async fn create_queue(
    State(state): State<Arc<AppState>>,
    Path((_, queue)): Path<(String, String)>,
) -> Response {
    match svc(&state).create_queue(&queue).await {
        Ok(q) => {
            telemetry::emit(&state.db, "azure", "servicebus", "create_queue", &serde_json::json!({"queue": queue}).to_string()).await;
            (StatusCode::CREATED, Json(serde_json::json!({ "name": q.name }))).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn list_queues(State(state): State<Arc<AppState>>, Path(_ns): Path<String>) -> Response {
    match svc(&state).list_queues().await {
        Ok(qs) => {
            let items: Vec<_> = qs
                .iter()
                .map(|q| serde_json::json!({ "name": q.name }))
                .collect();
            (StatusCode::OK, Json(serde_json::json!({ "value": items }))).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn delete_queue(
    State(state): State<Arc<AppState>>,
    Path((_, queue)): Path<(String, String)>,
) -> StatusCode {
    match svc(&state).delete_queue(&queue).await {
        Ok(_) => {
            let _ = telemetry::emit(&state.db, "azure", "servicebus", "delete_queue", &serde_json::json!({"queue": queue}).to_string()).await;
            StatusCode::OK
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn send_message(
    State(state): State<Arc<AppState>>,
    Path((_, queue)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let body_str = String::from_utf8_lossy(&body).to_string();
    match svc(&state).send_message(&queue, &body_str).await {
        Ok(mid) => {
            telemetry::emit(&state.db, "azure", "servicebus", "send_message", &serde_json::json!({"queue": queue}).to_string()).await;
            (StatusCode::CREATED, Json(serde_json::json!({ "messageId": mid }))).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn receive_messages(
    State(state): State<Arc<AppState>>,
    Path((_, queue)): Path<(String, String)>,
) -> Response {
    match svc(&state).receive_messages(&queue, 1).await {
        Ok(msgs) => {
            let items: Vec<_> = msgs
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "messageId":  m.message_id,
                        "body":       m.body,
                        "lockToken":  m.lock_token,
                    })
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!({ "value": items }))).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn complete_message(
    State(state): State<Arc<AppState>>,
    Path((_, _, lock_token)): Path<(String, String, String)>,
) -> StatusCode {
    match svc(&state).complete_message(&lock_token).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn abandon_message(
    State(state): State<Arc<AppState>>,
    Path((_, _, lock_token)): Path<(String, String, String)>,
) -> StatusCode {
    match svc(&state).abandon_message(&lock_token).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// ── Topic handlers ────────────────────────────────────────────────────────────

async fn create_topic(
    State(state): State<Arc<AppState>>,
    Path((_, topic)): Path<(String, String)>,
) -> Response {
    match svc(&state).create_topic(&topic).await {
        Ok(t) => {
            telemetry::emit(&state.db, "azure", "servicebus", "create_topic", &serde_json::json!({"topic": topic}).to_string()).await;
            (StatusCode::CREATED, Json(serde_json::json!({ "name": t.name }))).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn list_topics(State(state): State<Arc<AppState>>, Path(_ns): Path<String>) -> Response {
    match svc(&state).list_topics().await {
        Ok(ts) => {
            let items: Vec<_> = ts
                .iter()
                .map(|t| serde_json::json!({ "name": t.name }))
                .collect();
            (StatusCode::OK, Json(serde_json::json!({ "value": items }))).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn delete_topic(
    State(state): State<Arc<AppState>>,
    Path((_, topic)): Path<(String, String)>,
) -> StatusCode {
    match svc(&state).delete_topic(&topic).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn send_topic_message(
    State(state): State<Arc<AppState>>,
    Path((_, topic)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let body_str = String::from_utf8_lossy(&body).to_string();
    match svc(&state).send_topic_message(&topic, &body_str).await {
        Ok(mid) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "messageId": mid })),
        )
            .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

// ── Subscription handlers ─────────────────────────────────────────────────────

async fn create_subscription(
    State(state): State<Arc<AppState>>,
    Path((_, topic, sub)): Path<(String, String, String)>,
) -> Response {
    match svc(&state).create_subscription(&topic, &sub).await {
        Ok(s) => {
            telemetry::emit(&state.db, "azure", "servicebus", "create_subscription", &serde_json::json!({"topic": topic, "subscription": sub}).to_string()).await;
            (StatusCode::CREATED, Json(serde_json::json!({ "name": s.name, "topic": s.topic }))).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn list_subscriptions(
    State(state): State<Arc<AppState>>,
    Path((_, topic)): Path<(String, String)>,
) -> Response {
    match svc(&state).list_subscriptions(&topic).await {
        Ok(subs) => {
            let items: Vec<_> = subs
                .iter()
                .map(|s| serde_json::json!({ "name": s.name }))
                .collect();
            (StatusCode::OK, Json(serde_json::json!({ "value": items }))).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn receive_sub_messages(
    State(state): State<Arc<AppState>>,
    Path((_, topic, sub)): Path<(String, String, String)>,
) -> Response {
    match svc(&state)
        .receive_subscription_messages(&topic, &sub, 1)
        .await
    {
        Ok(msgs) => {
            let items: Vec<_> = msgs
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "messageId": m.message_id,
                        "body":      m.body,
                        "lockToken": m.lock_token,
                    })
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!({ "value": items }))).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn complete_sub_message(
    State(state): State<Arc<AppState>>,
    Path((_, _, _, lock_token)): Path<(String, String, String, String)>,
) -> StatusCode {
    match svc(&state).complete_message(&lock_token).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

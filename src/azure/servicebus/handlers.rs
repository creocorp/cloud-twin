//! Azure Service Bus HTTP handlers.
//!
//! The azure-servicebus SDK speaks AMQP, so CloudTwin exposes a small JSON
//! REST API instead (same shape as the original Python backend):
//!
//! ```text
//! GET    /{ns}/queues                                   list queues
//! PUT    /{ns}/queues/{q}                               create queue (201)
//! GET    /{ns}/queues/{q}                               get queue
//! DELETE /{ns}/queues/{q}                               delete queue (204)
//! POST   /{ns}/queues/{q}/messages                      send (raw body, 201)
//! GET    /{ns}/queues/{q}/messages?limit=N              peek-lock receive
//! DELETE /{ns}/queues/{q}/messages/{lock}               complete (204)
//! POST   /{ns}/queues/{q}/messages/{lock}/abandon       abandon
//! POST   /{ns}/queues/{q}/messages/{lock}/deadletter    dead-letter
//! GET    /{ns}/topics                                   list topics
//! PUT|GET|DELETE /{ns}/topics/{t}                       topic CRUD
//! POST   /{ns}/topics/{t}/messages                      publish (fan-out)
//! GET    /{ns}/topics/{t}/subscriptions                 list subscriptions
//! PUT|GET|DELETE /{ns}/topics/{t}/subscriptions/{s}     subscription CRUD
//! GET    /{ns}/topics/{t}/subscriptions/{s}/messages    receive (?limit=N)
//! DELETE /{ns}/topics/{t}/subscriptions/{s}/messages/{lock}            complete
//! POST   /{ns}/topics/{t}/subscriptions/{s}/messages/{lock}/{action}  abandon|deadletter
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde_json::{json, Value};

use super::models::AsbMessage;
use super::service::{state_name, subscription_queue, ServiceBusService};
use crate::telemetry;
use crate::AppState;

fn svc(state: &Arc<AppState>) -> ServiceBusService {
    ServiceBusService::new(state.db.clone(), state.cfg.azure_namespace.clone())
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "error": msg }))).into_response()
}

fn internal(e: anyhow::Error) -> Response {
    err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
}

fn ok(status: StatusCode, v: Value) -> Response {
    (status, Json(v)).into_response()
}

/// Requests for any namespace other than the configured one are unknown.
macro_rules! ns_guard {
    ($state:expr, $ns:expr) => {
        if $ns != $state.cfg.azure_namespace {
            return err(
                StatusCode::NOT_FOUND,
                &format!("Namespace not found: {}", $ns),
            );
        }
    };
}

fn msg_json(m: &AsbMessage) -> Value {
    json!({
        "message_id": m.message_id,
        "lock_token": m.lock_token,
        "body": m.body,
        "state": state_name(m.visible),
        "created_at": m.created_at,
    })
}

/// `?limit=` (default 1); `None` when it is not a positive integer.
fn limit(params: &HashMap<String, String>) -> Option<i64> {
    match params.get("limit") {
        None => Some(1),
        Some(v) => v.parse::<i64>().ok().filter(|n| *n >= 1),
    }
}

fn bad_limit() -> Response {
    err(StatusCode::BAD_REQUEST, "limit must be a positive integer")
}

/// Router is merged into the /azure prefix in main.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // Queues
        .route("/:namespace/queues", get(list_queues))
        .route(
            "/:namespace/queues/:queue",
            put(create_queue).get(get_queue).delete(delete_queue),
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
            "/:namespace/queues/:queue/messages/:lock_token/:action",
            post(settle_message),
        )
        // Topics
        .route("/:namespace/topics", get(list_topics))
        .route(
            "/:namespace/topics/:topic",
            put(create_topic).get(get_topic).delete(delete_topic),
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
            put(create_subscription)
                .get(get_subscription)
                .delete(delete_subscription),
        )
        .route(
            "/:namespace/topics/:topic/subscriptions/:sub/messages",
            get(receive_sub_messages),
        )
        .route(
            "/:namespace/topics/:topic/subscriptions/:sub/messages/:lock_token",
            delete(complete_sub_message),
        )
        .route(
            "/:namespace/topics/:topic/subscriptions/:sub/messages/:lock_token/:action",
            post(settle_sub_message),
        )
}

// ── Queue handlers ────────────────────────────────────────────────────────────

async fn create_queue(
    State(state): State<Arc<AppState>>,
    Path((ns, queue)): Path<(String, String)>,
) -> Response {
    ns_guard!(state, ns);
    match svc(&state).create_queue(&queue).await {
        Ok(q) => {
            telemetry::emit(
                &state.db,
                "azure",
                "servicebus",
                "create_queue",
                &json!({"queue": queue}).to_string(),
            )
            .await;
            ok(
                StatusCode::CREATED,
                json!({ "name": q.name, "namespace": q.namespace, "created_at": q.created_at }),
            )
        }
        Err(e) => internal(e),
    }
}

async fn list_queues(State(state): State<Arc<AppState>>, Path(ns): Path<String>) -> Response {
    ns_guard!(state, ns);
    match svc(&state).list_queues().await {
        Ok(qs) => {
            let items: Vec<_> = qs
                .iter()
                .map(|q| json!({ "name": q.name, "created_at": q.created_at }))
                .collect();
            ok(StatusCode::OK, json!({ "queues": items }))
        }
        Err(e) => internal(e),
    }
}

async fn get_queue(
    State(state): State<Arc<AppState>>,
    Path((ns, queue)): Path<(String, String)>,
) -> Response {
    ns_guard!(state, ns);
    match svc(&state).get_queue(&queue).await {
        Ok(Some(q)) => ok(
            StatusCode::OK,
            json!({ "name": q.name, "namespace": q.namespace, "created_at": q.created_at }),
        ),
        Ok(None) => err(StatusCode::NOT_FOUND, &format!("Queue not found: {queue}")),
        Err(e) => internal(e),
    }
}

async fn delete_queue(
    State(state): State<Arc<AppState>>,
    Path((ns, queue)): Path<(String, String)>,
) -> Response {
    ns_guard!(state, ns);
    match svc(&state).delete_queue(&queue).await {
        Ok(true) => {
            telemetry::emit(
                &state.db,
                "azure",
                "servicebus",
                "delete_queue",
                &json!({"queue": queue}).to_string(),
            )
            .await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => err(StatusCode::NOT_FOUND, &format!("Queue not found: {queue}")),
        Err(e) => internal(e),
    }
}

async fn send_message(
    State(state): State<Arc<AppState>>,
    Path((ns, queue)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    ns_guard!(state, ns);
    let body_str = String::from_utf8_lossy(&body).to_string();
    match svc(&state).send_message(&queue, &body_str).await {
        Ok(Some(m)) => {
            telemetry::emit(
                &state.db,
                "azure",
                "servicebus",
                "send_message",
                &json!({"queue": queue, "message_id": m.message_id}).to_string(),
            )
            .await;
            ok(StatusCode::CREATED, msg_json(&m))
        }
        Ok(None) => err(StatusCode::NOT_FOUND, &format!("Queue not found: {queue}")),
        Err(e) => internal(e),
    }
}

async fn receive_messages(
    State(state): State<Arc<AppState>>,
    Path((ns, queue)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    ns_guard!(state, ns);
    let Some(limit) = limit(&params) else {
        return bad_limit();
    };
    let svc = svc(&state);
    match svc.get_queue(&queue).await {
        Ok(Some(_)) => {}
        Ok(None) => return err(StatusCode::NOT_FOUND, &format!("Queue not found: {queue}")),
        Err(e) => return internal(e),
    }
    match svc.receive_messages(&queue, limit).await {
        Ok(msgs) => ok(
            StatusCode::OK,
            json!({ "messages": msgs.iter().map(msg_json).collect::<Vec<_>>() }),
        ),
        Err(e) => internal(e),
    }
}

async fn complete_message(
    State(state): State<Arc<AppState>>,
    Path((ns, queue, lock_token)): Path<(String, String, String)>,
) -> Response {
    ns_guard!(state, ns);
    complete(&state, &queue, &lock_token).await
}

async fn settle_message(
    State(state): State<Arc<AppState>>,
    Path((ns, queue, lock_token, action)): Path<(String, String, String, String)>,
) -> Response {
    ns_guard!(state, ns);
    settle(&state, &queue, &lock_token, &action).await
}

async fn complete(state: &Arc<AppState>, queue: &str, lock_token: &str) -> Response {
    match svc(state).complete_message(queue, lock_token).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => err(
            StatusCode::NOT_FOUND,
            &format!("Message not found: {lock_token}"),
        ),
        Err(e) => internal(e),
    }
}

async fn settle(state: &Arc<AppState>, queue: &str, lock_token: &str, action: &str) -> Response {
    let svc = svc(state);
    let res = match action {
        "abandon" => svc.abandon_message(queue, lock_token).await,
        "deadletter" => svc.deadletter_message(queue, lock_token).await,
        _ => {
            return err(
                StatusCode::NOT_FOUND,
                &format!("Unknown message action: {action}"),
            )
        }
    };
    match res {
        Ok(true) => ok(StatusCode::OK, json!({})),
        Ok(false) => err(
            StatusCode::NOT_FOUND,
            &format!("Message not found: {lock_token}"),
        ),
        Err(e) => internal(e),
    }
}

// ── Topic handlers ────────────────────────────────────────────────────────────

async fn create_topic(
    State(state): State<Arc<AppState>>,
    Path((ns, topic)): Path<(String, String)>,
) -> Response {
    ns_guard!(state, ns);
    match svc(&state).create_topic(&topic).await {
        Ok(t) => {
            telemetry::emit(
                &state.db,
                "azure",
                "servicebus",
                "create_topic",
                &json!({"topic": topic}).to_string(),
            )
            .await;
            ok(
                StatusCode::CREATED,
                json!({ "name": t.name, "namespace": t.namespace, "created_at": t.created_at }),
            )
        }
        Err(e) => internal(e),
    }
}

async fn list_topics(State(state): State<Arc<AppState>>, Path(ns): Path<String>) -> Response {
    ns_guard!(state, ns);
    match svc(&state).list_topics().await {
        Ok(ts) => {
            let items: Vec<_> = ts
                .iter()
                .map(|t| json!({ "name": t.name, "created_at": t.created_at }))
                .collect();
            ok(StatusCode::OK, json!({ "topics": items }))
        }
        Err(e) => internal(e),
    }
}

async fn get_topic(
    State(state): State<Arc<AppState>>,
    Path((ns, topic)): Path<(String, String)>,
) -> Response {
    ns_guard!(state, ns);
    match svc(&state).get_topic(&topic).await {
        Ok(Some(t)) => ok(
            StatusCode::OK,
            json!({ "name": t.name, "namespace": t.namespace, "created_at": t.created_at }),
        ),
        Ok(None) => err(StatusCode::NOT_FOUND, &format!("Topic not found: {topic}")),
        Err(e) => internal(e),
    }
}

async fn delete_topic(
    State(state): State<Arc<AppState>>,
    Path((ns, topic)): Path<(String, String)>,
) -> Response {
    ns_guard!(state, ns);
    match svc(&state).delete_topic(&topic).await {
        Ok(true) => {
            telemetry::emit(
                &state.db,
                "azure",
                "servicebus",
                "delete_topic",
                &json!({"topic": topic}).to_string(),
            )
            .await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => err(StatusCode::NOT_FOUND, &format!("Topic not found: {topic}")),
        Err(e) => internal(e),
    }
}

async fn send_topic_message(
    State(state): State<Arc<AppState>>,
    Path((ns, topic)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    ns_guard!(state, ns);
    let body_str = String::from_utf8_lossy(&body).to_string();
    match svc(&state).send_topic_message(&topic, &body_str).await {
        Ok(Some(copies)) => {
            telemetry::emit(
                &state.db,
                "azure",
                "servicebus",
                "send_topic_message",
                &json!({"topic": topic, "fan_out": copies.len()}).to_string(),
            )
            .await;
            ok(
                StatusCode::CREATED,
                json!({
                    "message_id": copies.first().map(|m| m.message_id.clone()),
                    "fan_out": copies.len(),
                }),
            )
        }
        Ok(None) => err(StatusCode::NOT_FOUND, &format!("Topic not found: {topic}")),
        Err(e) => internal(e),
    }
}

// ── Subscription handlers ─────────────────────────────────────────────────────

async fn create_subscription(
    State(state): State<Arc<AppState>>,
    Path((ns, topic, sub)): Path<(String, String, String)>,
) -> Response {
    ns_guard!(state, ns);
    match svc(&state).create_subscription(&topic, &sub).await {
        Ok(Some(s)) => {
            telemetry::emit(
                &state.db,
                "azure",
                "servicebus",
                "create_subscription",
                &json!({"topic": topic, "subscription": sub}).to_string(),
            )
            .await;
            ok(
                StatusCode::CREATED,
                json!({ "name": s.name, "topic": s.topic, "created_at": s.created_at }),
            )
        }
        Ok(None) => err(StatusCode::NOT_FOUND, &format!("Topic not found: {topic}")),
        Err(e) => internal(e),
    }
}

async fn list_subscriptions(
    State(state): State<Arc<AppState>>,
    Path((ns, topic)): Path<(String, String)>,
) -> Response {
    ns_guard!(state, ns);
    let svc = svc(&state);
    match svc.get_topic(&topic).await {
        Ok(Some(_)) => {}
        Ok(None) => return err(StatusCode::NOT_FOUND, &format!("Topic not found: {topic}")),
        Err(e) => return internal(e),
    }
    match svc.list_subscriptions(&topic).await {
        Ok(subs) => {
            let items: Vec<_> = subs
                .iter()
                .map(|s| json!({ "name": s.name, "created_at": s.created_at }))
                .collect();
            ok(StatusCode::OK, json!({ "subscriptions": items }))
        }
        Err(e) => internal(e),
    }
}

async fn get_subscription(
    State(state): State<Arc<AppState>>,
    Path((ns, topic, sub)): Path<(String, String, String)>,
) -> Response {
    ns_guard!(state, ns);
    match svc(&state).list_subscriptions(&topic).await {
        Ok(subs) => match subs.into_iter().find(|s| s.name == sub) {
            Some(s) => ok(
                StatusCode::OK,
                json!({ "name": s.name, "topic": s.topic, "created_at": s.created_at }),
            ),
            None => err(
                StatusCode::NOT_FOUND,
                &format!("Subscription not found: {topic}/{sub}"),
            ),
        },
        Err(e) => internal(e),
    }
}

async fn delete_subscription(
    State(state): State<Arc<AppState>>,
    Path((ns, topic, sub)): Path<(String, String, String)>,
) -> Response {
    ns_guard!(state, ns);
    match svc(&state).delete_subscription(&topic, &sub).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => err(
            StatusCode::NOT_FOUND,
            &format!("Subscription not found: {topic}/{sub}"),
        ),
        Err(e) => internal(e),
    }
}

async fn receive_sub_messages(
    State(state): State<Arc<AppState>>,
    Path((ns, topic, sub)): Path<(String, String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    ns_guard!(state, ns);
    let Some(limit) = limit(&params) else {
        return bad_limit();
    };
    let svc = svc(&state);
    match svc.subscription_exists(&topic, &sub).await {
        Ok(true) => {}
        Ok(false) => {
            return err(
                StatusCode::NOT_FOUND,
                &format!("Subscription not found: {topic}/{sub}"),
            )
        }
        Err(e) => return internal(e),
    }
    match svc
        .receive_messages(&subscription_queue(&topic, &sub), limit)
        .await
    {
        Ok(msgs) => ok(
            StatusCode::OK,
            json!({ "messages": msgs.iter().map(msg_json).collect::<Vec<_>>() }),
        ),
        Err(e) => internal(e),
    }
}

async fn complete_sub_message(
    State(state): State<Arc<AppState>>,
    Path((ns, topic, sub, lock_token)): Path<(String, String, String, String)>,
) -> Response {
    ns_guard!(state, ns);
    complete(&state, &subscription_queue(&topic, &sub), &lock_token).await
}

async fn settle_sub_message(
    State(state): State<Arc<AppState>>,
    Path((ns, topic, sub, lock_token, action)): Path<(String, String, String, String, String)>,
) -> Response {
    ns_guard!(state, ns);
    settle(
        &state,
        &subscription_queue(&topic, &sub),
        &lock_token,
        &action,
    )
    .await
}

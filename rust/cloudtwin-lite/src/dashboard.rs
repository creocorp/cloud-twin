//! Dashboard API endpoints served under /api/dashboard/*.
//!
//! Each handler queries SQLite directly and returns the same JSON shape that
//! the Python backend returns so the static dashboard UI works against both
//! backends without modification.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use rusqlite::OptionalExtension;

use crate::AppState;

/// Build the /api/dashboard sub-router.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health))
        .route("/events", get(events))
        .route("/mailbox", get(mailbox))
        .route("/mailbox/:id", get(mailbox_message))
        .route("/aws/ses", get(aws_ses))
        .route("/aws/s3", get(aws_s3))
        .route("/aws/sns", get(aws_sns))
        .route("/aws/sqs", get(aws_sqs))
        .route("/aws/bedrock", get(aws_bedrock))
        .route("/aws/dynamodb", get(aws_dynamodb))
        .route("/aws/lambda", get(aws_lambda))
        .route("/aws/secretsmanager", get(aws_secretsmanager))
        .route("/azure/blob", get(azure_blob))
        .route("/azure/servicebus", get(azure_servicebus))
        .route("/azure/eventgrid", get(azure_eventgrid))
        .route("/azure/functions", get(azure_functions))
        .route("/azure/keyvault", get(azure_keyvault))
        .route("/azure/queue", get(azure_queue))
        .route("/gcp/storage", get(gcp_storage))
        .route("/gcp/pubsub", get(gcp_pubsub))
        .route("/gcp/cloudfunctions", get(gcp_cloudfunctions))
        .route("/gcp/cloudtasks", get(gcp_cloudtasks))
        .route("/gcp/firestore", get(gcp_firestore))
        .route("/gcp/secretmanager", get(gcp_secretmanager))
}

async fn health(State(state): State<Arc<AppState>>) -> Json<Value> {
    let storage_mode = if state.cfg.db_path == ":memory:" {
        "memory"
    } else {
        "sqlite"
    };
    let services = json!({
        "aws/s3":              true,
        "aws/ses":             true,
        "aws/sns":             true,
        "aws/sqs":             true,
        "aws/dynamodb":        true,
        "aws/lambda":          true,
        "aws/secretsmanager":  true,
        "aws/bedrock":         true,
        "azure/blob":          true,
        "azure/servicebus":    true,
        "azure/eventgrid":     true,
        "azure/functions":     true,
        "azure/keyvault":      true,
        "azure/queue":         true,
        "gcp/storage":         true,
        "gcp/pubsub":          true,
        "gcp/cloudfunctions":  true,
        "gcp/cloudtasks":      true,
        "gcp/firestore":       true,
        "gcp/secretmanager":   true,
    });
    Json(json!({
        "status":       "ok",
        "storage_mode": storage_mode,
        "services":     services,
    }))
}

#[derive(Deserialize)]
struct EventsParams {
    #[allow(dead_code)]
    limit: Option<i64>,
}

/// Events are stored by `telemetry::emit` on every mutating operation.
async fn events(State(state): State<Arc<AppState>>, Query(p): Query<EventsParams>) -> Json<Value> {
    let limit = p.limit.unwrap_or(50);
    let rows: Vec<Value> = state
        .db
        .conn
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, provider, service, action, payload, created_at \
                 FROM events ORDER BY id DESC LIMIT ?1",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![limit], |row| {
                    Ok(json!({
                        "id":         row.get::<_, i64>(0)?,
                        "provider":   row.get::<_, String>(1)?,
                        "service":    row.get::<_, String>(2)?,
                        "action":     row.get::<_, String>(3)?,
                        "payload":    row.get::<_, String>(4)?,
                        "created_at": row.get::<_, String>(5)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();
    Json(json!({"events": rows}))
}

async fn aws_s3(State(state): State<Arc<AppState>>) -> Json<Value> {
    let buckets: Vec<Value> = state
        .db
        .conn
        .call(|conn| {
            let mut stmt = conn
                .prepare("SELECT name, created_at FROM s3_buckets ORDER BY created_at DESC")?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(json!({
                        "name": row.get::<_, String>(0)?,
                        "created_at": row.get::<_, String>(1)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();
    Json(json!({"buckets": buckets}))
}

async fn aws_ses(State(state): State<Arc<AppState>>) -> Json<Value> {
    let identities: Vec<Value> = state
        .db
        .conn
        .call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT identity, kind, verified, created_at \
                 FROM ses_identities ORDER BY created_at DESC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    let verified: i64 = row.get(2)?;
                    Ok(json!({
                        "identity": row.get::<_, String>(0)?,
                        "type": row.get::<_, String>(1)?,
                        "verified": verified != 0,
                        "created_at": row.get::<_, String>(3)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();

    let messages: Vec<Value> = state
        .db
        .conn
        .call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT message_id, source, destinations, subject, created_at \
                 FROM ses_messages ORDER BY created_at DESC LIMIT 50",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    let dest_json: String = row.get(2)?;
                    let dests: Vec<String> =
                        serde_json::from_str(&dest_json).unwrap_or_default();
                    Ok(json!({
                        "id": row.get::<_, String>(0)?,
                        "source": row.get::<_, String>(1)?,
                        "destination": dests.join(", "),
                        "subject": row.get::<_, String>(3)?,
                        "created_at": row.get::<_, String>(4)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();

    Json(json!({"identities": identities, "messages": messages}))
}

/// `GET /api/dashboard/mailbox` — list captured messages (newest first).
async fn mailbox(State(state): State<Arc<AppState>>) -> Json<Value> {
    let messages: Vec<Value> = state
        .db
        .conn
        .call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT m.message_id, m.from_addr, m.to_addrs, m.subject, m.source, m.size, \
                        m.text_body IS NOT NULL, m.html_body IS NOT NULL, m.created_at, \
                        (SELECT COUNT(*) FROM mailbox_attachments a WHERE a.message_id = m.message_id) \
                 FROM mailbox_messages m ORDER BY m.created_at DESC LIMIT 200",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    let to_json: String = row.get(2)?;
                    let to: Vec<String> = serde_json::from_str(&to_json).unwrap_or_default();
                    Ok(json!({
                        "id": row.get::<_, String>(0)?,
                        "from": row.get::<_, String>(1)?,
                        "to": to.join(", "),
                        "subject": row.get::<_, String>(3)?,
                        "source": row.get::<_, String>(4)?,
                        "size": row.get::<_, i64>(5)?,
                        "has_text": row.get::<_, i64>(6)? != 0,
                        "has_html": row.get::<_, i64>(7)? != 0,
                        "created_at": row.get::<_, String>(8)?,
                        "attachments": row.get::<_, i64>(9)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();

    Json(json!({ "messages": messages, "smtp_port": state.cfg.smtp_port }))
}

/// `GET /api/dashboard/mailbox/:id` — full detail for a single captured message.
async fn mailbox_message(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<Value> {
    let mid = id.clone();
    let message: Option<Value> = state
        .db
        .conn
        .call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT message_id, from_addr, to_addrs, cc_addrs, bcc_addrs, subject, \
                            text_body, html_body, raw, source, size, created_at \
                     FROM mailbox_messages WHERE message_id = ?1",
                    rusqlite::params![mid],
                    |row| {
                        let to: Vec<String> =
                            serde_json::from_str(&row.get::<_, String>(2)?).unwrap_or_default();
                        let cc: Vec<String> =
                            serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default();
                        let bcc: Vec<String> =
                            serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default();
                        Ok(json!({
                            "id": row.get::<_, String>(0)?,
                            "from": row.get::<_, String>(1)?,
                            "to": to,
                            "cc": cc,
                            "bcc": bcc,
                            "subject": row.get::<_, String>(5)?,
                            "text_body": row.get::<_, Option<String>>(6)?,
                            "html_body": row.get::<_, Option<String>>(7)?,
                            "raw": row.get::<_, String>(8)?,
                            "source": row.get::<_, String>(9)?,
                            "size": row.get::<_, i64>(10)?,
                            "created_at": row.get::<_, String>(11)?,
                        }))
                    },
                )
                .optional()?)
        })
        .await
        .unwrap_or_default();

    let attachments: Vec<Value> = state
        .db
        .conn
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT filename, content_type, size FROM mailbox_attachments \
                 WHERE message_id = ?1 ORDER BY id",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![id], |row| {
                    Ok(json!({
                        "filename": row.get::<_, String>(0)?,
                        "content_type": row.get::<_, String>(1)?,
                        "size": row.get::<_, i64>(2)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();

    match message {
        Some(mut m) => {
            if let Value::Object(ref mut obj) = m {
                obj.insert("attachments".into(), json!(attachments));
            }
            Json(m)
        }
        None => Json(json!({ "error": "not found" })),
    }
}

async fn aws_sns(State(state): State<Arc<AppState>>) -> Json<Value> {
    let topics: Vec<Value> = state
        .db
        .conn
        .call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT arn, name, created_at FROM sns_topics ORDER BY created_at DESC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(json!({
                        "arn": row.get::<_, String>(0)?,
                        "name": row.get::<_, String>(1)?,
                        "created_at": row.get::<_, String>(2)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();

    let subscriptions: Vec<Value> = state
        .db
        .conn
        .call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT arn, topic_arn, protocol, endpoint FROM sns_subscriptions",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(json!({
                        "arn": row.get::<_, String>(0)?,
                        "topic_arn": row.get::<_, String>(1)?,
                        "protocol": row.get::<_, String>(2)?,
                        "endpoint": row.get::<_, String>(3)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();

    Json(json!({"topics": topics, "subscriptions": subscriptions}))
}

async fn aws_sqs(State(state): State<Arc<AppState>>) -> Json<Value> {
    let queues: Vec<Value> = state
        .db
        .conn
        .call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT q.name, q.url, q.created_at, COUNT(m.id) AS msg_count \
                 FROM sqs_queues q \
                 LEFT JOIN sqs_messages m ON m.queue_id = q.id AND m.visible = 1 \
                 GROUP BY q.id \
                 ORDER BY q.created_at DESC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(json!({
                        "name": row.get::<_, String>(0)?,
                        "url": row.get::<_, String>(1)?,
                        "created_at": row.get::<_, String>(2)?,
                        "message_count": row.get::<_, i64>(3)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();
    Json(json!({"queues": queues}))
}

async fn aws_bedrock(State(state): State<Arc<AppState>>) -> Json<Value> {
    let models: Vec<Value> = state
        .bedrock
        .config
        .models
        .iter()
        .map(|(model_id, model)| {
            let count = state.bedrock.get_count(model_id);
            json!({
                "model_id":      model_id,
                "model_name":    model.effective_name(model_id),
                "provider":      model.effective_provider(model_id),
                "request_count": count,
            })
        })
        .collect();
    Json(json!({"models": models}))
}

async fn azure_blob(State(state): State<Arc<AppState>>) -> Json<Value> {
    let account = state.cfg.azure_account.clone();
    let containers: Vec<Value> = state
        .db
        .conn
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT c.name, c.created_at, COUNT(b.id) AS blob_count \
                 FROM azure_containers c \
                 LEFT JOIN azure_blobs b ON b.container = c.name AND b.account = c.account \
                 WHERE c.account = ?1 \
                 GROUP BY c.name, c.created_at \
                 ORDER BY c.created_at DESC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![account], |row| {
                    Ok(json!({
                        "name": row.get::<_, String>(0)?,
                        "created_at": row.get::<_, String>(1)?,
                        "blob_count": row.get::<_, i64>(2)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();
    Json(json!({"containers": containers}))
}

async fn azure_servicebus(State(state): State<Arc<AppState>>) -> Json<Value> {
    let ns_q = state.cfg.azure_namespace.clone();
    let queues: Vec<Value> = state
        .db
        .conn
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT q.name, q.created_at, COUNT(m.id) AS msg_count \
                 FROM asb_queues q \
                 LEFT JOIN asb_messages m \
                   ON m.queue_name = q.name AND m.namespace = q.namespace AND m.visible = 1 \
                 WHERE q.namespace = ?1 \
                 GROUP BY q.name, q.created_at \
                 ORDER BY q.created_at DESC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![ns_q], |row| {
                    Ok(json!({
                        "name": row.get::<_, String>(0)?,
                        "created_at": row.get::<_, String>(1)?,
                        "message_count": row.get::<_, i64>(2)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();

    let ns_t = state.cfg.azure_namespace.clone();
    let topics: Vec<Value> = state
        .db
        .conn
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT t.name, t.created_at, COUNT(s.name) AS sub_count \
                 FROM asb_topics t \
                 LEFT JOIN asb_subscriptions s \
                   ON s.topic = t.name AND s.namespace = t.namespace \
                 WHERE t.namespace = ?1 \
                 GROUP BY t.name, t.created_at \
                 ORDER BY t.created_at DESC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![ns_t], |row| {
                    Ok(json!({
                        "name": row.get::<_, String>(0)?,
                        "created_at": row.get::<_, String>(1)?,
                        "subscription_count": row.get::<_, i64>(2)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();

    Json(json!({"queues": queues, "topics": topics}))
}

async fn gcp_storage(State(state): State<Arc<AppState>>) -> Json<Value> {
    let project = state.cfg.gcp_project.clone();
    let buckets: Vec<Value> = state
        .db
        .conn
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT b.name, b.location, b.created_at, COUNT(o.id) AS obj_count \
                 FROM gcs_buckets b \
                 LEFT JOIN gcs_objects o ON o.bucket = b.name \
                 WHERE b.project = ?1 \
                 GROUP BY b.name, b.location, b.created_at \
                 ORDER BY b.created_at DESC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![project], |row| {
                    Ok(json!({
                        "name": row.get::<_, String>(0)?,
                        "location": row.get::<_, String>(1)?,
                        "created_at": row.get::<_, String>(2)?,
                        "object_count": row.get::<_, i64>(3)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();
    Json(json!({"buckets": buckets}))
}

async fn gcp_pubsub(State(state): State<Arc<AppState>>) -> Json<Value> {
    let topics: Vec<Value> = state
        .db
        .conn
        .call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT t.name, t.created_at, COUNT(s.name) AS sub_count \
                 FROM pubsub_topics t \
                 LEFT JOIN pubsub_subscriptions s ON s.topic = t.name \
                 GROUP BY t.name, t.created_at \
                 ORDER BY t.created_at DESC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(json!({
                        "name": row.get::<_, String>(0)?,
                        "created_at": row.get::<_, String>(1)?,
                        "subscription_count": row.get::<_, i64>(2)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();

    let subscriptions: Vec<Value> = state
        .db
        .conn
        .call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT s.name, s.topic, s.created_at, COUNT(a.ack_id) AS pending_count \
                 FROM pubsub_subscriptions s \
                 LEFT JOIN pubsub_ackables a ON a.subscription = s.name AND a.acked = 0 \
                 GROUP BY s.name, s.topic, s.created_at \
                 ORDER BY s.created_at DESC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(json!({
                        "name": row.get::<_, String>(0)?,
                        "topic": row.get::<_, String>(1)?,
                        "created_at": row.get::<_, String>(2)?,
                        "message_count": row.get::<_, i64>(3)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();

    Json(json!({"topics": topics, "subscriptions": subscriptions}))
}

// ── AWS: DynamoDB ────────────────────────────────────────────────────────────

async fn aws_dynamodb(State(state): State<Arc<AppState>>) -> Json<Value> {
    let tables: Vec<Value> = state
        .db
        .conn
        .call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT name, created_at FROM dynamo_tables ORDER BY created_at DESC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(json!({
                        "name": row.get::<_, String>(0)?,
                        "status": "ACTIVE",
                        "created_at": row.get::<_, String>(1)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();
    Json(json!({"tables": tables}))
}

// ── AWS: Lambda ──────────────────────────────────────────────────────────────

async fn aws_lambda(_state: State<Arc<AppState>>) -> Json<Value> {
    // Lambda is not implemented in the Rust backend; return empty list.
    Json(json!({"functions": []}))
}

// ── AWS: Secrets Manager ─────────────────────────────────────────────────────

async fn aws_secretsmanager(State(state): State<Arc<AppState>>) -> Json<Value> {
    let secrets: Vec<Value> = state
        .db
        .conn
        .call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT name, arn, created_at FROM secrets ORDER BY created_at DESC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(json!({
                        "name": row.get::<_, String>(0)?,
                        "arn": row.get::<_, String>(1)?,
                        "created_at": row.get::<_, String>(2)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();
    Json(json!({"secrets": secrets}))
}

// ── Azure: Event Grid ────────────────────────────────────────────────────────

async fn azure_eventgrid(_state: State<Arc<AppState>>) -> Json<Value> {
    // Event Grid is not implemented in the Rust backend; return empty list.
    Json(json!({"topics": []}))
}

// ── Azure: Functions ─────────────────────────────────────────────────────────

async fn azure_functions(_state: State<Arc<AppState>>) -> Json<Value> {
    // Azure Functions is not implemented in the Rust backend; return empty list.
    Json(json!({"functions": []}))
}

// ── Azure: Key Vault ─────────────────────────────────────────────────────────

async fn azure_keyvault(_state: State<Arc<AppState>>) -> Json<Value> {
    // Key Vault is not implemented in the Rust backend; return empty list.
    Json(json!({"secrets": []}))
}

// ── Azure: Queue Storage ─────────────────────────────────────────────────────

async fn azure_queue(_state: State<Arc<AppState>>) -> Json<Value> {
    // Azure Queue Storage is not implemented in the Rust backend; return empty list.
    Json(json!({"queues": []}))
}

// ── GCP: Cloud Functions ─────────────────────────────────────────────────────

async fn gcp_cloudfunctions(_state: State<Arc<AppState>>) -> Json<Value> {
    // Cloud Functions is not implemented in the Rust backend; return empty list.
    Json(json!({"functions": []}))
}

// ── GCP: Cloud Tasks ─────────────────────────────────────────────────────────

async fn gcp_cloudtasks(_state: State<Arc<AppState>>) -> Json<Value> {
    // Cloud Tasks is not implemented in the Rust backend; return empty list.
    Json(json!({"queues": []}))
}

// ── GCP: Firestore ───────────────────────────────────────────────────────────

async fn gcp_firestore(_state: State<Arc<AppState>>) -> Json<Value> {
    // Firestore is not implemented in the Rust backend; return empty list.
    Json(json!({"collections": []}))
}

// ── GCP: Secret Manager ─────────────────────────────────────────────────────

async fn gcp_secretmanager(_state: State<Arc<AppState>>) -> Json<Value> {
    // Secret Manager is not implemented in the Rust backend; return empty list.
    Json(json!({"secrets": []}))
}

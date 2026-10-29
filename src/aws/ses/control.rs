//! CloudTwin-only SES test controls, mounted at `/_cloudtwin/ses`.
//!
//! * `POST /identities/{identity}/verification` — force verification status:
//!   `{"status": "SUCCESS|PENDING|FAILED|TEMPORARY_FAILURE", "mail_from_status": "..."}`
//! * `GET  /verify-email?identity=...` — the link in captured verification emails
//! * `GET|POST /account` — inspect/override `GetAccount` values
//! * `GET  /events?message_id=&type=` — published SES events (Send, Delivery, Bounce, ...)
//! * `GET  /messages` — outbound messages accepted by SendEmail/SendRawEmail
//! * `POST /inbound?from=&to=a,b` — deliver a raw RFC 822 body through the receipt rules

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};

use super::handlers_v2::account_json;
use super::models::SesError;
use super::receiving::receive_inbound;
use super::service::SesService;
use crate::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/identities/:identity/verification", post(set_verification))
        .route("/verify-email", get(verify_email_link))
        .route("/account", get(get_account).post(set_account))
        .route("/events", get(list_events))
        .route("/messages", get(list_messages))
        .route("/inbound", post(inbound))
}

fn fail(e: SesError) -> Response {
    (
        StatusCode::from_u16(e.v2_status()).unwrap_or(StatusCode::BAD_REQUEST),
        Json(json!({"code": e.v2_code(), "message": e.message})),
    )
        .into_response()
}

fn parse(body: &Bytes) -> Value {
    serde_json::from_slice(body).unwrap_or_else(|_| json!({}))
}

async fn set_verification(
    State(state): State<Arc<AppState>>,
    Path(identity): Path<String>,
    body: Bytes,
) -> Response {
    let req = parse(&body);
    let status = req.get("status").and_then(|v| v.as_str());
    let mail_from = req.get("mail_from_status").and_then(|v| v.as_str());
    if status.is_none() && mail_from.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"message": "Provide status and/or mail_from_status"})),
        )
            .into_response();
    }
    match SesService::new(state)
        .set_verification(&identity, status, mail_from)
        .await
    {
        Ok(id) => Json(json!({
            "identity": id.identity,
            "status": id.status,
            "mail_from_status": id.mail_from_domain.as_ref().map(|_| id.mail_from.status.clone()),
        }))
        .into_response(),
        Err(e) => fail(e),
    }
}

async fn verify_email_link(
    State(state): State<Arc<AppState>>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let identity = q.get("identity").cloned().unwrap_or_default();
    match SesService::new(state).set_verification(&identity, Some("SUCCESS"), None).await {
        Ok(_) => Html(format!(
            "<!doctype html><title>Verified</title><h1>Congratulations!</h1>\
             <p>You have successfully verified <b>{}</b>. You can now send email from this address.</p>",
            crate::proto::xml_escape(&identity)
        ))
        .into_response(),
        Err(e) => fail(e),
    }
}

async fn get_account(State(state): State<Arc<AppState>>) -> Response {
    match SesService::new(state).account().await {
        Ok(a) => Json(account_json(&a)).into_response(),
        Err(e) => fail(e),
    }
}

/// Body keys: `production_access_enabled`, `sending_enabled`,
/// `enforcement_status`, `max_24_hour_send`, `max_send_rate`,
/// `suppressed_reasons`. `null` reverts a key to its configured default.
async fn set_account(State(state): State<Arc<AppState>>, body: Bytes) -> Response {
    const KEYS: &[&str] = &[
        "production_access_enabled",
        "sending_enabled",
        "enforcement_status",
        "max_24_hour_send",
        "max_send_rate",
        "suppressed_reasons",
    ];
    let req = parse(&body);
    let svc = SesService::new(state.clone());
    for k in KEYS {
        match req.get(*k) {
            Some(Value::Null) => {
                let key = k.to_string();
                if let Err(e) = state
                    .db
                    .call(move |c| {
                        c.execute("DELETE FROM ses_account_settings WHERE key = ?1", [key])
                    })
                    .await
                {
                    return fail(e.into());
                }
            }
            Some(v) => {
                if let Err(e) = svc.set_account_setting(k, v.clone()).await {
                    return fail(e);
                }
            }
            None => {}
        }
    }
    get_account(State(state)).await
}

async fn list_events(
    State(state): State<Arc<AppState>>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    match SesService::new(state)
        .list_events(q.get("message_id").cloned(), q.get("type").cloned())
        .await
    {
        Ok(events) => Json(json!({"events": events})).into_response(),
        Err(e) => fail(e),
    }
}

async fn list_messages(State(state): State<Arc<AppState>>) -> Response {
    let rows = state
        .db
        .call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT message_id, source, destinations, subject, status, configuration_set,
                        recipient_status, raw, created_at
                 FROM ses_messages ORDER BY id",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    let parse =
                        |s: String| serde_json::from_str::<Value>(&s).unwrap_or(Value::Null);
                    Ok(json!({
                        "message_id": r.get::<_, String>(0)?,
                        "source": r.get::<_, String>(1)?,
                        "destinations": parse(r.get(2)?),
                        "subject": r.get::<_, String>(3)?,
                        "status": r.get::<_, String>(4)?,
                        "configuration_set": r.get::<_, Option<String>>(5)?,
                        "recipient_status": parse(r.get(6)?),
                        "raw": r.get::<_, Option<String>>(7)?,
                        "created_at": r.get::<_, String>(8)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await;
    match rows {
        Ok(messages) => Json(json!({"messages": messages})).into_response(),
        Err(e) => fail(e.into()),
    }
}

/// Deliver a raw message as if it arrived over SMTP. Recipients not handled by
/// the active receipt rule set land in the capture inbox.
async fn inbound(
    State(state): State<Arc<AppState>>,
    Query(q): Query<HashMap<String, String>>,
    body: Bytes,
) -> Response {
    let from = q.get("from").cloned().unwrap_or_default();
    let to: Vec<String> = q
        .get("to")
        .map(|t| {
            t.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if to.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                json!({"message": "Query parameter `to` (comma-separated recipients) is required"}),
            ),
        )
            .into_response();
    }
    match receive_inbound(&state, &from, &to, &body).await {
        Ok(res) => {
            let mut captured = None;
            if !res.unhandled.is_empty() {
                match crate::mailbox::service::capture_raw(&state.db, &body, "smtp").await {
                    Ok(id) => captured = Some(id),
                    Err(e) => return fail(e.into()),
                }
            }
            Json(json!({
                "message_id": res.message_id,
                "handled": res.handled,
                "unhandled": res.unhandled,
                "mailbox_message_id": captured,
            }))
            .into_response()
        }
        Err(e) => fail(e),
    }
}

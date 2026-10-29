//! SES v2 REST/JSON handlers (`/v2/email/...`).
//!
//! Errors carry `x-amzn-ErrorType` so SDKs map them to typed exceptions
//! (`NotFoundException`, `AlreadyExistsException`, `MessageRejected`, ...).
//! Timestamps are epoch seconds, the restJson1 default.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Response,
    routing::{get, post, put},
    Router,
};
use base64::Engine;
use serde_json::{json, Value};

use super::models::*;
use super::sending::{SendContent, SendRequest};
use super::service::{AccountView, CreateIdentityOpts, SesService};
use crate::proto::{rest_json_error, rest_json_ok};
use crate::telemetry;
use crate::AppState;

pub fn router_v2() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v2/email/outbound-emails", post(send_email))
        .route(
            "/v2/email/identities",
            post(create_identity).get(list_identities),
        )
        .route("/v2/email/list-identities", post(list_identities))
        .route(
            "/v2/email/identities/:identity",
            get(get_identity).delete(delete_identity),
        )
        .route(
            "/v2/email/identities/:identity/mail-from",
            put(put_mail_from),
        )
        .route("/v2/email/identities/:identity/dkim", put(put_dkim))
        .route("/v2/email/identities/:identity/feedback", put(put_feedback))
        .route(
            "/v2/email/identities/:identity/configuration-set",
            put(put_identity_configuration_set),
        )
        .route("/v2/email/account", get(get_account))
        .route("/v2/email/account/sending", put(put_account_sending))
        .route(
            "/v2/email/account/suppression",
            put(put_account_suppression),
        )
        .route(
            "/v2/email/suppression/addresses",
            put(put_suppressed).get(list_suppressed),
        )
        .route(
            "/v2/email/suppression/addresses/:email",
            get(get_suppressed).delete(delete_suppressed),
        )
        .route(
            "/v2/email/configuration-sets",
            post(create_configuration_set).get(list_configuration_sets),
        )
        .route(
            "/v2/email/configuration-sets/:name",
            get(get_configuration_set).delete(delete_configuration_set),
        )
        .route(
            "/v2/email/configuration-sets/:name/event-destinations",
            post(create_event_destination).get(get_event_destinations),
        )
        .route(
            "/v2/email/configuration-sets/:name/event-destinations/:dest",
            put(update_event_destination).delete(delete_event_destination),
        )
        .route(
            "/v2/email/configuration-sets/:name/:option",
            put(put_configuration_set_option),
        )
}

fn svc(state: &Arc<AppState>) -> SesService {
    SesService::new(state.clone())
}

fn err(e: SesError) -> Response {
    rest_json_error(
        StatusCode::from_u16(e.v2_status()).unwrap_or(StatusCode::BAD_REQUEST),
        e.v2_code(),
        &e.message,
    )
}

fn body_json(body: &Bytes) -> SesResult<Value> {
    if body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(body).map_err(|e| bad_request(format!("Malformed JSON body: {e}")))
}

fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str()).filter(|x| !x.is_empty())
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|i| i.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn empty_ok() -> Response {
    rest_json_ok(json!({}))
}

async fn emit(state: &Arc<AppState>, action: &str, payload: Value) {
    telemetry::emit(&state.db, "aws", "ses", action, &payload.to_string()).await;
}

/// Simple offset pagination: `NextToken` is the next index.
fn paginate<T: Clone>(
    items: &[T],
    token: Option<&str>,
    page_size: Option<usize>,
) -> (Vec<T>, Option<String>) {
    let start = token.and_then(|t| t.parse::<usize>().ok()).unwrap_or(0);
    let size = page_size.filter(|n| *n > 0).unwrap_or(1000);
    let end = (start + size).min(items.len());
    let page = items
        .get(start..end)
        .map(|s| s.to_vec())
        .unwrap_or_default();
    let next = (end < items.len()).then(|| end.to_string());
    (page, next)
}

// ── Identities ───────────────────────────────────────────────────────────────

fn dkim_json(id: &SesIdentity) -> Value {
    if id.is_domain() {
        json!({
            "SigningEnabled": id.dkim_signing_enabled,
            "Status": id.dkim_status(),
            "Tokens": id.dkim_tokens,
            "SigningAttributesOrigin": id.dkim_origin,
            "NextSigningKeyLength": "RSA_2048_BIT",
            "CurrentSigningKeyLength": "RSA_2048_BIT",
            "LastKeyGenerationTimestamp": epoch(&id.created_at),
        })
    } else {
        json!({
            "SigningEnabled": false,
            "Status": "NOT_STARTED",
            "Tokens": [],
            "SigningAttributesOrigin": "AWS_SES",
        })
    }
}

fn mail_from_json(id: &SesIdentity) -> Value {
    match &id.mail_from_domain {
        Some(d) => json!({
            "MailFromDomain": d,
            "MailFromDomainStatus": id.mail_from.status,
            "BehaviorOnMxFailure": id.mail_from_behavior,
        }),
        None => json!({"BehaviorOnMxFailure": id.mail_from_behavior}),
    }
}

async fn create_identity(State(state): State<Arc<AppState>>, body: Bytes) -> Response {
    let req = match body_json(&body) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let Some(identity) = s(&req, "EmailIdentity") else {
        return err(bad_request("EmailIdentity is required."));
    };
    let opts = CreateIdentityOpts {
        tags: req.get("Tags").cloned(),
        configuration_set: s(&req, "ConfigurationSetName").map(String::from),
        byodkim_selector: req
            .get("DkimSigningAttributes")
            .and_then(|d| s(d, "DomainSigningSelector"))
            .map(String::from),
    };
    match svc(&state).create_identity(identity, opts, true).await {
        Ok(id) => {
            emit(&state, "verify_identity", json!({"identity": id.identity})).await;
            rest_json_ok(json!({
                "IdentityType": id.identity_type(),
                "VerifiedForSendingStatus": id.verified(),
                "DkimAttributes": dkim_json(&id),
            }))
        }
        Err(e) => err(e),
    }
}

async fn list_identities(
    State(state): State<Arc<AppState>>,
    Query(q): Query<HashMap<String, String>>,
    body: Bytes,
) -> Response {
    let b = body_json(&body).unwrap_or_else(|_| json!({}));
    let token = q
        .get("NextToken")
        .map(|s| s.as_str())
        .or(s(&b, "NextToken"));
    let size = q.get("PageSize").and_then(|v| v.parse().ok()).or(b
        .get("PageSize")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize));
    match svc(&state).list_identities().await {
        Ok(ids) => {
            let items: Vec<Value> = ids
                .iter()
                .map(|i| {
                    json!({
                        "IdentityType": i.identity_type(),
                        "IdentityName": i.identity,
                        "SendingEnabled": i.verified(),
                        "VerificationStatus": i.status,
                    })
                })
                .collect();
            let (page, next) = paginate(&items, token, size);
            let mut out = json!({"EmailIdentities": page});
            if let Some(n) = next {
                out["NextToken"] = json!(n);
            }
            rest_json_ok(out)
        }
        Err(e) => err(e),
    }
}

async fn get_identity(
    State(state): State<Arc<AppState>>,
    Path(identity): Path<String>,
) -> Response {
    match svc(&state).require_identity(&identity, true).await {
        Ok(id) => {
            let mut info = json!({});
            if let Some(t) = &id.last_checked_at {
                info["LastCheckedTimestamp"] = json!(epoch(t));
            }
            if let Some(t) = &id.last_success_at {
                info["LastSuccessTimestamp"] = json!(epoch(t));
            }
            if matches!(id.status.as_str(), "FAILED" | "TEMPORARY_FAILURE") {
                info["ErrorType"] = json!(if id.status == "FAILED" {
                    "HOST_NOT_FOUND"
                } else {
                    "SERVICE_ERROR"
                });
            }
            let mut out = json!({
                "IdentityType": id.identity_type(),
                "FeedbackForwardingStatus": id.feedback_forwarding,
                "VerifiedForSendingStatus": id.verified(),
                "DkimAttributes": dkim_json(&id),
                "MailFromAttributes": mail_from_json(&id),
                "Policies": {},
                "Tags": id.tags,
                "VerificationStatus": id.status,
                "VerificationInfo": info,
            });
            if let Some(cs) = &id.configuration_set {
                out["ConfigurationSetName"] = json!(cs);
            }
            rest_json_ok(out)
        }
        Err(e) => err(e),
    }
}

async fn delete_identity(
    State(state): State<Arc<AppState>>,
    Path(identity): Path<String>,
) -> Response {
    match svc(&state).delete_identity(&identity).await {
        Ok(true) => {
            emit(&state, "delete_identity", json!({"identity": identity})).await;
            empty_ok()
        }
        Ok(false) => err(not_found(format!(
            "Email identity {identity} does not exist."
        ))),
        Err(e) => err(e),
    }
}

async fn put_mail_from(
    State(state): State<Arc<AppState>>,
    Path(identity): Path<String>,
    body: Bytes,
) -> Response {
    let req = match body_json(&body) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    match svc(&state)
        .put_mail_from(
            &identity,
            s(&req, "MailFromDomain"),
            s(&req, "BehaviorOnMxFailure"),
        )
        .await
    {
        Ok(()) => {
            emit(&state, "put_mail_from", json!({"identity": identity})).await;
            empty_ok()
        }
        Err(e) => err(e),
    }
}

async fn put_dkim(
    State(state): State<Arc<AppState>>,
    Path(identity): Path<String>,
    body: Bytes,
) -> Response {
    let req = match body_json(&body) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let enabled = req
        .get("SigningEnabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    match svc(&state).put_dkim_signing(&identity, enabled).await {
        Ok(()) => empty_ok(),
        Err(e) => err(e),
    }
}

async fn put_feedback(
    State(state): State<Arc<AppState>>,
    Path(identity): Path<String>,
    body: Bytes,
) -> Response {
    let req = match body_json(&body) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let enabled = req
        .get("EmailForwardingEnabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    match svc(&state)
        .put_feedback_forwarding(&identity, enabled)
        .await
    {
        Ok(()) => empty_ok(),
        Err(e) => err(e),
    }
}

async fn put_identity_configuration_set(
    State(state): State<Arc<AppState>>,
    Path(identity): Path<String>,
    body: Bytes,
) -> Response {
    let req = match body_json(&body) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let cs = s(&req, "ConfigurationSetName").map(String::from);
    match svc(&state)
        .put_identity_configuration_set(&identity, cs)
        .await
    {
        Ok(()) => empty_ok(),
        Err(e) => err(e),
    }
}

// ── Account ──────────────────────────────────────────────────────────────────

pub fn account_json(a: &AccountView) -> Value {
    json!({
        "DedicatedIpAutoWarmupEnabled": false,
        "EnforcementStatus": a.enforcement_status,
        "ProductionAccessEnabled": a.production_access_enabled,
        "SendQuota": {
            "Max24HourSend": a.max_24_hour_send,
            "MaxSendRate": a.max_send_rate,
            "SentLast24Hours": a.sent_last_24_hours,
        },
        "SendingEnabled": a.sending_enabled,
        "SuppressionAttributes": {"SuppressedReasons": a.suppressed_reasons},
    })
}

async fn get_account(State(state): State<Arc<AppState>>) -> Response {
    match svc(&state).account().await {
        Ok(a) => rest_json_ok(account_json(&a)),
        Err(e) => err(e),
    }
}

async fn put_account_sending(State(state): State<Arc<AppState>>, body: Bytes) -> Response {
    let req = match body_json(&body) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let enabled = req
        .get("SendingEnabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    match svc(&state)
        .set_account_setting("sending_enabled", json!(enabled))
        .await
    {
        Ok(()) => empty_ok(),
        Err(e) => err(e),
    }
}

async fn put_account_suppression(State(state): State<Arc<AppState>>, body: Bytes) -> Response {
    let req = match body_json(&body) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let reasons = strings(req.get("SuppressedReasons"));
    if reasons.iter().any(|r| r != "BOUNCE" && r != "COMPLAINT") {
        return err(bad_request(
            "SuppressedReasons may only contain BOUNCE and COMPLAINT.",
        ));
    }
    match svc(&state)
        .set_account_setting("suppressed_reasons", json!(reasons))
        .await
    {
        Ok(()) => empty_ok(),
        Err(e) => err(e),
    }
}

// ── Suppression list ─────────────────────────────────────────────────────────

async fn put_suppressed(State(state): State<Arc<AppState>>, body: Bytes) -> Response {
    let req = match body_json(&body) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let (Some(email), Some(reason)) = (s(&req, "EmailAddress"), s(&req, "Reason")) else {
        return err(bad_request("EmailAddress and Reason are required."));
    };
    match svc(&state).put_suppressed(email, reason, None, None).await {
        Ok(()) => {
            emit(
                &state,
                "put_suppressed_destination",
                json!({"email": email}),
            )
            .await;
            empty_ok()
        }
        Err(e) => err(e),
    }
}

async fn get_suppressed(State(state): State<Arc<AppState>>, Path(email): Path<String>) -> Response {
    match svc(&state).get_suppressed(&email).await {
        Ok(Some((email, reason, message_id, feedback_id, updated))) => {
            let mut d = json!({
                "EmailAddress": email,
                "Reason": reason,
                "LastUpdateTime": epoch(&updated),
            });
            if message_id.is_some() || feedback_id.is_some() {
                d["Attributes"] = json!({"MessageId": message_id, "FeedbackId": feedback_id});
            }
            rest_json_ok(json!({"SuppressedDestination": d}))
        }
        Ok(None) => err(not_found(format!(
            "Email address {email} does not exist on your suppression list."
        ))),
        Err(e) => err(e),
    }
}

async fn list_suppressed(
    State(state): State<Arc<AppState>>,
    Query(q): Query<Vec<(String, String)>>,
) -> Response {
    let reasons: Vec<String> = q
        .iter()
        .filter(|(k, _)| k == "Reason")
        .map(|(_, v)| v.to_ascii_uppercase())
        .collect();
    let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let parse_ts = |v: Option<&str>| -> Option<f64> {
        let v = v?;
        v.parse::<f64>().ok().or_else(|| {
            chrono::DateTime::parse_from_rfc3339(v)
                .ok()
                .map(|t| t.timestamp() as f64)
        })
    };
    let (start, end) = (parse_ts(get("StartDate")), parse_ts(get("EndDate")));
    match svc(&state).list_suppressed().await {
        Ok(rows) => {
            let items: Vec<Value> = rows
                .into_iter()
                .filter(|(_, r, _)| reasons.is_empty() || reasons.contains(r))
                .filter(|(_, _, t)| start.is_none_or(|s| epoch(t) >= s))
                .filter(|(_, _, t)| end.is_none_or(|e| epoch(t) <= e))
                .map(|(email, reason, t)| {
                    json!({"EmailAddress": email, "Reason": reason, "LastUpdateTime": epoch(&t)})
                })
                .collect();
            let (page, next) = paginate(
                &items,
                get("NextToken"),
                get("PageSize").and_then(|v| v.parse().ok()),
            );
            let mut out = json!({"SuppressedDestinationSummaries": page});
            if let Some(n) = next {
                out["NextToken"] = json!(n);
            }
            rest_json_ok(out)
        }
        Err(e) => err(e),
    }
}

async fn delete_suppressed(
    State(state): State<Arc<AppState>>,
    Path(email): Path<String>,
) -> Response {
    match svc(&state).delete_suppressed(&email).await {
        Ok(true) => empty_ok(),
        Ok(false) => err(not_found(format!(
            "Email address {email} does not exist on your suppression list."
        ))),
        Err(e) => err(e),
    }
}

// ── Configuration sets ───────────────────────────────────────────────────────

const CS_OPTION_KEYS: &[&str] = &[
    "TrackingOptions",
    "DeliveryOptions",
    "ReputationOptions",
    "SendingOptions",
    "SuppressionOptions",
    "VdmOptions",
    "Tags",
];

async fn create_configuration_set(State(state): State<Arc<AppState>>, body: Bytes) -> Response {
    let req = match body_json(&body) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let Some(name) = s(&req, "ConfigurationSetName") else {
        return err(bad_request("ConfigurationSetName is required."));
    };
    let options: serde_json::Map<String, Value> = CS_OPTION_KEYS
        .iter()
        .filter_map(|k| req.get(*k).map(|v| (k.to_string(), v.clone())))
        .collect();
    match svc(&state)
        .create_configuration_set(name, Value::Object(options))
        .await
    {
        Ok(()) => {
            emit(&state, "create_configuration_set", json!({"name": name})).await;
            empty_ok()
        }
        Err(e) => err(e),
    }
}

async fn list_configuration_sets(State(state): State<Arc<AppState>>) -> Response {
    match svc(&state).list_configuration_sets().await {
        Ok(names) => rest_json_ok(json!({"ConfigurationSets": names})),
        Err(e) => err(e),
    }
}

async fn get_configuration_set(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
    match svc(&state).require_configuration_set(&name).await {
        Ok(opts) => {
            let mut out = json!({"ConfigurationSetName": name});
            for k in CS_OPTION_KEYS {
                if let Some(v) = opts.get(*k) {
                    out[*k] = v.clone();
                }
            }
            if out.get("SendingOptions").is_none() {
                out["SendingOptions"] = json!({"SendingEnabled": true});
            }
            rest_json_ok(out)
        }
        Err(e) => err(e),
    }
}

async fn delete_configuration_set(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
    match svc(&state).delete_configuration_set(&name).await {
        Ok(()) => empty_ok(),
        Err(e) => err(e),
    }
}

async fn put_configuration_set_option(
    State(state): State<Arc<AppState>>,
    Path((name, option)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let req = match body_json(&body) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let key = match option.as_str() {
        "tracking-options" => "TrackingOptions",
        "delivery-options" => "DeliveryOptions",
        "reputation-options" => "ReputationOptions",
        "sending" => "SendingOptions",
        "suppression-options" => "SuppressionOptions",
        "vdm-options" => "VdmOptions",
        _ => {
            return err(not_found(format!(
                "Unknown configuration set option: {option}"
            )))
        }
    };
    match svc(&state)
        .update_configuration_set_options(&name, json!({ key: req }))
        .await
    {
        Ok(()) => empty_ok(),
        Err(e) => err(e),
    }
}

async fn create_event_destination(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    body: Bytes,
) -> Response {
    let req = match body_json(&body) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let dest = s(&req, "EventDestinationName").unwrap_or("").to_string();
    let def = req
        .get("EventDestination")
        .cloned()
        .unwrap_or_else(|| json!({}));
    match svc(&state)
        .create_event_destination(&name, &dest, def)
        .await
    {
        Ok(()) => {
            emit(
                &state,
                "create_event_destination",
                json!({"configuration_set": name, "name": dest}),
            )
            .await;
            empty_ok()
        }
        Err(e) => err(e),
    }
}

async fn get_event_destinations(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
    let service = svc(&state);
    if let Err(e) = service.require_configuration_set(&name).await {
        return err(e);
    }
    match service.event_destinations(&name).await {
        Ok(dests) => {
            let items: Vec<Value> = dests
                .into_iter()
                .map(|(n, mut d)| {
                    d["Name"] = json!(n);
                    if d.get("Enabled").is_none() {
                        d["Enabled"] = json!(false);
                    }
                    d
                })
                .collect();
            rest_json_ok(json!({"EventDestinations": items}))
        }
        Err(e) => err(e),
    }
}

async fn update_event_destination(
    State(state): State<Arc<AppState>>,
    Path((name, dest)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let req = match body_json(&body) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let def = req
        .get("EventDestination")
        .cloned()
        .unwrap_or_else(|| json!({}));
    match svc(&state)
        .update_event_destination(&name, &dest, def)
        .await
    {
        Ok(()) => empty_ok(),
        Err(e) => err(e),
    }
}

async fn delete_event_destination(
    State(state): State<Arc<AppState>>,
    Path((name, dest)): Path<(String, String)>,
) -> Response {
    match svc(&state).delete_event_destination(&name, &dest).await {
        Ok(()) => empty_ok(),
        Err(e) => err(e),
    }
}

// ── Sending ──────────────────────────────────────────────────────────────────

async fn send_email(State(state): State<Arc<AppState>>, body: Bytes) -> Response {
    let req = match body_json(&body) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let dest = req.get("Destination").cloned().unwrap_or_else(|| json!({}));
    let content = req.get("Content").cloned().unwrap_or_else(|| json!({}));
    let content = if let Some(raw) = content.get("Raw") {
        let data = s(raw, "Data").unwrap_or("");
        match base64::engine::general_purpose::STANDARD.decode(data) {
            Ok(bytes) => SendContent::Raw(bytes),
            Err(_) => return err(bad_request("Content.Raw.Data must be base64-encoded.")),
        }
    } else if let Some(simple) = content.get("Simple") {
        let part = |v: Option<&Value>| v.and_then(|p| s(p, "Data")).map(String::from);
        let body = simple.get("Body");
        SendContent::Simple {
            subject: part(simple.get("Subject")).unwrap_or_default(),
            text: part(body.and_then(|b| b.get("Text"))),
            html: part(body.and_then(|b| b.get("Html"))),
            headers: simple
                .get("Headers")
                .and_then(|h| h.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|h| {
                            Some((
                                s(h, "Name")?.to_string(),
                                s(h, "Value").unwrap_or("").to_string(),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    } else if content.get("Template").is_some() {
        return err(bad_request(
            "Template content is not supported by CloudTwin.",
        ));
    } else {
        return err(bad_request("Content must contain Simple or Raw."));
    };
    let send = SendRequest {
        from: s(&req, "FromEmailAddress").unwrap_or("").to_string(),
        to: strings(dest.get("ToAddresses")),
        cc: strings(dest.get("CcAddresses")),
        bcc: strings(dest.get("BccAddresses")),
        reply_to: strings(req.get("ReplyToAddresses")),
        content,
        tags: req
            .get("EmailTags")
            .and_then(|t| t.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|t| {
                        Some((
                            s(t, "Name")?.to_string(),
                            s(t, "Value").unwrap_or("").to_string(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        configuration_set: s(&req, "ConfigurationSetName").map(String::from),
        feedback_forwarding_address: s(&req, "FeedbackForwardingEmailAddress").map(String::from),
        return_path: None,
    };
    let source = send.from.clone();
    match svc(&state).send_email(send).await {
        Ok(mid) => {
            emit(
                &state,
                "send_email",
                json!({"source": source, "message_id": mid}),
            )
            .await;
            rest_json_ok(json!({"MessageId": mid}))
        }
        Err(e) => err(e),
    }
}

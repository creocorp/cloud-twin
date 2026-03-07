//! SES protocol handlers.
//!
//! This file contains both the AWS Query handlers for SES v1 and the REST-style
//! routes for SES v2, with both delegating to the same service layer.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};

use crate::proto::{wrap_xml, xml_error_response, xml_escape, xml_ok};
use crate::AppState;
use super::service::SesService;

fn svc(state: &Arc<AppState>) -> SesService { SesService::new(state.db.clone()) }

// ── Query protocol (SES v1) ──────────────────────────────────────────────────

/// AWS Query actions routed to SES.
pub const QUERY_ACTIONS: &[&str] = &[
    "SendEmail",
    "VerifyEmailIdentity",
    "VerifyDomainIdentity",
    "ListIdentities",
    "DeleteIdentity",
    "GetIdentityVerificationAttributes",
];

pub async fn handle_query(
    state: &Arc<AppState>,
    action: &str,
    params: &HashMap<String, String>,
) -> Response {
    const NS: &str = "https://email.amazonaws.com/doc/2010-03-31/";

    match action {
        "VerifyEmailIdentity" => {
            let email = params.get("EmailAddress").map(|s| s.as_str()).unwrap_or("");
            if email.is_empty() {
                return xml_error_response(StatusCode::BAD_REQUEST, "ValidationError", "EmailAddress required");
            }
            match svc(state).verify_email(email).await {
                Ok(_)  => xml_ok(wrap_xml("VerifyEmailIdentity", NS, "<VerifyEmailIdentityResult/>")),
                Err(e) => xml_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalFailure", &e.to_string()),
            }
        }
        "VerifyDomainIdentity" => {
            let domain = params.get("Domain").map(|s| s.as_str()).unwrap_or("");
            if domain.is_empty() {
                return xml_error_response(StatusCode::BAD_REQUEST, "ValidationError", "Domain required");
            }
            match svc(state).verify_domain(domain).await {
                Ok(token) => xml_ok(wrap_xml(
                    "VerifyDomainIdentity", NS,
                    &format!(
                        "<VerifyDomainIdentityResult>\
<VerificationToken>{}</VerificationToken>\
</VerifyDomainIdentityResult>",
                        xml_escape(&token)
                    ),
                )),
                Err(e) => xml_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalFailure", &e.to_string()),
            }
        }
        "ListIdentities" => {
            match svc(state).list_identities().await {
                Ok(ids) => {
                    let members: String = ids.iter()
                        .map(|i| format!("<member>{}</member>", xml_escape(&i.identity)))
                        .collect();
                    xml_ok(wrap_xml(
                        "ListIdentities", NS,
                        &format!("<ListIdentitiesResult><Identities>{members}</Identities></ListIdentitiesResult>"),
                    ))
                }
                Err(e) => xml_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalFailure", &e.to_string()),
            }
        }
        "DeleteIdentity" => {
            let identity = params.get("Identity").map(|s| s.as_str()).unwrap_or("");
            match svc(state).delete_identity(identity).await {
                Ok(_)  => xml_ok(wrap_xml("DeleteIdentity", NS, "<DeleteIdentityResult/>")),
                Err(e) => xml_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalFailure", &e.to_string()),
            }
        }
        "GetIdentityVerificationAttributes" => {
            let mut identities = Vec::new();
            let mut i = 1;
            loop {
                if let Some(v) = params.get(&format!("Identities.member.{i}")) {
                    identities.push(v.clone());
                    i += 1;
                } else { break; }
            }
            match svc(state).list_identities().await {
                Ok(all) => {
                    let map: HashMap<String, _> =
                        all.into_iter().map(|id| (id.identity.clone(), id)).collect();
                    let mut entries = String::new();
                    for ident in &identities {
                        let (status, token) = if let Some(rec) = map.get(ident) {
                            let s = if rec.verified { "Success" } else { "Pending" };
                            (s, rec.token.as_deref().unwrap_or("").to_string())
                        } else {
                            ("NotStarted", String::new())
                        };
                        entries.push_str(&format!(
                            "<entry><key>{k}</key><value>\
<VerificationStatus>{s}</VerificationStatus>\
<VerificationToken>{t}</VerificationToken>\
</value></entry>",
                            k = xml_escape(ident), s = xml_escape(status), t = xml_escape(&token),
                        ));
                    }
                    xml_ok(wrap_xml(
                        "GetIdentityVerificationAttributes", NS,
                        &format!(
                            "<GetIdentityVerificationAttributesResult>\
<VerificationAttributes>{entries}</VerificationAttributes>\
</GetIdentityVerificationAttributesResult>"
                        ),
                    ))
                }
                Err(e) => xml_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalFailure", &e.to_string()),
            }
        }
        "SendEmail" => {
            let source = params.get("Source").map(|s| s.clone()).unwrap_or_default();
            let mut dests: Vec<String> = Vec::new();
            for prefix in &["Destination.ToAddresses", "Destination.CcAddresses", "Destination.BccAddresses"] {
                let mut j = 1;
                loop {
                    if let Some(v) = params.get(&format!("{prefix}.member.{j}")) {
                        dests.push(v.clone()); j += 1;
                    } else { break; }
                }
            }
            let subject   = params.get("Message.Subject.Data").cloned().unwrap_or_default();
            let text_body = params.get("Message.Body.Text.Data").cloned();
            let html_body = params.get("Message.Body.Html.Data").cloned();
            match svc(state).send_email(&source, dests, &subject, text_body, html_body).await {
                Ok(mid) => xml_ok(wrap_xml(
                    "SendEmail", NS,
                    &format!("<SendEmailResult><MessageId>{}</MessageId></SendEmailResult>", xml_escape(&mid)),
                )),
                Err(e) => xml_error_response(StatusCode::BAD_REQUEST, "MessageRejected", &e.to_string()),
            }
        }
        _ => xml_error_response(StatusCode::BAD_REQUEST, "InvalidAction", &format!("Unknown SES action: {action}")),
    }
}

// ── SES v2 REST ───────────────────────────────────────────────────────────────

pub fn router_v2() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v2/email/outbound-emails",        post(v2_send_email))
        .route("/v2/email/identities",             post(v2_create_identity).get(v2_list_identities))
        .route("/v2/email/identities/:identity",   get(v2_get_identity).delete(v2_delete_identity))
}

#[derive(serde::Deserialize)]
struct V2SendEmailReq {
    #[serde(rename = "FromEmailAddress")]
    from: Option<String>,
    #[serde(rename = "Destination")]
    destination: Option<V2Destination>,
    #[serde(rename = "Content")]
    content: Option<V2Content>,
}
#[derive(serde::Deserialize)]
struct V2Destination {
    #[serde(rename = "ToAddresses")]
    to: Option<Vec<String>>,
    #[serde(rename = "CcAddresses")]
    cc: Option<Vec<String>>,
}
#[derive(serde::Deserialize)]
struct V2Content {
    #[serde(rename = "Simple")]
    simple: Option<V2Simple>,
}
#[derive(serde::Deserialize)]
struct V2Simple {
    #[serde(rename = "Subject")]
    subject: Option<V2Part>,
    #[serde(rename = "Body")]
    body: Option<V2Body>,
}
#[derive(serde::Deserialize)]
struct V2Part { #[serde(rename = "Data")] data: Option<String> }
#[derive(serde::Deserialize)]
struct V2Body {
    #[serde(rename = "Text")] text: Option<V2Part>,
    #[serde(rename = "Html")] html: Option<V2Part>,
}

async fn v2_send_email(State(state): State<Arc<AppState>>, Json(req): Json<V2SendEmailReq>) -> Response {
    let source = req.from.unwrap_or_default();
    let mut dests = Vec::new();
    if let Some(d) = &req.destination {
        if let Some(to) = &d.to { dests.extend(to.iter().cloned()); }
        if let Some(cc) = &d.cc { dests.extend(cc.iter().cloned()); }
    }
    let simple    = req.content.as_ref().and_then(|c| c.simple.as_ref());
    let subject   = simple.and_then(|s| s.subject.as_ref()).and_then(|p| p.data.as_deref()).unwrap_or("").to_string();
    let text_body = simple.and_then(|s| s.body.as_ref()).and_then(|b| b.text.as_ref()).and_then(|p| p.data.clone());
    let html_body = simple.and_then(|s| s.body.as_ref()).and_then(|b| b.html.as_ref()).and_then(|p| p.data.clone());
    match svc(&state).send_email(&source, dests, &subject, text_body, html_body).await {
        Ok(mid) => (StatusCode::OK,  Json(serde_json::json!({ "MessageId": mid }))).into_response(),
        Err(e)  => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "message": e.to_string() }))).into_response(),
    }
}

#[derive(serde::Deserialize)]
struct V2CreateIdentityReq {
    #[serde(rename = "EmailIdentity")]
    email_identity: Option<String>,
}

async fn v2_create_identity(State(state): State<Arc<AppState>>, Json(req): Json<V2CreateIdentityReq>) -> Response {
    let identity = req.email_identity.as_deref().unwrap_or("");
    let is_domain = !identity.contains('@');
    if is_domain {
        match svc(&state).verify_domain(identity).await {
            Ok(_)  => (StatusCode::OK, Json(serde_json::json!({ "IdentityType": "DOMAIN", "VerifiedForSendingStatus": true }))).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "message": e.to_string() }))).into_response(),
        }
    } else {
        match svc(&state).verify_email(identity).await {
            Ok(_)  => (StatusCode::OK, Json(serde_json::json!({ "IdentityType": "EMAIL_ADDRESS", "VerifiedForSendingStatus": true }))).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "message": e.to_string() }))).into_response(),
        }
    }
}

async fn v2_list_identities(State(state): State<Arc<AppState>>) -> Response {
    match svc(&state).list_identities().await {
        Ok(ids) => {
            let items: Vec<_> = ids.iter().map(|i| serde_json::json!({
                "IdentityName":             i.identity,
                "IdentityType":             if i.kind == "domain" { "DOMAIN" } else { "EMAIL_ADDRESS" },
                "VerifiedForSendingStatus": i.verified,
            })).collect();
            (StatusCode::OK, Json(serde_json::json!({ "EmailIdentities": items }))).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "message": e.to_string() }))).into_response(),
    }
}

async fn v2_get_identity(State(state): State<Arc<AppState>>, Path(identity): Path<String>) -> Response {
    match svc(&state).get_identity(&identity).await {
        Ok(Some(id)) => (StatusCode::OK, Json(serde_json::json!({
            "IdentityName":             id.identity,
            "VerifiedForSendingStatus": id.verified,
        }))).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "message": "Identity not found" }))).into_response(),
        Err(e)   => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "message": e.to_string() }))).into_response(),
    }
}

async fn v2_delete_identity(State(state): State<Arc<AppState>>, Path(identity): Path<String>) -> StatusCode {
    match svc(&state).delete_identity(&identity).await {
        Ok(_)  => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

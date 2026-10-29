//! SES feature module.
//!
//! SES is split across two protocol surfaces: the older AWS Query API (v1) and
//! the SES v2 REST/JSON API. Both share one service so identity, sending, and
//! receiving state is unified. Inbound mail from the SMTP listener runs through
//! the active receipt rule set (`receiving.rs`).

mod config;
mod control;
mod events;
mod handlers;
mod handlers_v2;
mod models;
mod receiving;
pub mod schema;
mod sending;
mod service;
#[cfg(test)]
mod tests;

pub use config::SesConfig;
pub use handlers::{handle_query, QUERY_ACTIONS};
pub use handlers_v2::router_v2;
pub use receiving::{receive_inbound, MAX_INBOUND_BYTES};

use crate::AppState;
use axum::Router;
use serde_json::Value;
use std::sync::Arc;

use service::SesService;

/// Test-control routes mounted under `/_cloudtwin/ses`.
pub fn control_router() -> Router<Arc<AppState>> {
    control::router()
}

fn err(e: models::SesError) -> anyhow::Error {
    anyhow::anyhow!("{}: {}", e.v2_code(), e.message)
}

/// Provision SES resources (idempotent). Shape:
///
/// ```yaml
/// ses:
///   configuration_sets:
///     - name: outbound
///       event_destinations:
///         - name: to-sns
///           enabled: true
///           matching_event_types: [SEND, DELIVERY, BOUNCE, COMPLAINT]
///           sns_topic_arn: arn:aws:sns:us-east-1:000000000000:ses-events
///   identities:
///     - example.com                       # verified (SUCCESS)
///     - identity: pending.example.com
///       status: PENDING                   # SUCCESS | PENDING | FAILED | TEMPORARY_FAILURE
///       mail_from_domain: mail.pending.example.com
///       mail_from_behavior: USE_DEFAULT_VALUE
///       configuration_set: outbound
///   receipt_rule_sets:
///     - name: inbound
///       active: true
///       rules:                            # SES `ReceiptRule` shape
///         - Name: store
///           Enabled: true
///           Recipients: [example.com]
///           Actions: [{S3Action: {BucketName: mail-inbound, ObjectKeyPrefix: raw/}}]
///   suppressed_destinations:
///     - {email: blocked@example.com, reason: BOUNCE}
/// ```
pub async fn seed(state: &Arc<AppState>, doc: &Value) -> anyhow::Result<()> {
    let svc = SesService::new(state.clone());
    let list = |k: &str| doc.get(k).and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let str_of = |v: &Value, k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from);

    for cs in list("configuration_sets") {
        let Some(name) = cs.as_str().map(String::from).or_else(|| str_of(&cs, "name")) else {
            continue;
        };
        if svc.get_configuration_set(&name).await.map_err(err)?.is_none() {
            svc.create_configuration_set(&name, serde_json::json!({}))
                .await
                .map_err(err)?;
        }
        for d in cs
            .get("event_destinations")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            let Some(dest) = str_of(d, "name") else { continue };
            let def = match d.get("EventDestination") {
                Some(def) => def.clone(),
                None => {
                    let mut def = serde_json::json!({
                        "Enabled": d.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
                        "MatchingEventTypes": d.get("matching_event_types").cloned()
                            .unwrap_or_else(|| serde_json::json!(["SEND", "DELIVERY", "BOUNCE", "COMPLAINT", "REJECT", "DELIVERY_DELAY"])),
                    });
                    if let Some(t) = str_of(d, "sns_topic_arn") {
                        def["SnsDestination"] = serde_json::json!({"TopicArn": t});
                    }
                    def
                }
            };
            let exists = svc
                .event_destinations(&name)
                .await
                .map_err(err)?
                .iter()
                .any(|(n, _)| *n == dest);
            if exists {
                svc.update_event_destination(&name, &dest, def).await.map_err(err)?;
            } else {
                svc.create_event_destination(&name, &dest, def).await.map_err(err)?;
            }
        }
    }

    for i in list("identities") {
        let Some(identity) = i.as_str().map(String::from).or_else(|| str_of(&i, "identity")) else {
            continue;
        };
        let status = str_of(&i, "status").unwrap_or_else(|| "SUCCESS".into());
        svc.ensure_identity_with_status(&identity, &status).await.map_err(err)?;
        if let Some(cs) = str_of(&i, "configuration_set") {
            svc.put_identity_configuration_set(&identity, Some(cs)).await.map_err(err)?;
        }
        if let Some(mf) = str_of(&i, "mail_from_domain") {
            let current = svc.get_identity(&identity, false).await.map_err(err)?;
            if current.and_then(|c| c.mail_from_domain).as_deref() != Some(mf.as_str()) {
                svc.put_mail_from(&identity, Some(&mf), str_of(&i, "mail_from_behavior").as_deref())
                    .await
                    .map_err(err)?;
                let mf_status = str_of(&i, "mail_from_status").unwrap_or_else(|| "SUCCESS".into());
                svc.set_verification(&identity, None, Some(&mf_status)).await.map_err(err)?;
            }
        }
    }

    for s in list("suppressed_destinations") {
        let email = s.as_str().map(String::from).or_else(|| str_of(&s, "email"));
        if let Some(email) = email {
            let reason = str_of(&s, "reason").unwrap_or_else(|| "BOUNCE".into());
            svc.put_suppressed(&email, &reason, None, None).await.map_err(err)?;
        }
    }

    for rs in list("receipt_rule_sets") {
        let Some(name) = str_of(&rs, "name") else { continue };
        if !svc.list_rule_sets().await.map_err(err)?.iter().any(|(n, _)| *n == name) {
            svc.create_rule_set(&name).await.map_err(err)?;
        }
        let (_, existing) = svc.describe_rule_set(&name).await.map_err(err)?;
        let mut after: Option<String> = None;
        for rule in rs.get("rules").and_then(|v| v.as_array()).into_iter().flatten() {
            let Some(rule_name) = str_of(rule, "Name") else { continue };
            if existing.iter().any(|r| r["Name"].as_str() == Some(rule_name.as_str())) {
                svc.update_rule(&name, rule.clone()).await.map_err(err)?;
            } else {
                svc.create_rule(&name, rule.clone(), after.as_deref()).await.map_err(err)?;
            }
            after = Some(rule_name);
        }
        if rs.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
            svc.set_active_rule_set(Some(&name)).await.map_err(err)?;
        }
    }

    Ok(())
}

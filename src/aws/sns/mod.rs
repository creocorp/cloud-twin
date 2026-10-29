//! SNS feature module.

mod delivery;
mod filter;
mod handlers;
mod models;
pub mod schema;
mod service;
#[cfg(test)]
mod tests;

pub use handlers::{handle_query, QUERY_ACTIONS};

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::AppState;

/// Publish to a topic on behalf of another service (SES events and receipt
/// notifications, S3 event notifications) and deliver to its subscribers
/// exactly like the Publish API (filter policies, raw delivery, SQS/HTTP/email).
/// `message_attributes` uses the SQS JSON shape.
pub async fn publish_internal(
    state: &Arc<AppState>,
    topic_arn: &str,
    message: String,
    subject: Option<String>,
    message_attributes: Option<serde_json::Value>,
) -> anyhow::Result<String> {
    let attributes = crate::aws::sqs::models::parse_attrs(message_attributes.as_ref())
        .map_err(|e| anyhow::anyhow!(e))?;
    let req = delivery::PublishRequest {
        topic_arn: Some(topic_arn.to_string()),
        message,
        subject,
        attributes,
        ..Default::default()
    };
    let res = delivery::publish(state, req)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(res.message_id)
}

fn value_map(v: Option<&serde_json::Value>) -> BTreeMap<String, String> {
    v.and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .map(|(k, v)| {
                    let s = match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    (k.clone(), s)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Provision topics and subscriptions:
/// `{"topics": ["name" | {"name": ..., "attributes": {...}, "tags": {...},
///   "subscriptions": [{"protocol": "sqs", "endpoint": "<queue ARN, URL or name>", "attributes": {...}}]}]}`.
///
/// Idempotent: existing topics/subscriptions get their attributes updated.
/// A bare queue name as an `sqs` endpoint is converted to the queue ARN.
pub async fn seed(state: &Arc<AppState>, doc: &serde_json::Value) -> anyhow::Result<()> {
    let svc = service::SnsService::new(state.db.clone());
    for t in doc
        .get("topics")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        let Some(name) = t
            .as_str()
            .or_else(|| t.get("name").and_then(|v| v.as_str()))
        else {
            continue;
        };
        let mut attrs = value_map(t.get("attributes"));
        if name.ends_with(".fifo") {
            attrs
                .entry("FifoTopic".into())
                .or_insert_with(|| "true".into());
        }
        let tags = value_map(t.get("tags"));
        let arn = service::topic_arn(name);
        match svc.find_topic(&arn).await? {
            Some(existing) => {
                for (k, v) in &attrs {
                    if k != "FifoTopic" {
                        svc.set_topic_attribute(&arn, k, v).await?;
                    }
                }
                if !tags.is_empty() {
                    let mut merged = existing.tags;
                    merged.extend(tags);
                    svc.set_tags(&arn, merged).await?;
                }
            }
            None => {
                svc.create_topic(name, attrs, tags).await?;
            }
        }
        for sub in t
            .get("subscriptions")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            let protocol = sub
                .get("protocol")
                .and_then(|v| v.as_str())
                .unwrap_or("sqs");
            let mut endpoint = sub
                .get("endpoint")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if protocol == "sqs" && !endpoint.starts_with("arn:") && !endpoint.starts_with("http") {
                endpoint = crate::aws::sqs::queue_arn_for(&endpoint);
            }
            let sub_attrs = value_map(sub.get("attributes"));
            let sub_arn = svc
                .subscribe(&arn, protocol, &endpoint, BTreeMap::new())
                .await?;
            for (k, v) in &sub_attrs {
                svc.set_subscription_attribute(&sub_arn, k, v).await?;
            }
        }
    }
    Ok(())
}

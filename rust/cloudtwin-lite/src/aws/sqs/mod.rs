//! SQS feature module.

mod errors;
mod handlers;
pub(crate) mod models;
pub mod schema;
mod service;
#[cfg(test)]
mod tests;

pub use handlers::handle_json;

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::AppState;
use models::{parse_attrs, MsgAttrs, SendInput};
use service::SqsService;

pub(crate) use models::{MsgAttr, MsgAttrs as MessageAttributes};
pub(crate) use service::arn_for as queue_arn_for;

fn svc(state: &Arc<AppState>) -> SqsService {
    SqsService::new(state.db.clone(), state.cfg.base_url.clone())
}

/// Enqueue a message on behalf of another service (SNS fan-out, S3 event
/// notifications). `queue` may be a queue ARN, URL, or name.
/// `message_attributes` uses the SQS JSON shape
/// `{"Name": {"DataType": "String", "StringValue": "v"}}`.
/// The queue's DelaySeconds applies; FIFO queues get a synthetic group id and
/// a unique deduplication id.
pub async fn send_internal(
    state: &Arc<AppState>,
    queue: &str,
    body: String,
    message_attributes: Option<serde_json::Value>,
) -> anyhow::Result<String> {
    let attrs = parse_attrs(message_attributes.as_ref()).map_err(|e| anyhow::anyhow!(e))?;
    send_with(state, queue, body, attrs, None, None).await
}

/// Like [`send_internal`] with typed attributes and optional FIFO fields.
pub(crate) async fn send_with(
    state: &Arc<AppState>,
    queue: &str,
    body: String,
    attributes: MsgAttrs,
    group_id: Option<String>,
    dedup_id: Option<String>,
) -> anyhow::Result<String> {
    let s = svc(state);
    let q = s.get_queue(queue).await?;
    let (group_id, dedup_id) = if q.is_fifo() {
        (
            Some(group_id.unwrap_or_else(|| "cloudtwin".into())),
            Some(dedup_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string())),
        )
    } else {
        (group_id, None)
    };
    let out = s
        .send(
            &q.name,
            SendInput {
                body,
                attributes,
                group_id,
                dedup_id,
                ..Default::default()
            },
        )
        .await?;
    Ok(out.message_id)
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

/// Provision queues:
/// `{"queues": ["name" | {"name": ..., "attributes": {...}, "tags": {...}}]}`.
///
/// Idempotent: existing queues get their attributes/tags updated. Queues are
/// created first and RedrivePolicies applied afterwards, so dead-letter queues
/// may appear anywhere in the list. `RedrivePolicy` may be a JSON string or a
/// map, and its `deadLetterTargetArn` may be a bare queue name.
pub async fn seed(state: &Arc<AppState>, doc: &serde_json::Value) -> anyhow::Result<()> {
    let s = svc(state);
    let mut redrives = Vec::new();
    for q in doc
        .get("queues")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        let Some(name) = q
            .as_str()
            .or_else(|| q.get("name").and_then(|v| v.as_str()))
        else {
            continue;
        };
        let mut attrs = value_map(q.get("attributes"));
        let tags = value_map(q.get("tags"));
        if let Some(rp) = q.get("attributes").and_then(|a| a.get("RedrivePolicy")) {
            let mut rp: serde_json::Value = match rp {
                serde_json::Value::String(s) => serde_json::from_str(s)?,
                other => other.clone(),
            };
            if let Some(target) = rp.get("deadLetterTargetArn").and_then(|v| v.as_str()) {
                let arn = queue_arn_for(target);
                rp["deadLetterTargetArn"] = serde_json::Value::String(arn);
            }
            redrives.push((name.to_string(), rp.to_string()));
        }
        attrs.remove("RedrivePolicy");
        if name.ends_with(".fifo") {
            attrs
                .entry("FifoQueue".into())
                .or_insert_with(|| "true".into());
        }
        match s.get_queue(name).await {
            Ok(_) => {
                attrs.remove("FifoQueue");
                if !attrs.is_empty() {
                    s.set_queue_attributes(name, attrs).await?;
                }
                if !tags.is_empty() {
                    s.tag_queue(name, tags).await?;
                }
            }
            Err(_) => {
                s.create_queue(name, attrs, tags).await?;
            }
        }
    }
    for (name, rp) in redrives {
        s.set_queue_attributes(&name, BTreeMap::from([("RedrivePolicy".to_string(), rp)]))
            .await?;
    }
    Ok(())
}

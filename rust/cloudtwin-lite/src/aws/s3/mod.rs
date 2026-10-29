//! S3 feature module mounted under the AWS provider.

mod bucket;
mod chunked;
mod config;
mod cors;
mod errors;
mod handlers;
mod models;
mod multipart;
mod notify;
mod object;
mod post_object;
pub mod schema;
mod service;
mod xmlutil;

#[cfg(test)]
mod tests;

pub use config::S3Config;

use crate::AppState;
use axum::Router;
use serde_json::{json, Value};
use std::sync::Arc;

use models::ObjMeta;
use service::S3Service;

/// Build the REST router for S3-style bucket and object routes.
pub fn router() -> Router<Arc<AppState>> {
    handlers::router()
}

/// Write an object on behalf of another service (e.g. an SES receipt rule's
/// S3 action) with the same side effects as a client `PutObject`: versioning,
/// default encryption, telemetry and `ObjectCreated:Put` notifications.
/// Metadata names may be given with or without the `x-amz-meta-` prefix.
pub async fn put_object_internal(
    state: &Arc<AppState>,
    bucket: &str,
    key: &str,
    body: Vec<u8>,
    content_type: &str,
    metadata: Vec<(String, String)>,
) -> anyhow::Result<String> {
    let meta = ObjMeta {
        user: metadata
            .into_iter()
            .map(|(k, v)| {
                let k = k.to_ascii_lowercase();
                (k.strip_prefix("x-amz-meta-").unwrap_or(&k).to_string(), v)
            })
            .collect(),
        ..Default::default()
    };
    let wr = S3Service::new(state.db.clone())
        .put_object(bucket, key, body, content_type, meta, Vec::new())
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    crate::telemetry::emit(
        &state.db,
        "aws",
        "s3",
        "put_object",
        &json!({"bucket": bucket, "key": key, "size": wr.size, "internal": true}).to_string(),
    )
    .await;
    let request_id = uuid::Uuid::new_v4().simple().to_string()[..16].to_ascii_uppercase();
    object::fire_created(state, bucket, key, "ObjectCreated:Put", &wr, &request_id).await;
    Ok(wr.etag)
}

/// XML document from either a raw XML string or a JSON object (boto3 shape).
fn seed_doc(v: &Value, root: &str) -> String {
    match v {
        Value::String(s) => s.clone(),
        _ => format!(
            "{}<{root} xmlns=\"{}\">{}</{root}>",
            xmlutil::XML_DECL,
            xmlutil::XMLNS,
            xmlutil::json_to_xml_children(v)
        ),
    }
}

/// Provision buckets from a seed document:
/// `{"buckets": ["name" | {"name", "versioning"?, "cors"?, "notifications"?,
/// "lifecycle"?, "public_access_block"?, "encryption"?, "policy"?, "tags"?}]}`.
/// Idempotent; seeded notifications do not send `s3:TestEvent` messages.
pub async fn seed(state: &Arc<AppState>, doc: &Value) -> anyhow::Result<()> {
    let svc = S3Service::new(state.db.clone());
    let err = |e: errors::S3Error| anyhow::anyhow!("{e}");
    for b in doc
        .get("buckets")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        let Some(name) = b
            .as_str()
            .or_else(|| b.get("name").and_then(|v| v.as_str()))
        else {
            continue;
        };
        svc.create_bucket(name).await.map_err(err)?;
        match b.get("versioning") {
            Some(Value::Bool(true)) => svc
                .put_config(name, "versioning", "Enabled")
                .await
                .map_err(err)?,
            Some(Value::String(s)) if s == "Enabled" || s == "Suspended" => {
                svc.put_config(name, "versioning", s).await.map_err(err)?
            }
            _ => {}
        }
        if let Some(c) = b.get("cors") {
            let rules = cors::rules_from_json(c).map_err(err)?;
            svc.put_config(name, "cors", &cors::rules_to_xml(&rules))
                .await
                .map_err(err)?;
        }
        if let Some(n) = b.get("notifications") {
            svc.put_config(name, "notification", &notify::to_xml(&notify::from_seed(n)))
                .await
                .map_err(err)?;
        }
        if let Some(l) = b.get("lifecycle") {
            let l = match l {
                Value::Array(rules) => json!({"Rules": rules}),
                other => other.clone(),
            };
            svc.put_config(name, "lifecycle", &seed_doc(&l, "LifecycleConfiguration"))
                .await
                .map_err(err)?;
        }
        if let Some(p) = b.get("public_access_block") {
            svc.put_config(
                name,
                "publicAccessBlock",
                &seed_doc(p, "PublicAccessBlockConfiguration"),
            )
            .await
            .map_err(err)?;
        }
        if let Some(e) = b.get("encryption") {
            let xml = match e {
                Value::String(s) if !s.trim_start().starts_with('<') => format!(
                    "{}<ServerSideEncryptionConfiguration xmlns=\"{}\"><Rule><ApplyServerSideEncryptionByDefault><SSEAlgorithm>{}</SSEAlgorithm></ApplyServerSideEncryptionByDefault></Rule></ServerSideEncryptionConfiguration>",
                    xmlutil::XML_DECL,
                    xmlutil::XMLNS,
                    xmlutil::esc(s)
                ),
                other => seed_doc(other, "ServerSideEncryptionConfiguration"),
            };
            svc.put_config(name, "encryption", &xml)
                .await
                .map_err(err)?;
        }
        if let Some(p) = b.get("policy") {
            let body = match p {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            svc.put_config(name, "policy", &body).await.map_err(err)?;
        }
        if let Some(Value::Object(tags)) = b.get("tags") {
            let pairs: Vec<(String, String)> = tags
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        v.as_str()
                            .map(String::from)
                            .unwrap_or_else(|| v.to_string()),
                    )
                })
                .collect();
            let xml = xmlutil::tagging_xml(&pairs);
            svc.put_config(name, "tagging", &xml).await.map_err(err)?;
        }
    }
    Ok(())
}

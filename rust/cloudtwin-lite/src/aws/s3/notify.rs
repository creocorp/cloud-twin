//! Bucket event notifications (SQS queues and SNS topics).

use std::sync::Arc;

use chrono::Utc;
use serde_json::json;
use tracing::warn;

use super::errors::{S3Error, S3Result};
use super::service::S3Service;
use super::xmlutil::{esc, url_encode_plus, XMLNS, XML_DECL};
use crate::aws::{ACCOUNT_ID, REGION};
use crate::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestKind {
    Queue,
    Topic,
    Lambda,
}

impl DestKind {
    fn element(self) -> (&'static str, &'static str) {
        match self {
            DestKind::Queue => ("QueueConfiguration", "Queue"),
            DestKind::Topic => ("TopicConfiguration", "Topic"),
            DestKind::Lambda => ("CloudFunctionConfiguration", "CloudFunction"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub kind: DestKind,
    pub id: String,
    pub arn: String,
    pub events: Vec<String>,
    /// Filter rules as (name as given, value); names are prefix/suffix.
    pub rules: Vec<(String, String)>,
}

impl Target {
    fn prefix(&self) -> Option<&str> {
        self.rules
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("prefix"))
            .map(|(_, v)| v.as_str())
    }

    fn suffix(&self) -> Option<&str> {
        self.rules
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("suffix"))
            .map(|(_, v)| v.as_str())
    }

    pub fn matches(&self, event_name: &str, key: &str) -> bool {
        let full = format!("s3:{event_name}");
        let event_ok = self
            .events
            .iter()
            .any(|e| e == &full || e.strip_suffix('*').is_some_and(|p| full.starts_with(p)));
        event_ok
            && self.prefix().is_none_or(|p| key.starts_with(p))
            && self.suffix().is_none_or(|s| key.ends_with(s))
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NotificationConfig {
    pub targets: Vec<Target>,
    pub eventbridge: bool,
}

fn generated_id() -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD_NO_PAD
        .encode(uuid::Uuid::new_v4().simple().to_string())
}

/// Parse and validate a `<NotificationConfiguration>` document.
pub fn parse(xml: &[u8]) -> S3Result<NotificationConfig> {
    let doc = crate::xml::parse(xml).map_err(|_| S3Error::malformed_xml())?;
    if doc.name != "NotificationConfiguration" {
        return Err(S3Error::malformed_xml());
    }
    let mut cfg = NotificationConfig::default();
    for c in &doc.children {
        let (kind, arn_name) = match c.name.as_str() {
            "QueueConfiguration" => (DestKind::Queue, "Queue"),
            "TopicConfiguration" => (DestKind::Topic, "Topic"),
            "CloudFunctionConfiguration" | "LambdaFunctionConfiguration" => {
                (DestKind::Lambda, "CloudFunction")
            }
            "EventBridgeConfiguration" => {
                cfg.eventbridge = true;
                continue;
            }
            _ => continue,
        };
        let arn = c
            .child_text(arn_name)
            .or_else(|| c.child_text("LambdaFunctionArn"))
            .filter(|a| !a.is_empty())
            .ok_or_else(S3Error::malformed_xml)?;
        let events: Vec<String> = c
            .children_named("Event")
            .map(|e| e.text.trim().to_string())
            .collect();
        if events.is_empty() {
            return Err(S3Error::malformed_xml());
        }
        for e in &events {
            if !e.starts_with("s3:") {
                return Err(S3Error::invalid_argument(
                    "The event is not supported for notifications",
                )
                .with("ArgumentName", e.clone()));
            }
        }
        let mut rules = Vec::new();
        if let Some(key) = c.path(&["Filter", "S3Key"]) {
            for r in key.children_named("FilterRule") {
                let name = r.child_text("Name").unwrap_or_default();
                if !name.eq_ignore_ascii_case("prefix") && !name.eq_ignore_ascii_case("suffix") {
                    return Err(S3Error::invalid_argument(
                        "filter rule name must be either prefix or suffix",
                    )
                    .with("ArgumentName", "FilterRule.Name")
                    .with("ArgumentValue", name));
                }
                rules.push((
                    name,
                    r.child("Value").map(|v| v.text.clone()).unwrap_or_default(),
                ));
            }
        }
        cfg.targets.push(Target {
            kind,
            id: c
                .child_text("Id")
                .filter(|i| !i.is_empty())
                .unwrap_or_else(generated_id),
            arn,
            events,
            rules,
        });
    }
    Ok(cfg)
}

pub fn to_xml(cfg: &NotificationConfig) -> String {
    let mut out = format!("{XML_DECL}<NotificationConfiguration xmlns=\"{XMLNS}\">");
    for kind in [DestKind::Topic, DestKind::Queue, DestKind::Lambda] {
        for t in cfg.targets.iter().filter(|t| t.kind == kind) {
            let (el, arn_el) = kind.element();
            out.push_str(&format!(
                "<{el}><Id>{}</Id><{arn_el}>{}</{arn_el}>",
                esc(&t.id),
                esc(&t.arn)
            ));
            for e in &t.events {
                out.push_str(&format!("<Event>{}</Event>", esc(e)));
            }
            if !t.rules.is_empty() {
                out.push_str("<Filter><S3Key>");
                for (n, v) in &t.rules {
                    out.push_str(&format!(
                        "<FilterRule><Name>{}</Name><Value>{}</Value></FilterRule>",
                        esc(n),
                        esc(v)
                    ));
                }
                out.push_str("</S3Key></Filter>");
            }
            out.push_str(&format!("</{el}>"));
        }
    }
    if cfg.eventbridge {
        out.push_str("<EventBridgeConfiguration/>");
    }
    out.push_str("</NotificationConfiguration>");
    out
}

fn qualify(kind: DestKind, name: &str) -> String {
    if name.starts_with("arn:") {
        return name.to_string();
    }
    let svc = if kind == DestKind::Queue {
        "sqs"
    } else {
        "sns"
    };
    format!("arn:aws:{svc}:{REGION}:{ACCOUNT_ID}:{name}")
}

/// Seed shape: `{"queues": [{"id"?, "queue", "events"?, "prefix"?, "suffix"?}], "topics": [{"topic", ...}]}`.
pub fn from_seed(v: &serde_json::Value) -> NotificationConfig {
    let mut cfg = NotificationConfig::default();
    for (list, kind, field) in [
        ("queues", DestKind::Queue, "queue"),
        ("topics", DestKind::Topic, "topic"),
    ] {
        for (i, item) in v
            .get(list)
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
            .enumerate()
        {
            let Some(dest) = item
                .get(field)
                .or_else(|| item.get("arn"))
                .and_then(|x| x.as_str())
            else {
                continue;
            };
            let events: Vec<String> = item
                .get("events")
                .and_then(|e| e.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .filter(|a: &Vec<String>| !a.is_empty())
                .unwrap_or_else(|| vec!["s3:ObjectCreated:*".into()]);
            let mut rules = Vec::new();
            for r in ["prefix", "suffix"] {
                if let Some(val) = item.get(r).and_then(|x| x.as_str()) {
                    rules.push((r.to_string(), val.to_string()));
                }
            }
            cfg.targets.push(Target {
                kind,
                id: item
                    .get("id")
                    .and_then(|x| x.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| format!("seed-{list}-{i}")),
                arn: qualify(kind, dest),
                events,
                rules,
            });
        }
    }
    cfg
}

async fn deliver(state: &Arc<AppState>, t: &Target, body: String) -> anyhow::Result<()> {
    match t.kind {
        DestKind::Queue => {
            crate::aws::sqs::send_internal(state, &t.arn, body, None).await?;
        }
        DestKind::Topic => {
            crate::aws::sns::publish_internal(
                state,
                &t.arn,
                body,
                Some("Amazon S3 Notification".to_string()),
                None,
            )
            .await?;
        }
        DestKind::Lambda => {}
    }
    Ok(())
}

async fn topic_exists(state: &Arc<AppState>, arn: &str) -> bool {
    let arn = arn.to_string();
    state
        .db
        .call(move |c| {
            c.query_row(
                "SELECT COUNT(*) FROM sns_topics WHERE arn = ?1",
                [arn],
                |r| r.get::<_, i64>(0),
            )
        })
        .await
        .map(|n| n > 0)
        // If the SNS schema changed, fall back to publish errors for validation.
        .unwrap_or(true)
}

/// PutBucketNotificationConfiguration: validate destinations by sending an
/// `s3:TestEvent` to each (like AWS), then store the normalized document.
pub async fn put(
    state: &Arc<AppState>,
    bucket: &str,
    body: &[u8],
    request_id: &str,
    skip_validation: bool,
) -> S3Result<()> {
    let svc = S3Service::new(state.db.clone());
    if !svc.bucket_exists(bucket).await? {
        return Err(S3Error::no_such_bucket(bucket));
    }
    let cfg = parse(body)?;
    if !skip_validation {
        let test = json!({
            "Service": "Amazon S3",
            "Event": "s3:TestEvent",
            "Time": Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            "Bucket": bucket,
            "RequestId": request_id,
            "HostId": host_id(request_id),
        })
        .to_string();
        let mut seen = Vec::new();
        for t in cfg.targets.iter().filter(|t| t.kind != DestKind::Lambda) {
            if seen.contains(&t.arn) {
                continue;
            }
            seen.push(t.arn.clone());
            let ok = (t.kind != DestKind::Topic || topic_exists(state, &t.arn).await)
                && deliver(state, t, test.clone()).await.is_ok();
            if !ok {
                return Err(S3Error::invalid_argument(
                    "Unable to validate the following destination configurations",
                )
                .with("ArgumentName1", t.arn.clone())
                .with(
                    "ArgumentValue1",
                    "The destination does not exist or is not reachable",
                ));
            }
        }
    }
    svc.put_config(bucket, "notification", &to_xml(&cfg)).await
}

pub fn empty_xml() -> String {
    format!("{XML_DECL}<NotificationConfiguration xmlns=\"{XMLNS}\"/>")
}

pub fn host_id(request_id: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(format!("cloudtwin/{request_id}"))
}

/// An object-level event to deliver to matching destinations.
pub struct Event<'a> {
    pub bucket: &'a str,
    pub key: &'a str,
    /// e.g. `ObjectCreated:Put`.
    pub name: &'a str,
    pub size: Option<i64>,
    pub etag: Option<&'a str>,
    pub version_id: Option<&'a str>,
    pub seq: i64,
    pub request_id: &'a str,
}

/// Deliver an event to every matching destination. Failures are only logged.
pub async fn fire(state: &Arc<AppState>, ev: Event<'_>) {
    let body = match S3Service::new(state.db.clone())
        .get_config(ev.bucket, "notification")
        .await
    {
        Ok(Some(b)) => b,
        _ => return,
    };
    let Ok(cfg) = parse(body.as_bytes()) else {
        return;
    };
    for t in cfg
        .targets
        .iter()
        .filter(|t| t.kind != DestKind::Lambda && t.matches(ev.name, ev.key))
    {
        let mut object = json!({
            "key": url_encode_plus(ev.key),
            "sequencer": format!("{:016X}", ev.seq),
        });
        if let Some(size) = ev.size {
            object["size"] = json!(size);
        }
        if let Some(etag) = ev.etag {
            object["eTag"] = json!(etag.trim_matches('"'));
        }
        if let Some(v) = ev.version_id {
            object["versionId"] = json!(v);
        }
        let record = json!({"Records": [{
            "eventVersion": "2.1",
            "eventSource": "aws:s3",
            "awsRegion": REGION,
            "eventTime": Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            "eventName": ev.name,
            "userIdentity": {"principalId": "AWS:cloudtwin"},
            "requestParameters": {"sourceIPAddress": "127.0.0.1"},
            "responseElements": {
                "x-amz-request-id": ev.request_id,
                "x-amz-id-2": host_id(ev.request_id),
            },
            "s3": {
                "s3SchemaVersion": "1.0",
                "configurationId": t.id,
                "bucket": {
                    "name": ev.bucket,
                    "ownerIdentity": {"principalId": "cloudtwin"},
                    "arn": format!("arn:aws:s3:::{}", ev.bucket),
                },
                "object": object,
            },
        }]});
        if let Err(e) = deliver(state, t, record.to_string()).await {
            warn!(bucket = ev.bucket, dest = %t.arn, "S3 event notification delivery failed: {e}");
        }
    }
}

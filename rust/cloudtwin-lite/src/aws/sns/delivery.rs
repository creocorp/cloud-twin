//! Publish validation and fan-out to subscriptions (SQS, HTTP/S, email).
//!
//! Subscriptions are auto-confirmed, so no SubscriptionConfirmation message is
//! ever sent. SQS and email deliveries complete before Publish returns; HTTP/S
//! deliveries are fire-and-forget background POSTs (failures are logged).

use std::collections::BTreeMap;
use std::sync::Arc;

use base64::Engine;
use chrono::Utc;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::filter;
use super::models::{SnsSubscription, SnsTopic};
use super::service::{Result, SnsError, SnsService};
use crate::aws::sqs::{self, MessageAttributes};
use crate::telemetry;
use crate::AppState;

const MAX_MESSAGE_BYTES: usize = 262_144;
const SIGNATURE_PLACEHOLDER: &str =
    "Y2xvdWR0d2luLWxpdGUgZG9lcyBub3Qgc2lnbiBTTlMgbWVzc2FnZXMgLSB0aGlzIGlzIGEgcGxhY2Vob2xkZXIu";

#[derive(Debug, Clone, Default)]
pub struct PublishRequest {
    pub topic_arn: Option<String>,
    pub phone_number: Option<String>,
    pub message: String,
    pub subject: Option<String>,
    pub message_structure: Option<String>,
    pub attributes: MessageAttributes,
    pub group_id: Option<String>,
    pub dedup_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PublishResult {
    pub message_id: String,
    pub sequence_number: Option<String>,
}

/// One message being fanned out.
struct Outgoing {
    message_id: String,
    topic_arn: String,
    subject: Option<String>,
    /// Per-protocol bodies (`MessageStructure=json`) or just `default`.
    bodies: BTreeMap<String, String>,
    attributes: MessageAttributes,
    timestamp: String,
    group_id: Option<String>,
    dedup_id: Option<String>,
    sequence_number: Option<String>,
}

impl Outgoing {
    fn body_for(&self, protocol: &str) -> &str {
        self.bodies
            .get(protocol)
            .or_else(|| self.bodies.get("default"))
            .map(|s| s.as_str())
            .unwrap_or("")
    }
}

fn validate_attributes(attrs: &MessageAttributes) -> Result<()> {
    for (name, a) in attrs {
        let ok = match a.data_type.as_str() {
            "String" | "String.Array" => a.string_value.is_some(),
            "Number" => a
                .string_value
                .as_deref()
                .map(|s| s.trim().parse::<f64>().is_ok())
                .unwrap_or(false),
            "Binary" => a.binary_value.is_some(),
            t if t.starts_with("String.") || t.starts_with("Number.") => a.string_value.is_some(),
            t if t.starts_with("Binary.") => a.binary_value.is_some(),
            _ => false,
        };
        if !ok {
            return Err(SnsError::invalid_value(format!(
                "The message attribute '{name}' has an invalid message attribute type, the set of supported type prefixes is Binary, Number, and String."
            )));
        }
        if a.data_type == "String.Array"
            && !matches!(
                serde_json::from_str::<Value>(a.string_value.as_deref().unwrap_or("")),
                Ok(Value::Array(_))
            )
        {
            return Err(SnsError::invalid_value(format!(
                "The message attribute '{name}' with type 'String.Array' must contain a JSON array."
            )));
        }
    }
    Ok(())
}

/// Validate and publish a message, delivering it to matching subscriptions.
pub async fn publish(state: &Arc<AppState>, req: PublishRequest) -> Result<PublishResult> {
    if req.message.is_empty() {
        return Err(SnsError::invalid("Invalid parameter: Empty message"));
    }
    let attr_bytes: usize = req
        .attributes
        .iter()
        .map(|(k, a)| k.len() + a.data_type.len() + a.value_bytes().len())
        .sum();
    if req.message.len() + attr_bytes > MAX_MESSAGE_BYTES {
        return Err(SnsError::invalid("Invalid parameter: Message too long"));
    }
    if let Some(s) = &req.subject {
        if s.is_empty() || s.chars().count() > 100 || s.chars().any(|c| c.is_control()) {
            return Err(SnsError::invalid("Invalid parameter: Subject"));
        }
    }
    validate_attributes(&req.attributes)?;

    let mut bodies = BTreeMap::new();
    match req.message_structure.as_deref() {
        Some("json") => {
            let parsed: Map<String, Value> = serde_json::from_str(&req.message).map_err(|_| {
                SnsError::invalid(
                    "Invalid parameter: Message Structure - JSON message body failed to parse",
                )
            })?;
            for (k, v) in parsed {
                let s = match v {
                    Value::String(s) => s,
                    other => other.to_string(),
                };
                bodies.insert(k, s);
            }
            if !bodies.contains_key("default") {
                return Err(SnsError::invalid(
                    "Invalid parameter: Message Structure - No default entry in JSON message body",
                ));
            }
        }
        Some(other) if !other.is_empty() => {
            return Err(SnsError::invalid("Invalid parameter: MessageStructure"));
        }
        _ => {
            bodies.insert("default".to_string(), req.message.clone());
        }
    }

    let svc = SnsService::new(state.db.clone());
    let message_id = Uuid::new_v4().to_string();
    let attrs_json = (!req.attributes.is_empty())
        .then(|| sqs::models::attrs_to_json(&req.attributes).to_string());

    let target = req.topic_arn.clone().filter(|t| !t.is_empty());
    let Some(target) = target else {
        if let Some(phone) = req.phone_number.filter(|p| !p.is_empty()) {
            tracing::info!(phone = %phone, "SNS SMS publish stored (not delivered)");
            svc.record_message(
                &message_id,
                &format!("sms:{phone}"),
                req.subject.as_deref(),
                &req.message,
                attrs_json,
                0,
            )
            .await?;
            return Ok(PublishResult {
                message_id,
                sequence_number: None,
            });
        }
        return Err(SnsError::invalid(
            "Invalid parameter: TopicArn or TargetArn Reason: no value for required parameter",
        ));
    };
    // Platform application endpoints are stored only.
    if target.contains(":endpoint/") {
        tracing::info!(target = %target, "SNS mobile push publish stored (not delivered)");
        svc.record_message(
            &message_id,
            &target,
            req.subject.as_deref(),
            &req.message,
            attrs_json,
            0,
        )
        .await?;
        return Ok(PublishResult {
            message_id,
            sequence_number: None,
        });
    }

    let topic = svc.get_topic(&target).await?;
    let (mut group_id, mut dedup_id, mut sequence_number) = (req.group_id.clone(), None, None);
    if topic.is_fifo() {
        if group_id.as_deref().unwrap_or("").is_empty() {
            return Err(SnsError::invalid(
                "Invalid parameter: The MessageGroupId parameter is required for FIFO topics",
            ));
        }
        let content_dedup = topic
            .attributes
            .get("ContentBasedDeduplication")
            .map(|v| v == "true")
            .unwrap_or(false);
        dedup_id = match req.dedup_id.clone().filter(|d| !d.is_empty()) {
            Some(d) => Some(d),
            None if content_dedup => Some(hex::encode(Sha256::digest(req.message.as_bytes()))),
            None => {
                return Err(SnsError::invalid(
                    "Invalid parameter: The topic should either have ContentBasedDeduplication enabled or MessageDeduplicationId provided explicitly",
                ))
            }
        };
        sequence_number = Some(svc.next_sequence(&topic.arn).await?);
    } else if group_id.as_deref() == Some("") {
        group_id = None;
    }

    let out = Outgoing {
        message_id: message_id.clone(),
        topic_arn: topic.arn.clone(),
        subject: req.subject.clone(),
        bodies,
        attributes: req.attributes.clone(),
        timestamp: Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        group_id,
        dedup_id,
        sequence_number: sequence_number.clone(),
    };
    let delivered = fan_out(state, &svc, &topic, &out).await?;
    svc.record_message(
        &message_id,
        &topic.arn,
        req.subject.as_deref(),
        &req.message,
        attrs_json,
        delivered,
    )
    .await?;
    Ok(PublishResult {
        message_id,
        sequence_number,
    })
}

async fn fan_out(
    state: &Arc<AppState>,
    svc: &SnsService,
    topic: &SnsTopic,
    out: &Outgoing,
) -> Result<usize> {
    let mut delivered = 0;
    for sub in svc.list_subscriptions(Some(&topic.arn)).await? {
        if !passes_filter(&sub, out) {
            continue;
        }
        let status = deliver(state, &sub, out).await;
        let ok = status.is_ok();
        if let Err(e) = &status {
            tracing::warn!(subscription = %sub.arn, protocol = %sub.protocol, error = %e, "SNS delivery failed");
        } else {
            delivered += 1;
        }
        telemetry::emit(
            &state.db,
            "aws",
            "sns",
            "deliver",
            &json!({
                "topic_arn": topic.arn,
                "subscription_arn": sub.arn,
                "protocol": sub.protocol,
                "endpoint": sub.endpoint,
                "message_id": out.message_id,
                "ok": ok,
            })
            .to_string(),
        )
        .await;
    }
    Ok(delivered)
}

fn passes_filter(sub: &SnsSubscription, out: &Outgoing) -> bool {
    let Some(policy) = sub.attributes.get("FilterPolicy") else {
        return true;
    };
    let Ok(policy) = serde_json::from_str::<Value>(policy) else {
        return true;
    };
    let scope = sub
        .attributes
        .get("FilterPolicyScope")
        .map(|s| s.as_str())
        .unwrap_or("MessageAttributes");
    filter::matches(&policy, scope, &out.attributes, out.body_for(&sub.protocol))
}

/// The standard SNS notification JSON document.
fn envelope(state: &AppState, sub: &SnsSubscription, out: &Outgoing) -> String {
    let base = state.cfg.base_url.trim_end_matches('/');
    let mut m = Map::new();
    m.insert("Type".into(), json!("Notification"));
    m.insert("MessageId".into(), json!(out.message_id));
    if let Some(seq) = &out.sequence_number {
        m.insert("SequenceNumber".into(), json!(seq));
    }
    m.insert("TopicArn".into(), json!(out.topic_arn));
    if let Some(s) = &out.subject {
        m.insert("Subject".into(), json!(s));
    }
    m.insert("Message".into(), json!(out.body_for(&sub.protocol)));
    m.insert("Timestamp".into(), json!(out.timestamp));
    m.insert("SignatureVersion".into(), json!("1"));
    m.insert("Signature".into(), json!(SIGNATURE_PLACEHOLDER));
    m.insert(
        "SigningCertURL".into(),
        json!(format!("{base}/SimpleNotificationService-cloudtwin.pem")),
    );
    m.insert(
        "UnsubscribeURL".into(),
        json!(format!(
            "{base}/?Action=Unsubscribe&SubscriptionArn={}",
            sub.arn
        )),
    );
    if !out.attributes.is_empty() {
        let attrs: Map<String, Value> = out
            .attributes
            .iter()
            .map(|(k, a)| {
                let value = if a.is_binary() {
                    base64::engine::general_purpose::STANDARD
                        .encode(a.binary_value.clone().unwrap_or_default())
                } else {
                    a.string_value.clone().unwrap_or_default()
                };
                (k.clone(), json!({"Type": a.data_type, "Value": value}))
            })
            .collect();
        m.insert("MessageAttributes".into(), Value::Object(attrs));
    }
    Value::Object(m).to_string()
}

async fn deliver(
    state: &Arc<AppState>,
    sub: &SnsSubscription,
    out: &Outgoing,
) -> anyhow::Result<()> {
    match sub.protocol.as_str() {
        "sqs" => {
            let (body, attrs) = if sub.raw_delivery() {
                (out.body_for("sqs").to_string(), out.attributes.clone())
            } else {
                (envelope(state, sub, out), MessageAttributes::new())
            };
            let res = sqs::send_with(
                state,
                &sub.endpoint,
                body.clone(),
                attrs.clone(),
                out.group_id.clone(),
                out.dedup_id.clone(),
            )
            .await;
            if let Err(e) = res {
                // Subscription dead-letter queue (RedrivePolicy) gets undeliverable messages.
                if let Some(dlq) = sub
                    .attributes
                    .get("RedrivePolicy")
                    .and_then(|r| serde_json::from_str::<Value>(r).ok())
                    .and_then(|v| v.get("deadLetterTargetArn")?.as_str().map(String::from))
                {
                    sqs::send_with(
                        state,
                        &dlq,
                        body,
                        attrs,
                        out.group_id.clone(),
                        out.dedup_id.clone(),
                    )
                    .await?;
                }
                return Err(e);
            }
            Ok(())
        }
        "http" | "https" => {
            let raw = sub.raw_delivery();
            let body = if raw {
                out.body_for(&sub.protocol).to_string()
            } else {
                envelope(state, sub, out)
            };
            let mut req = state
                .http
                .post(&sub.endpoint)
                .header("Content-Type", "text/plain; charset=UTF-8")
                .header("User-Agent", "Amazon Simple Notification Service Agent")
                .header("x-amz-sns-message-type", "Notification")
                .header("x-amz-sns-message-id", &out.message_id)
                .header("x-amz-sns-topic-arn", &out.topic_arn)
                .header("x-amz-sns-subscription-arn", &sub.arn)
                .body(body);
            if raw {
                req = req.header("x-amz-sns-rawdelivery", "true");
            }
            let endpoint = sub.endpoint.clone();
            tokio::spawn(async move {
                match req.send().await {
                    Ok(r) if r.status().is_success() => {}
                    Ok(r) => {
                        tracing::warn!(endpoint = %endpoint, status = %r.status(), "SNS HTTP delivery rejected")
                    }
                    Err(e) => {
                        tracing::warn!(endpoint = %endpoint, error = %e, "SNS HTTP delivery failed")
                    }
                }
            });
            Ok(())
        }
        "email" | "email-json" => {
            let body = if sub.protocol == "email-json" {
                envelope(state, sub, out)
            } else {
                let base = state.cfg.base_url.trim_end_matches('/');
                format!(
                    "{}\n\n--\nIf you wish to stop receiving notifications from this topic, please click or visit the link below to unsubscribe:\n{base}/?Action=Unsubscribe&SubscriptionArn={}\n",
                    out.body_for("email"),
                    sub.arn
                )
            };
            let subject = out
                .subject
                .clone()
                .unwrap_or_else(|| "AWS Notification Message".into());
            let raw = format!(
                "From: AWS Notifications <no-reply@sns.amazonaws.com>\r\nTo: {to}\r\nSubject: {subject}\r\nDate: {date}\r\nMessage-ID: <{id}@sns.amazonaws.com>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n{body}",
                to = sub.endpoint,
                date = Utc::now().to_rfc2822(),
                id = out.message_id,
                body = body.replace("\r\n", "\n").replace('\n', "\r\n"),
            );
            crate::mailbox::service::capture_raw(&state.db, raw.as_bytes(), "sns").await?;
            Ok(())
        }
        other => {
            tracing::info!(protocol = %other, endpoint = %sub.endpoint, "SNS delivery for protocol not emulated; message stored only");
            Ok(())
        }
    }
}

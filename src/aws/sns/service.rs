//! SNS persistence and validation: topics, subscriptions, tags, and the
//! published-message log. Delivery lives in `delivery.rs`.

use std::collections::BTreeMap;
use std::fmt;

use base64::Engine;
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

use super::filter;
use super::models::{SnsSubscription, SnsTopic};
use crate::aws::{ACCOUNT_ID, REGION};
use crate::db::Database;

#[derive(Debug, Clone)]
pub struct SnsError {
    pub code: &'static str,
    pub status: u16,
    pub message: String,
}

pub type Result<T> = std::result::Result<T, SnsError>;

impl SnsError {
    pub fn not_found(msg: impl Into<String>) -> Self {
        SnsError {
            code: "NotFound",
            status: 404,
            message: msg.into(),
        }
    }
    pub fn resource_not_found() -> Self {
        SnsError {
            code: "ResourceNotFound",
            status: 404,
            message: "Resource does not exist".into(),
        }
    }
    pub fn invalid(msg: impl Into<String>) -> Self {
        SnsError {
            code: "InvalidParameter",
            status: 400,
            message: msg.into(),
        }
    }
    pub fn invalid_value(msg: impl Into<String>) -> Self {
        SnsError {
            code: "ParameterValueInvalid",
            status: 400,
            message: msg.into(),
        }
    }
    pub fn batch(code: &'static str, msg: impl Into<String>) -> Self {
        SnsError {
            code,
            status: 400,
            message: msg.into(),
        }
    }
    pub fn tag_limit() -> Self {
        SnsError {
            code: "TagLimitExceeded",
            status: 400,
            message: "Could not complete request: tag quota of per resource exceeded".into(),
        }
    }
    pub fn internal(e: impl fmt::Display) -> Self {
        SnsError {
            code: "InternalError",
            status: 500,
            message: e.to_string(),
        }
    }
}

impl fmt::Display for SnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SnsError {}

impl From<anyhow::Error> for SnsError {
    fn from(e: anyhow::Error) -> Self {
        SnsError::internal(e)
    }
}

pub const PROTOCOLS: &[&str] = &[
    "http",
    "https",
    "email",
    "email-json",
    "sms",
    "sqs",
    "application",
    "lambda",
    "firehose",
];

const SUBSCRIPTION_ATTRS: &[&str] = &[
    "DeliveryPolicy",
    "FilterPolicy",
    "FilterPolicyScope",
    "RawMessageDelivery",
    "RedrivePolicy",
    "SubscriptionRoleArn",
    "ReplayPolicy",
];

const TOPIC_ATTRS: &[&str] = &[
    "DisplayName",
    "Policy",
    "DeliveryPolicy",
    "KmsMasterKeyId",
    "SignatureVersion",
    "TracingConfig",
    "FifoTopic",
    "ContentBasedDeduplication",
    "ArchivePolicy",
    "DataProtectionPolicy",
    "FifoThroughputScope",
];

pub fn topic_arn(name: &str) -> String {
    format!("arn:aws:sns:{REGION}:{ACCOUNT_ID}:{name}")
}

pub fn encode_token(s: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s.as_bytes())
}

pub fn decode_token(t: Option<&str>) -> Result<Option<String>> {
    match t.filter(|t| !t.is_empty()) {
        None => Ok(None),
        Some(t) => base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(t)
            .map(|b| Some(String::from_utf8_lossy(&b).to_string()))
            .map_err(|_| SnsError::invalid("Invalid parameter: NextToken")),
    }
}

/// Page of `items` (sorted by key) after the `after` key, at most `limit`.
pub fn page<T: Clone>(
    items: &[T],
    key: impl Fn(&T) -> &str,
    after: Option<String>,
    limit: usize,
) -> (Vec<T>, Option<String>) {
    let start = match after {
        Some(a) => items
            .iter()
            .position(|i| key(i) > a.as_str())
            .unwrap_or(items.len()),
        None => 0,
    };
    let page: Vec<T> = items.iter().skip(start).take(limit).cloned().collect();
    let token = (start + page.len() < items.len())
        .then(|| page.last().map(|l| encode_token(key(l))))
        .flatten();
    (page, token)
}

fn json_map(s: String) -> BTreeMap<String, String> {
    serde_json::from_str(&s).unwrap_or_default()
}

fn topic_from_row(r: &rusqlite::Row) -> rusqlite::Result<SnsTopic> {
    Ok(SnsTopic {
        arn: r.get(0)?,
        attributes: json_map(r.get(2)?),
        tags: json_map(r.get(3)?),
    })
}

fn sub_from_row(r: &rusqlite::Row) -> rusqlite::Result<SnsSubscription> {
    Ok(SnsSubscription {
        arn: r.get(0)?,
        topic_arn: r.get(1)?,
        protocol: r.get(2)?,
        endpoint: r.get(3)?,
        attributes: json_map(r.get(4)?),
    })
}

const SUB_COLS: &str = "arn, topic_arn, protocol, endpoint, attributes";

fn normalize_bool(name: &str, v: &str) -> Result<String> {
    match v.to_ascii_lowercase().as_str() {
        "true" => Ok("true".into()),
        "false" => Ok("false".into()),
        _ => Err(SnsError::invalid(format!("Invalid parameter: Attributes Reason: {name}: Invalid value [{v}]. Must be true or false."))),
    }
}

/// Validate and normalize one subscription attribute.
pub fn validate_subscription_attr(name: &str, value: &str) -> Result<String> {
    if !SUBSCRIPTION_ATTRS.contains(&name) {
        return Err(SnsError::invalid(format!(
            "Invalid parameter: AttributeName: {name}"
        )));
    }
    match name {
        "RawMessageDelivery" => normalize_bool(name, value),
        "FilterPolicy" if !value.is_empty() => {
            filter::validate(value).map_err(SnsError::invalid)?;
            Ok(value.to_string())
        }
        "FilterPolicyScope" => match value {
            "MessageAttributes" | "MessageBody" => Ok(value.to_string()),
            _ => Err(SnsError::invalid("Invalid parameter: Attributes Reason: FilterPolicyScope: Invalid value. Must be one of: MessageAttributes, MessageBody")),
        },
        "DeliveryPolicy" | "RedrivePolicy" if !value.is_empty() => {
            serde_json::from_str::<serde_json::Value>(value)
                .map_err(|_| SnsError::invalid(format!("Invalid parameter: {name}: failed to parse JSON.")))?;
            Ok(value.to_string())
        }
        _ => Ok(value.to_string()),
    }
}

fn validate_topic_attr(name: &str, value: &str) -> Result<String> {
    let feedback = name.ends_with("FeedbackRoleArn") || name.ends_with("FeedbackSampleRate");
    if !TOPIC_ATTRS.contains(&name) && !feedback {
        return Err(SnsError::invalid(format!(
            "Invalid parameter: AttributeName: {name}"
        )));
    }
    match name {
        "FifoTopic" | "ContentBasedDeduplication" => normalize_bool(name, value),
        "Policy" | "DeliveryPolicy" | "DataProtectionPolicy" | "ArchivePolicy"
            if !value.is_empty() =>
        {
            serde_json::from_str::<serde_json::Value>(value).map_err(|_| {
                SnsError::invalid(format!("Invalid parameter: {name}: failed to parse JSON."))
            })?;
            Ok(value.to_string())
        }
        _ => Ok(value.to_string()),
    }
}

fn valid_topic_name(name: &str) -> bool {
    let base = name.strip_suffix(".fifo").unwrap_or(name);
    !base.is_empty()
        && name.len() <= 256
        && base
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Encapsulates SNS topic and subscription persistence.
#[derive(Clone)]
pub struct SnsService {
    db: Database,
}

impl SnsService {
    pub fn new(db: Database) -> Self {
        SnsService { db }
    }

    async fn call<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut rusqlite::Connection) -> rusqlite::Result<T> + Send + 'static,
    {
        self.db.call(f).await.map_err(SnsError::internal)
    }

    // ── Topics ────────────────────────────────────────────────────────────────

    pub async fn create_topic(
        &self,
        name: &str,
        attributes: BTreeMap<String, String>,
        tags: BTreeMap<String, String>,
    ) -> Result<String> {
        if !valid_topic_name(name) {
            return Err(SnsError::invalid("Invalid parameter: Topic Name"));
        }
        let mut attrs = BTreeMap::new();
        for (k, v) in &attributes {
            attrs.insert(k.clone(), validate_topic_attr(k, v)?);
        }
        let fifo = attrs.get("FifoTopic").map(|v| v == "true").unwrap_or(false);
        if fifo != name.ends_with(".fifo") {
            return Err(SnsError::invalid(
                "Invalid parameter: Fifo Topic names must end with .fifo and must be made up of only uppercase and lowercase ASCII letters, numbers, underscores, and hyphens, and must be between 1 and 256 characters long.",
            ));
        }
        if tags.len() > 50 {
            return Err(SnsError::tag_limit());
        }
        let arn = topic_arn(name);
        if let Some(existing) = self.find_topic(&arn).await? {
            for (k, v) in &tags {
                if existing.tags.get(k).is_some_and(|cur| cur != v) {
                    return Err(SnsError::invalid(
                        "Invalid parameter: Tags Reason: Topic already exists with different tags",
                    ));
                }
            }
            return Ok(arn);
        }
        let (a, n) = (arn.clone(), name.to_string());
        let attrs_s = serde_json::to_string(&attrs).map_err(SnsError::internal)?;
        let tags_s = serde_json::to_string(&tags).map_err(SnsError::internal)?;
        self.call(move |c| {
            c.execute(
                "INSERT INTO sns_topics (arn, name, created_at, attributes, tags)
                 VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(name) DO NOTHING",
                params![a, n, Utc::now().to_rfc3339(), attrs_s, tags_s],
            )?;
            Ok(())
        })
        .await?;
        Ok(arn)
    }

    pub async fn find_topic(&self, arn: &str) -> Result<Option<SnsTopic>> {
        let a = arn.to_string();
        self.call(move |c| {
            c.query_row(
                "SELECT arn, name, attributes, tags FROM sns_topics WHERE arn = ?1",
                params![a],
                topic_from_row,
            )
            .optional()
        })
        .await
    }

    pub async fn get_topic(&self, arn: &str) -> Result<SnsTopic> {
        self.find_topic(arn)
            .await?
            .ok_or_else(|| SnsError::not_found("Topic does not exist"))
    }

    pub async fn list_topics(&self) -> Result<Vec<SnsTopic>> {
        self.call(|c| {
            let mut stmt =
                c.prepare("SELECT arn, name, attributes, tags FROM sns_topics ORDER BY arn")?;
            let rows = stmt
                .query_map([], topic_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
    }

    pub async fn delete_topic(&self, arn: &str) -> Result<()> {
        let a = arn.to_string();
        self.call(move |c| {
            let tx = c.transaction()?;
            tx.execute(
                "DELETE FROM sns_subscriptions WHERE topic_arn = ?1",
                params![a],
            )?;
            tx.execute("DELETE FROM sns_topics WHERE arn = ?1", params![a])?;
            tx.commit()
        })
        .await
    }

    pub async fn set_topic_attribute(&self, arn: &str, name: &str, value: &str) -> Result<()> {
        let topic = self.get_topic(arn).await?;
        let v = validate_topic_attr(name, value)?;
        if name == "FifoTopic" && (v == "true") != topic.is_fifo() {
            return Err(SnsError::invalid(
                "Invalid parameter: AttributeName: FifoTopic cannot be changed",
            ));
        }
        let mut attrs = topic.attributes;
        if v.is_empty() {
            attrs.remove(name);
        } else {
            attrs.insert(name.to_string(), v);
        }
        let a = arn.to_string();
        let s = serde_json::to_string(&attrs).map_err(SnsError::internal)?;
        self.call(move |c| {
            c.execute(
                "UPDATE sns_topics SET attributes = ?1 WHERE arn = ?2",
                params![s, a],
            )
            .map(|_| ())
        })
        .await
    }

    pub async fn topic_attributes(&self, arn: &str) -> Result<BTreeMap<String, String>> {
        let topic = self.get_topic(arn).await?;
        let subs = self.list_subscriptions(Some(arn)).await?;
        let mut out = BTreeMap::new();
        out.insert("TopicArn".into(), topic.arn.clone());
        out.insert("Owner".into(), ACCOUNT_ID.into());
        out.insert("DisplayName".into(), String::new());
        out.insert("SubscriptionsConfirmed".into(), subs.len().to_string());
        out.insert("SubscriptionsPending".into(), "0".into());
        out.insert("SubscriptionsDeleted".into(), "0".into());
        out.insert("Policy".into(), default_policy(&topic.arn));
        out.insert(
            "EffectiveDeliveryPolicy".into(),
            EFFECTIVE_DELIVERY_POLICY.into(),
        );
        if topic.is_fifo() {
            out.insert("FifoTopic".into(), "true".into());
            out.insert("ContentBasedDeduplication".into(), "false".into());
        }
        out.extend(topic.attributes);
        Ok(out)
    }

    pub async fn set_tags(&self, arn: &str, tags: BTreeMap<String, String>) -> Result<()> {
        if tags.len() > 50 {
            return Err(SnsError::tag_limit());
        }
        let a = arn.to_string();
        let s = serde_json::to_string(&tags).map_err(SnsError::internal)?;
        self.call(move |c| {
            c.execute(
                "UPDATE sns_topics SET tags = ?1 WHERE arn = ?2",
                params![s, a],
            )
            .map(|_| ())
        })
        .await
    }

    /// Increment and return the topic's FIFO sequence number.
    pub async fn next_sequence(&self, arn: &str) -> Result<String> {
        let a = arn.to_string();
        let seq: i64 = self
            .call(move |c| {
                c.execute(
                    "UPDATE sns_topics SET fifo_seq = fifo_seq + 1 WHERE arn = ?1",
                    params![a],
                )?;
                c.query_row(
                    "SELECT fifo_seq FROM sns_topics WHERE arn = ?1",
                    params![a],
                    |r| r.get(0),
                )
            })
            .await?;
        Ok(format!(
            "{:020}",
            10_000_000_000_000_000_000u64 + seq as u64
        ))
    }

    // ── Subscriptions ─────────────────────────────────────────────────────────

    pub async fn subscribe(
        &self,
        topic_arn: &str,
        protocol: &str,
        endpoint: &str,
        attributes: BTreeMap<String, String>,
    ) -> Result<String> {
        self.get_topic(topic_arn).await?;
        if !PROTOCOLS.contains(&protocol) {
            return Err(SnsError::invalid(format!(
                "Invalid parameter: Amazon SNS does not support this protocol string: {protocol}"
            )));
        }
        let endpoint_ok = match protocol {
            "http" => endpoint.starts_with("http://"),
            "https" => endpoint.starts_with("https://"),
            "email" | "email-json" => endpoint.contains('@'),
            "sqs" => endpoint.starts_with("arn:aws:sqs:") || endpoint.starts_with("http"),
            _ => !endpoint.is_empty(),
        };
        if !endpoint_ok {
            return Err(SnsError::invalid(format!(
                "Invalid parameter: {protocol} Endpoint"
            )));
        }
        let mut attrs = BTreeMap::new();
        for (k, v) in &attributes {
            let v = validate_subscription_attr(k, v)?;
            if !v.is_empty() {
                attrs.insert(k.clone(), v);
            }
        }
        let (ta, pr, ep) = (
            topic_arn.to_string(),
            protocol.to_string(),
            endpoint.to_string(),
        );
        let existing: Option<SnsSubscription> = self
            .call(move |c| {
                c.query_row(
                    &format!("SELECT {SUB_COLS} FROM sns_subscriptions WHERE topic_arn = ?1 AND protocol = ?2 AND endpoint = ?3"),
                    params![ta, pr, ep],
                    sub_from_row,
                )
                .optional()
            })
            .await?;
        if let Some(sub) = existing {
            if attrs.iter().any(|(k, v)| sub.attributes.get(k) != Some(v)) {
                return Err(SnsError::invalid(
                    "Invalid parameter: Attributes Reason: Subscription already exists with different attributes",
                ));
            }
            return Ok(sub.arn);
        }
        let sub_arn = format!("{topic_arn}:{}", Uuid::new_v4());
        let (sa, ta, pr, ep) = (
            sub_arn.clone(),
            topic_arn.to_string(),
            protocol.to_string(),
            endpoint.to_string(),
        );
        let attrs_s = serde_json::to_string(&attrs).map_err(SnsError::internal)?;
        self.call(move |c| {
            c.execute(
                "INSERT INTO sns_subscriptions (arn, topic_arn, protocol, endpoint, created_at, attributes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![sa, ta, pr, ep, Utc::now().to_rfc3339(), attrs_s],
            )
            .map(|_| ())
        })
        .await?;
        Ok(sub_arn)
    }

    pub async fn find_subscription(&self, arn: &str) -> Result<Option<SnsSubscription>> {
        let a = arn.to_string();
        self.call(move |c| {
            c.query_row(
                &format!("SELECT {SUB_COLS} FROM sns_subscriptions WHERE arn = ?1"),
                params![a],
                sub_from_row,
            )
            .optional()
        })
        .await
    }

    pub async fn get_subscription(&self, arn: &str) -> Result<SnsSubscription> {
        self.find_subscription(arn)
            .await?
            .ok_or_else(|| SnsError::not_found("Subscription does not exist"))
    }

    pub async fn unsubscribe(&self, arn: &str) -> Result<()> {
        if !arn.starts_with("arn:") {
            return Err(SnsError::invalid("Invalid parameter: SubscriptionArn"));
        }
        let a = arn.to_string();
        self.call(move |c| {
            c.execute("DELETE FROM sns_subscriptions WHERE arn = ?1", params![a])
                .map(|_| ())
        })
        .await
    }

    pub async fn list_subscriptions(
        &self,
        topic_arn: Option<&str>,
    ) -> Result<Vec<SnsSubscription>> {
        let ta = topic_arn.map(String::from);
        self.call(move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {SUB_COLS} FROM sns_subscriptions WHERE (?1 IS NULL OR topic_arn = ?1) ORDER BY arn"
            ))?;
            let rows = stmt.query_map(params![ta], sub_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
    }

    pub async fn set_subscription_attribute(
        &self,
        arn: &str,
        name: &str,
        value: &str,
    ) -> Result<()> {
        let sub = self.get_subscription(arn).await?;
        let v = validate_subscription_attr(name, value)?;
        let mut attrs = sub.attributes;
        if v.is_empty() {
            attrs.remove(name);
        } else {
            attrs.insert(name.to_string(), v);
        }
        let a = arn.to_string();
        let s = serde_json::to_string(&attrs).map_err(SnsError::internal)?;
        self.call(move |c| {
            c.execute(
                "UPDATE sns_subscriptions SET attributes = ?1 WHERE arn = ?2",
                params![s, a],
            )
            .map(|_| ())
        })
        .await
    }

    pub fn subscription_attributes(sub: &SnsSubscription) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        out.insert("SubscriptionArn".into(), sub.arn.clone());
        out.insert("TopicArn".into(), sub.topic_arn.clone());
        out.insert("Protocol".into(), sub.protocol.clone());
        out.insert("Endpoint".into(), sub.endpoint.clone());
        out.insert("Owner".into(), ACCOUNT_ID.into());
        out.insert("RawMessageDelivery".into(), "false".into());
        out.insert("PendingConfirmation".into(), "false".into());
        out.insert("ConfirmationWasAuthenticated".into(), "true".into());
        out.insert(
            "SubscriptionPrincipal".into(),
            format!("arn:aws:iam::{ACCOUNT_ID}:user/cloudtwin"),
        );
        if sub.protocol.starts_with("http") {
            out.insert(
                "EffectiveDeliveryPolicy".into(),
                EFFECTIVE_DELIVERY_POLICY.into(),
            );
        }
        if sub.attributes.contains_key("FilterPolicy") {
            out.insert("FilterPolicyScope".into(), "MessageAttributes".into());
        }
        out.extend(sub.attributes.clone());
        out
    }

    // ── Messages ──────────────────────────────────────────────────────────────

    pub async fn record_message(
        &self,
        message_id: &str,
        topic_arn: &str,
        subject: Option<&str>,
        body: &str,
        attributes_json: Option<String>,
        delivered: usize,
    ) -> Result<()> {
        let (mid, ta, subj, b) = (
            message_id.to_string(),
            topic_arn.to_string(),
            subject.map(String::from),
            body.to_string(),
        );
        self.call(move |c| {
            c.execute(
                "INSERT INTO sns_messages (message_id, topic_arn, subject, body, created_at, message_attributes, delivered)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![mid, ta, subj, b, Utc::now().to_rfc3339(), attributes_json, delivered as i64],
            )
            .map(|_| ())
        })
        .await
    }
}

pub fn default_policy(arn: &str) -> String {
    serde_json::json!({
        "Version": "2008-10-17",
        "Id": "__default_policy_ID",
        "Statement": [{
            "Sid": "__default_statement_ID",
            "Effect": "Allow",
            "Principal": {"AWS": "*"},
            "Action": [
                "SNS:GetTopicAttributes", "SNS:SetTopicAttributes", "SNS:AddPermission",
                "SNS:RemovePermission", "SNS:DeleteTopic", "SNS:Subscribe",
                "SNS:ListSubscriptionsByTopic", "SNS:Publish"
            ],
            "Resource": arn,
            "Condition": {"StringEquals": {"AWS:SourceOwner": ACCOUNT_ID}}
        }]
    })
    .to_string()
}

pub const EFFECTIVE_DELIVERY_POLICY: &str = r#"{"http":{"defaultHealthyRetryPolicy":{"minDelayTarget":20,"maxDelayTarget":20,"numRetries":3,"numMaxDelayRetries":0,"numNoDelayRetries":0,"numMinDelayRetries":0,"backoffFunction":"linear"},"disableSubscriptionOverrides":false,"defaultRequestPolicy":{"headerContentType":"text/plain; charset=UTF-8"}}}"#;

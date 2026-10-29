//! SQS business logic.
//!
//! Queue and message semantics live here as synchronous functions over one
//! SQLite connection (each public operation runs in a single transaction on the
//! DB thread, so receive/visibility updates are atomic). `SqsService` is the
//! async facade the handlers and internal callers use.

use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

use base64::Engine;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::errors::{Result, SqsError};
use super::models::{
    default_attr, md5_hex, md5_of_attrs, queue_arn, MsgAttr, MsgAttrs, ReceivedMessage, SendInput,
    SendOutcome, SqsQueue,
};
use crate::aws::ACCOUNT_ID;
use crate::db::Database;

/// FIFO deduplication window.
const DEDUP_WINDOW_MS: i64 = 5 * 60 * 1000;
const MAX_VISIBILITY: i64 = 43_200;
const MAX_ATTRIBUTES: usize = 10;

pub fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

/// Queue name from a queue URL, ARN, or bare name.
pub fn queue_name_of(id: &str) -> &str {
    let id = id.trim().trim_end_matches('/');
    if id.starts_with("arn:") {
        return id.rsplit(':').next().unwrap_or(id);
    }
    let id = id.split('?').next().unwrap_or(id);
    id.rsplit('/').next().unwrap_or(id)
}

/// Options for ReceiveMessage.
#[derive(Debug, Clone, Default)]
pub struct ReceiveOptions {
    pub max: i64,
    pub visibility_timeout: Option<i64>,
    pub wait_seconds: Option<i64>,
    pub system_attribute_names: Vec<String>,
    pub message_attribute_names: Vec<String>,
}

/// Implements queue and message operations on top of SQLite.
#[derive(Clone)]
pub struct SqsService {
    db: Database,
    base_url: String,
}

impl SqsService {
    pub fn new(db: Database, base_url: String) -> Self {
        SqsService {
            db,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    pub fn queue_url(&self, name: &str) -> String {
        format!("{}/{}/{}", self.base_url, ACCOUNT_ID, name)
    }

    async fn run<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        self.db
            .call(move |c| Ok(f(c)))
            .await
            .map_err(SqsError::internal)?
    }

    // ── Queues ────────────────────────────────────────────────────────────────

    pub async fn create_queue(
        &self,
        name: &str,
        attributes: BTreeMap<String, String>,
        tags: BTreeMap<String, String>,
    ) -> Result<String> {
        let n = name.to_string();
        let url = self.queue_url(name);
        let u = url.clone();
        self.run(move |c| create_queue_tx(c, &n, &u, attributes, tags))
            .await?;
        Ok(url)
    }

    pub async fn get_queue(&self, id: &str) -> Result<SqsQueue> {
        let name = queue_name_of(id).to_string();
        if name.is_empty() {
            return Err(SqsError::queue_does_not_exist());
        }
        self.run(move |c| require_queue(c, &name)).await
    }

    pub async fn get_queue_url(&self, name: &str) -> Result<String> {
        let q = self.get_queue(name).await?;
        Ok(self.queue_url(&q.name))
    }

    pub async fn list_queues(
        &self,
        prefix: &str,
        max: Option<i64>,
        next_token: Option<&str>,
    ) -> Result<(Vec<String>, Option<String>)> {
        let p = prefix.to_string();
        let names: Vec<String> = self
            .run(move |c| {
                let mut stmt = c.prepare("SELECT name FROM sqs_queues ORDER BY name")?;
                let rows = stmt
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows.into_iter().filter(|n| n.starts_with(&p)).collect())
            })
            .await?;
        let (page, token) = paginate(names, max, next_token)?;
        Ok((page.iter().map(|n| self.queue_url(n)).collect(), token))
    }

    pub async fn list_dead_letter_source_queues(
        &self,
        queue: &str,
        max: Option<i64>,
        next_token: Option<&str>,
    ) -> Result<(Vec<String>, Option<String>)> {
        let q = self.get_queue(queue).await?;
        let arn = q.arn();
        let names: Vec<String> = self
            .run(move |c| {
                Ok(all_queues(c)?
                    .into_iter()
                    .filter(|s| s.redrive().map(|(a, _)| a == arn).unwrap_or(false))
                    .map(|s| s.name)
                    .collect())
            })
            .await?;
        let (page, token) = paginate(names, max, next_token)?;
        Ok((page.iter().map(|n| self.queue_url(n)).collect(), token))
    }

    pub async fn delete_queue(&self, queue: &str) -> Result<()> {
        let q = self.get_queue(queue).await?;
        self.run(move |c| {
            let tx = c.transaction()?;
            tx.execute(
                "DELETE FROM sqs_messages WHERE queue_id = ?1",
                params![q.id],
            )?;
            tx.execute(
                "DELETE FROM sqs_fifo_dedup WHERE queue_id = ?1",
                params![q.id],
            )?;
            tx.execute("DELETE FROM sqs_queues WHERE id = ?1", params![q.id])?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    pub async fn purge_queue(&self, queue: &str) -> Result<()> {
        let q = self.get_queue(queue).await?;
        self.run(move |c| {
            c.execute(
                "DELETE FROM sqs_messages WHERE queue_id = ?1",
                params![q.id],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn get_queue_attributes(
        &self,
        queue: &str,
        names: Vec<String>,
    ) -> Result<BTreeMap<String, String>> {
        let q = self.get_queue(queue).await?;
        self.run(move |c| get_attributes_tx(c, &q, &names)).await
    }

    pub async fn set_queue_attributes(
        &self,
        queue: &str,
        attributes: BTreeMap<String, String>,
    ) -> Result<()> {
        let q = self.get_queue(queue).await?;
        self.run(move |c| {
            let normalized = validate_attributes(c, &q.name, &attributes, false)?;
            if let Some(v) = normalized.get("FifoQueue") {
                if (v == "true") != q.is_fifo() {
                    return Err(SqsError::invalid_attribute_value(
                        "Invalid value for the parameter FifoQueue. Reason: Modifying queue type is not supported.",
                    ));
                }
            }
            let mut merged = q.attributes.clone();
            for (k, v) in normalized {
                // An empty RedrivePolicy/Policy removes the setting.
                if v.is_empty() {
                    merged.remove(&k);
                } else {
                    merged.insert(k, v);
                }
            }
            c.execute(
                "UPDATE sqs_queues SET attributes = ?1, modified_ts = ?2 WHERE id = ?3",
                params![serde_json::to_string(&merged)?, now_ms() / 1000, q.id],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn tag_queue(&self, queue: &str, tags: BTreeMap<String, String>) -> Result<()> {
        let q = self.get_queue(queue).await?;
        let mut merged = q.tags.clone();
        merged.extend(tags);
        self.write_tags(q.id, merged).await
    }

    pub async fn untag_queue(&self, queue: &str, keys: Vec<String>) -> Result<()> {
        let q = self.get_queue(queue).await?;
        let mut merged = q.tags.clone();
        for k in keys {
            merged.remove(&k);
        }
        self.write_tags(q.id, merged).await
    }

    async fn write_tags(&self, id: i64, tags: BTreeMap<String, String>) -> Result<()> {
        if tags.len() > 50 {
            return Err(SqsError::invalid_parameter(
                "Too many tags added for queue.",
            ));
        }
        self.run(move |c| {
            c.execute(
                "UPDATE sqs_queues SET tags = ?1 WHERE id = ?2",
                params![serde_json::to_string(&tags)?, id],
            )?;
            Ok(())
        })
        .await
    }

    // ── Messages ──────────────────────────────────────────────────────────────

    pub async fn send(&self, queue: &str, input: SendInput) -> Result<SendOutcome> {
        let q = self.get_queue(queue).await?;
        self.run(move |c| send_tx(c, &q, input, now_ms())).await
    }

    /// Validate a whole batch payload size against the queue limit.
    pub async fn check_batch_size(&self, queue: &str, total: usize) -> Result<()> {
        let q = self.get_queue(queue).await?;
        let limit = q.attr_i64("MaximumMessageSize");
        if total as i64 > limit {
            return Err(SqsError::batch_too_long(limit));
        }
        Ok(())
    }

    /// Receive with optional long polling. Waiting happens on the async side
    /// (short DB calls every ~100 ms), never on the DB thread.
    pub async fn receive(&self, queue: &str, opts: ReceiveOptions) -> Result<Vec<ReceivedMessage>> {
        let q = self.get_queue(queue).await?;
        if !(1..=10).contains(&opts.max) {
            return Err(SqsError::invalid_parameter(format!(
                "Value {} for parameter MaxNumberOfMessages is invalid. Reason: Must be between 1 and 10, if provided.",
                opts.max
            )));
        }
        if let Some(vt) = opts.visibility_timeout {
            if !(0..=MAX_VISIBILITY).contains(&vt) {
                return Err(SqsError::invalid_parameter(format!(
                    "Value {vt} for parameter VisibilityTimeout is invalid. Reason: Must be between 0 and 43200, if provided."
                )));
            }
        }
        let wait = opts
            .wait_seconds
            .unwrap_or_else(|| q.attr_i64("ReceiveMessageWaitTimeSeconds"));
        if !(0..=20).contains(&wait) {
            return Err(SqsError::invalid_parameter(format!(
                "Value {wait} for parameter WaitTimeSeconds is invalid. Reason: Must be >= 0 and <= 20, if provided."
            )));
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(wait as u64);
        loop {
            let name = q.name.clone();
            let o = opts.clone();
            let msgs = self
                .run(move |c| {
                    let q = require_queue(c, &name)?;
                    receive_tx(c, &q, &o, now_ms())
                })
                .await?;
            let now = tokio::time::Instant::now();
            if !msgs.is_empty() || now >= deadline {
                return Ok(msgs);
            }
            tokio::time::sleep((deadline - now).min(Duration::from_millis(100))).await;
        }
    }

    pub async fn delete_message(&self, queue: &str, receipt_handle: &str) -> Result<()> {
        let q = self.get_queue(queue).await?;
        let rh = receipt_handle.to_string();
        self.run(move |c| delete_message_tx(c, &q, &rh)).await
    }

    pub async fn change_visibility(
        &self,
        queue: &str,
        receipt_handle: &str,
        timeout: i64,
    ) -> Result<()> {
        let q = self.get_queue(queue).await?;
        let rh = receipt_handle.to_string();
        self.run(move |c| change_visibility_tx(c, &q, &rh, timeout, now_ms()))
            .await
    }
}

// ── Pagination ───────────────────────────────────────────────────────────────

fn paginate(
    names: Vec<String>,
    max: Option<i64>,
    next_token: Option<&str>,
) -> Result<(Vec<String>, Option<String>)> {
    let start = match next_token.filter(|t| !t.is_empty()) {
        Some(t) => {
            let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(t)
                .map_err(|_| SqsError::invalid_parameter("Invalid NextToken value."))?;
            let after = String::from_utf8_lossy(&bytes).to_string();
            names.iter().position(|n| *n > after).unwrap_or(names.len())
        }
        None => 0,
    };
    let limit = match max {
        Some(m) if !(1..=1000).contains(&m) => {
            return Err(SqsError::invalid_parameter(format!(
                "Value {m} for parameter MaxResults is invalid. Reason: MaxResults must be an integer between 1 and 1000."
            )))
        }
        Some(m) => m as usize,
        None => 1000,
    };
    let page: Vec<String> = names.iter().skip(start).take(limit).cloned().collect();
    let more = start + page.len() < names.len();
    let token = if more && max.is_some() {
        page.last()
            .map(|l| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(l.as_bytes()))
    } else {
        None
    };
    Ok((page, token))
}

// ── Queue rows ───────────────────────────────────────────────────────────────

const QUEUE_COLS: &str = "id, name, url, attributes, tags, created_ts, modified_ts, created_at";

fn queue_from_row(r: &rusqlite::Row) -> rusqlite::Result<SqsQueue> {
    let attrs: String = r.get(3)?;
    let tags: String = r.get(4)?;
    let mut created_ts: i64 = r.get(5)?;
    if created_ts == 0 {
        let created_at: String = r.get(7)?;
        created_ts = chrono::DateTime::parse_from_rfc3339(&created_at)
            .map(|d| d.timestamp())
            .unwrap_or(0);
    }
    let modified_ts: i64 = r.get(6)?;
    Ok(SqsQueue {
        id: r.get(0)?,
        name: r.get(1)?,
        attributes: serde_json::from_str(&attrs).unwrap_or_default(),
        tags: serde_json::from_str(&tags).unwrap_or_default(),
        created_ts,
        modified_ts: if modified_ts == 0 {
            created_ts
        } else {
            modified_ts
        },
    })
}

fn find_queue(c: &Connection, name: &str) -> Result<Option<SqsQueue>> {
    Ok(c.query_row(
        &format!("SELECT {QUEUE_COLS} FROM sqs_queues WHERE name = ?1"),
        params![name],
        queue_from_row,
    )
    .optional()?)
}

fn require_queue(c: &Connection, name: &str) -> Result<SqsQueue> {
    find_queue(c, name)?.ok_or_else(SqsError::queue_does_not_exist)
}

fn all_queues(c: &Connection) -> Result<Vec<SqsQueue>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {QUEUE_COLS} FROM sqs_queues ORDER BY name"
    ))?;
    let rows = stmt
        .query_map([], queue_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn valid_queue_name(name: &str) -> bool {
    let base = name.strip_suffix(".fifo").unwrap_or(name);
    !base.is_empty()
        && name.len() <= 80
        && base
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn create_queue_tx(
    c: &mut Connection,
    name: &str,
    url: &str,
    attributes: BTreeMap<String, String>,
    tags: BTreeMap<String, String>,
) -> Result<()> {
    if !valid_queue_name(name) {
        return Err(SqsError::invalid_parameter(
            "Can only include alphanumeric characters, hyphens, or underscores. 1 to 80 in length",
        ));
    }
    let normalized = validate_attributes(c, name, &attributes, true)?;
    let fifo = normalized
        .get("FifoQueue")
        .map(|v| v == "true")
        .unwrap_or(false);
    if fifo != name.ends_with(".fifo") {
        return Err(SqsError::invalid_parameter(if fifo {
            "The name of a FIFO queue can only include alphanumeric characters, hyphens, or underscores, must end with .fifo suffix and be 1 to 80 in length."
        } else {
            "Can only include alphanumeric characters, hyphens, or underscores. 1 to 80 in length"
        }));
    }
    if let Some(existing) = find_queue(c, name)? {
        for (k, v) in &normalized {
            let current = effective_attr(&existing, k);
            if current.as_deref().unwrap_or("") != v {
                return Err(SqsError::queue_name_exists(format!(
                    "A queue already exists with the same name and a different value for attribute {k}"
                )));
            }
        }
        return Ok(());
    }
    let mut stored = normalized;
    stored.retain(|_, v| !v.is_empty());
    let now = Utc::now();
    c.execute(
        "INSERT INTO sqs_queues (name, url, created_at, attributes, tags, created_ts, modified_ts)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![
            name,
            url,
            now.to_rfc3339(),
            serde_json::to_string(&stored)?,
            serde_json::to_string(&tags)?,
            now.timestamp()
        ],
    )?;
    Ok(())
}

/// Effective value of a configurable attribute (stored value or AWS default).
fn effective_attr(q: &SqsQueue, name: &str) -> Option<String> {
    if let Some(v) = q.attributes.get(name) {
        return Some(v.clone());
    }
    if let Some(d) = default_attr(name) {
        return Some(d.to_string());
    }
    if q.is_fifo() {
        return match name {
            "ContentBasedDeduplication" => Some("false".into()),
            "DeduplicationScope" => Some("queue".into()),
            "FifoThroughputLimit" => Some("perQueue".into()),
            "FifoQueue" => Some("true".into()),
            _ => None,
        };
    }
    if name == "FifoQueue" {
        return Some("false".into());
    }
    None
}

fn parse_bool_attr(name: &str, v: &str) -> Result<String> {
    match v.to_ascii_lowercase().as_str() {
        "true" => Ok("true".into()),
        "false" => Ok("false".into()),
        _ => Err(SqsError::invalid_attribute_value(format!(
            "Invalid value for the parameter {name}."
        ))),
    }
}

fn parse_int_attr(name: &str, v: &str, min: i64, max: i64) -> Result<String> {
    match v.trim().parse::<i64>() {
        Ok(n) if (min..=max).contains(&n) => Ok(n.to_string()),
        _ => Err(SqsError::invalid_attribute_value(format!(
            "Invalid value for the parameter {name}."
        ))),
    }
}

/// Validate and normalize settable queue attributes.
fn validate_attributes(
    c: &Connection,
    queue_name: &str,
    attrs: &BTreeMap<String, String>,
    creating: bool,
) -> Result<BTreeMap<String, String>> {
    let fifo = if creating {
        attrs
            .get("FifoQueue")
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    } else {
        queue_name.ends_with(".fifo")
    };
    let mut out = BTreeMap::new();
    for (k, v) in attrs {
        let norm = match k.as_str() {
            "VisibilityTimeout" => parse_int_attr(k, v, 0, MAX_VISIBILITY)?,
            "DelaySeconds" => parse_int_attr(k, v, 0, 900)?,
            "MaximumMessageSize" => parse_int_attr(k, v, 1024, 1_048_576)?,
            "MessageRetentionPeriod" => parse_int_attr(k, v, 60, 1_209_600)?,
            "ReceiveMessageWaitTimeSeconds" => parse_int_attr(k, v, 0, 20)?,
            "KmsDataKeyReusePeriodSeconds" => parse_int_attr(k, v, 60, 86_400)?,
            "SqsManagedSseEnabled" => parse_bool_attr(k, v)?,
            "KmsMasterKeyId" => v.clone(),
            "FifoQueue" => parse_bool_attr(k, v)?,
            "ContentBasedDeduplication" | "DeduplicationScope" | "FifoThroughputLimit" if !fifo => {
                return Err(SqsError::invalid_attribute_name(format!(
                    "Unknown Attribute {k}."
                )))
            }
            "ContentBasedDeduplication" => parse_bool_attr(k, v)?,
            "DeduplicationScope" => match v.as_str() {
                "messageGroup" | "queue" => v.clone(),
                _ => {
                    return Err(SqsError::invalid_attribute_value(format!(
                        "Invalid value for the parameter {k}."
                    )))
                }
            },
            "FifoThroughputLimit" => match v.as_str() {
                "perQueue" | "perMessageGroupId" => v.clone(),
                _ => {
                    return Err(SqsError::invalid_attribute_value(format!(
                        "Invalid value for the parameter {k}."
                    )))
                }
            },
            "Policy" | "RedriveAllowPolicy" => {
                if !v.is_empty() && serde_json::from_str::<Value>(v).is_err() {
                    return Err(SqsError::invalid_attribute_value(format!(
                        "Invalid value for the parameter {k}. Reason: {k} must be valid JSON."
                    )));
                }
                v.clone()
            }
            "RedrivePolicy" => normalize_redrive(c, v, fifo)?,
            _ => {
                return Err(SqsError::invalid_attribute_name(format!(
                    "Unknown Attribute {k}."
                )))
            }
        };
        out.insert(k.clone(), norm);
    }
    Ok(out)
}

fn normalize_redrive(c: &Connection, v: &str, fifo: bool) -> Result<String> {
    if v.is_empty() {
        return Ok(String::new());
    }
    let bad = |reason: &str| {
        SqsError::invalid_parameter(format!(
            "Value {v} for parameter RedrivePolicy is invalid. Reason: {reason}"
        ))
    };
    let parsed: Value =
        serde_json::from_str(v).map_err(|_| bad("Redrive policy is not a valid JSON map."))?;
    let arn = parsed
        .get("deadLetterTargetArn")
        .and_then(|a| a.as_str())
        .ok_or_else(|| {
            bad("Redrive policy does not contain mandatory attribute: deadLetterTargetArn.")
        })?;
    let max = parsed
        .get("maxReceiveCount")
        .and_then(|m| {
            m.as_i64()
                .or_else(|| m.as_str().and_then(|s| s.trim().parse().ok()))
        })
        .ok_or_else(|| {
            bad("Redrive policy does not contain mandatory attribute: maxReceiveCount.")
        })?;
    if !(1..=1000).contains(&max) {
        return Err(bad(
            "Invalid value for maxReceiveCount: must be between 1 and 1000.",
        ));
    }
    let dlq_name = queue_name_of(arn);
    let dlq = find_queue(c, dlq_name)?.ok_or_else(|| bad("Dead letter target does not exist."))?;
    if dlq.is_fifo() != fifo {
        return Err(bad(
            "Dead-letter queue must be same type of queue as the source.",
        ));
    }
    Ok(serde_json::json!({"deadLetterTargetArn": arn, "maxReceiveCount": max}).to_string())
}

const COMPUTED_ATTRS: &[&str] = &[
    "QueueArn",
    "ApproximateNumberOfMessages",
    "ApproximateNumberOfMessagesNotVisible",
    "ApproximateNumberOfMessagesDelayed",
    "CreatedTimestamp",
    "LastModifiedTimestamp",
];

const CONFIG_ATTRS: &[&str] = &[
    "VisibilityTimeout",
    "DelaySeconds",
    "MaximumMessageSize",
    "MessageRetentionPeriod",
    "ReceiveMessageWaitTimeSeconds",
    "SqsManagedSseEnabled",
    "Policy",
    "RedrivePolicy",
    "RedriveAllowPolicy",
    "KmsMasterKeyId",
    "KmsDataKeyReusePeriodSeconds",
    "FifoQueue",
    "ContentBasedDeduplication",
    "DeduplicationScope",
    "FifoThroughputLimit",
];

fn get_attributes_tx(
    c: &mut Connection,
    q: &SqsQueue,
    names: &[String],
) -> Result<BTreeMap<String, String>> {
    for n in names {
        if n != "All"
            && !COMPUTED_ATTRS.contains(&n.as_str())
            && !CONFIG_ATTRS.contains(&n.as_str())
        {
            return Err(SqsError::invalid_attribute_name(format!(
                "Unknown Attribute {n}."
            )));
        }
    }
    let all = names.iter().any(|n| n == "All");
    let wanted = |n: &str| all || names.iter().any(|x| x == n);
    let now = now_ms();
    refresh(c, q, now)?;
    let count =
        |sql: &str| -> rusqlite::Result<i64> { c.query_row(sql, params![q.id, now], |r| r.get(0)) };
    let mut out = BTreeMap::new();
    if wanted("QueueArn") {
        out.insert("QueueArn".into(), q.arn());
    }
    if wanted("ApproximateNumberOfMessages") {
        let n =
            count("SELECT COUNT(*) FROM sqs_messages WHERE queue_id = ?1 AND visible_at <= ?2")?;
        out.insert("ApproximateNumberOfMessages".into(), n.to_string());
    }
    if wanted("ApproximateNumberOfMessagesNotVisible") {
        let n = count("SELECT COUNT(*) FROM sqs_messages WHERE queue_id = ?1 AND visible_at > ?2 AND receive_count > 0")?;
        out.insert(
            "ApproximateNumberOfMessagesNotVisible".into(),
            n.to_string(),
        );
    }
    if wanted("ApproximateNumberOfMessagesDelayed") {
        let n = count("SELECT COUNT(*) FROM sqs_messages WHERE queue_id = ?1 AND visible_at > ?2 AND receive_count = 0")?;
        out.insert("ApproximateNumberOfMessagesDelayed".into(), n.to_string());
    }
    if wanted("CreatedTimestamp") {
        out.insert("CreatedTimestamp".into(), q.created_ts.to_string());
    }
    if wanted("LastModifiedTimestamp") {
        out.insert("LastModifiedTimestamp".into(), q.modified_ts.to_string());
    }
    for n in CONFIG_ATTRS {
        if !wanted(n) {
            continue;
        }
        if !q.is_fifo()
            && matches!(
                *n,
                "FifoQueue"
                    | "ContentBasedDeduplication"
                    | "DeduplicationScope"
                    | "FifoThroughputLimit"
            )
        {
            continue;
        }
        if let Some(v) = effective_attr(q, n) {
            out.insert(n.to_string(), v);
        }
    }
    Ok(out)
}

// ── Message operations ───────────────────────────────────────────────────────

/// Purge messages older than the retention period and sync the `visible` flag
/// (`visible = 1` iff the message is receivable now) for the dashboard.
fn refresh(c: &Connection, q: &SqsQueue, now: i64) -> Result<()> {
    let retention_ms = q.attr_i64("MessageRetentionPeriod") * 1000;
    c.execute(
        "DELETE FROM sqs_messages WHERE queue_id = ?1 AND sent_at > 0 AND sent_at + ?2 < ?3",
        params![q.id, retention_ms, now],
    )?;
    c.execute(
        "UPDATE sqs_messages SET visible = (visible_at <= ?2)
         WHERE queue_id = ?1 AND visible != (visible_at <= ?2)",
        params![q.id, now],
    )?;
    c.execute(
        "DELETE FROM sqs_fifo_dedup WHERE expires_at < ?1",
        params![now],
    )?;
    Ok(())
}

/// Characters SQS accepts in a message body.
fn valid_body_char(ch: char) -> bool {
    matches!(ch as u32, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
}

fn attrs_size(attrs: &MsgAttrs) -> usize {
    attrs
        .iter()
        .map(|(k, a)| k.len() + a.data_type.len() + a.value_bytes().len())
        .sum()
}

/// Payload size of one send (body plus message attributes) as SQS counts it.
pub fn message_size(input: &SendInput) -> usize {
    input.body.len() + attrs_size(&input.attributes)
}

fn send_tx(c: &mut Connection, q: &SqsQueue, input: SendInput, now: i64) -> Result<SendOutcome> {
    if input.body.is_empty() {
        return Err(SqsError::missing_parameter("MessageBody"));
    }
    if let Some(bad) = input.body.chars().find(|ch| !valid_body_char(*ch)) {
        return Err(SqsError::invalid_message_contents(format!(
            "Invalid binary character '#x{:X}' was found in the message body, the set of allowed characters is #x9 | #xA | #xD | #x20 to #xD7FF | #xE000 to #xFFFD | #x10000 to #x10FFFF",
            bad as u32
        )));
    }
    if input.attributes.len() > MAX_ATTRIBUTES {
        return Err(SqsError::invalid_parameter(format!(
            "Number of message attributes [{}] exceeds the allowed maximum [{MAX_ATTRIBUTES}].",
            input.attributes.len()
        )));
    }
    let limit = q.attr_i64("MaximumMessageSize");
    if message_size(&input) as i64 > limit {
        return Err(SqsError::invalid_parameter(format!(
            "One or more parameters are invalid. Reason: Message must be shorter than {limit} bytes."
        )));
    }
    for k in input.system_attributes.keys() {
        if k != "AWSTraceHeader" {
            return Err(SqsError::invalid_parameter(format!(
                "Message system attribute name '{k}' is invalid."
            )));
        }
    }
    let fifo = q.is_fifo();
    let delay = match input.delay_seconds {
        Some(_) if fifo => {
            return Err(SqsError::invalid_parameter(
                "Value for parameter DelaySeconds is invalid. Reason: The request include parameter that is not valid for this queue type.",
            ))
        }
        Some(d) if !(0..=900).contains(&d) => {
            return Err(SqsError::invalid_parameter(format!(
                "Value {d} for parameter DelaySeconds is invalid. Reason: DelaySeconds must be >= 0 and <= 900."
            )))
        }
        Some(d) => d,
        None => q.attr_i64("DelaySeconds"),
    };
    let md5_body = md5_hex(input.body.as_bytes());
    let md5_attrs = (!input.attributes.is_empty()).then(|| md5_of_attrs(&input.attributes));
    let md5_system_attrs =
        (!input.system_attributes.is_empty()).then(|| md5_of_attrs(&input.system_attributes));

    let tx = c.transaction()?;
    refresh(&tx, q, now)?;
    let mut sequence_number = None;
    let mut dedup_id = None;
    if fifo {
        if input.group_id.as_deref().unwrap_or("").is_empty() {
            return Err(SqsError::missing_parameter("MessageGroupId"));
        }
        let dedup = match input.dedup_id.clone().filter(|d| !d.is_empty()) {
            Some(d) => d,
            None if q.attr_bool("ContentBasedDeduplication") => {
                hex::encode(Sha256::digest(input.body.as_bytes()))
            }
            None => {
                return Err(SqsError::invalid_parameter(
                    "The queue should either have ContentBasedDeduplication enabled or MessageDeduplicationId provided explicitly",
                ))
            }
        };
        let existing: Option<(String, String)> = tx
            .query_row(
                "SELECT message_id, sequence_number FROM sqs_fifo_dedup
                 WHERE queue_id = ?1 AND dedup_id = ?2 AND expires_at >= ?3",
                params![q.id, dedup, now],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((message_id, seq)) = existing {
            return Ok(SendOutcome {
                message_id,
                md5_body,
                md5_attrs,
                md5_system_attrs,
                sequence_number: Some(seq),
            });
        }
        tx.execute(
            "UPDATE sqs_queues SET fifo_seq = fifo_seq + 1 WHERE id = ?1",
            params![q.id],
        )?;
        let seq: i64 = tx.query_row(
            "SELECT fifo_seq FROM sqs_queues WHERE id = ?1",
            params![q.id],
            |r| r.get(0),
        )?;
        sequence_number = Some(format!(
            "{:020}",
            10_000_000_000_000_000_000u64 + seq as u64
        ));
        dedup_id = Some(dedup);
    }
    let message_id = Uuid::new_v4().to_string();
    insert_message(
        &tx,
        q.id,
        &message_id,
        &input,
        now,
        now + delay * 1000,
        dedup_id.as_deref(),
        sequence_number.as_deref(),
    )?;
    if let (Some(d), Some(s)) = (&dedup_id, &sequence_number) {
        tx.execute(
            "INSERT OR REPLACE INTO sqs_fifo_dedup (queue_id, dedup_id, message_id, sequence_number, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![q.id, d, message_id, s, now + DEDUP_WINDOW_MS],
        )?;
    }
    tx.commit()?;
    Ok(SendOutcome {
        message_id,
        md5_body,
        md5_attrs,
        md5_system_attrs,
        sequence_number,
    })
}

#[allow(clippy::too_many_arguments)]
fn insert_message(
    c: &Connection,
    queue_id: i64,
    message_id: &str,
    input: &SendInput,
    now: i64,
    visible_at: i64,
    dedup_id: Option<&str>,
    sequence_number: Option<&str>,
) -> Result<()> {
    let attrs = (!input.attributes.is_empty())
        .then(|| super::models::attrs_to_json(&input.attributes).to_string());
    let sys = (!input.system_attributes.is_empty())
        .then(|| super::models::attrs_to_json(&input.system_attributes).to_string());
    c.execute(
        "INSERT INTO sqs_messages
           (message_id, queue_id, body, receipt_handle, visible, created_at, visible_at, sent_at,
            receive_count, message_attributes, system_attributes, group_id, dedup_id, sequence_number)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10, ?11, ?12, ?13)",
        params![
            message_id,
            queue_id,
            input.body,
            format!("pending-{}", Uuid::new_v4()),
            (visible_at <= now) as i64,
            Utc::now().to_rfc3339(),
            visible_at,
            now,
            attrs,
            sys,
            input.group_id,
            dedup_id,
            sequence_number,
        ],
    )?;
    Ok(())
}

fn new_receipt_handle(queue: &str, message_id: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(
        "{queue}|{message_id}|{}|{}",
        Uuid::new_v4(),
        Uuid::new_v4().simple()
    ))
}

/// Decode a receipt handle we issued into its message id.
fn parse_receipt_handle(rh: &str) -> Option<String> {
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(rh)
        .ok()?;
    let s = String::from_utf8(raw).ok()?;
    let parts: Vec<&str> = s.split('|').collect();
    (parts.len() == 4).then(|| parts[1].to_string())
}

fn wants(names: &[String], name: &str) -> bool {
    names.iter().any(|n| n == "All" || n == name)
}

fn wants_message_attr(names: &[String], name: &str) -> bool {
    names.iter().any(|n| {
        n == "All"
            || n == ".*"
            || n == name
            || n.strip_suffix(".*")
                .map(|p| name.starts_with(&format!("{p}.")))
                .unwrap_or(false)
    })
}

struct Candidate {
    row_id: i64,
    message_id: String,
    body: String,
    receive_count: i64,
    first_receive_at: Option<i64>,
    sent_at: i64,
    message_attributes: Option<String>,
    system_attributes: Option<String>,
    group_id: Option<String>,
    dedup_id: Option<String>,
    sequence_number: Option<String>,
    dlq_source_arn: Option<String>,
}

fn receive_tx(
    c: &mut Connection,
    q: &SqsQueue,
    opts: &ReceiveOptions,
    now: i64,
) -> Result<Vec<ReceivedMessage>> {
    let tx = c.transaction()?;
    refresh(&tx, q, now)?;
    let vt = opts
        .visibility_timeout
        .unwrap_or_else(|| q.attr_i64("VisibilityTimeout"));
    let fifo = q.is_fifo();
    let dlq = match q.redrive() {
        Some((arn, max)) => find_queue(&tx, queue_name_of(&arn))?.map(|d| (d, max)),
        None => None,
    };
    let mut blocked_groups: HashSet<String> = HashSet::new();
    if fifo {
        let mut stmt = tx.prepare(
            "SELECT DISTINCT group_id FROM sqs_messages
             WHERE queue_id = ?1 AND visible_at > ?2 AND receive_count > 0 AND group_id IS NOT NULL",
        )?;
        for g in stmt.query_map(params![q.id, now], |r| r.get::<_, String>(0))? {
            blocked_groups.insert(g?);
        }
    }
    let candidates: Vec<Candidate> = {
        let mut stmt = tx.prepare(
            "SELECT id, message_id, body, receive_count, first_receive_at, sent_at, message_attributes,
                    system_attributes, group_id, dedup_id, sequence_number, dlq_source_arn
             FROM sqs_messages WHERE queue_id = ?1 AND visible_at <= ?2 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![q.id, now], |r| {
                Ok(Candidate {
                    row_id: r.get(0)?,
                    message_id: r.get(1)?,
                    body: r.get(2)?,
                    receive_count: r.get(3)?,
                    first_receive_at: r.get(4)?,
                    sent_at: r.get(5)?,
                    message_attributes: r.get(6)?,
                    system_attributes: r.get(7)?,
                    group_id: r.get(8)?,
                    dedup_id: r.get(9)?,
                    sequence_number: r.get(10)?,
                    dlq_source_arn: r.get(11)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let sys_names = &opts.system_attribute_names;
    let mut out = Vec::new();
    for m in candidates {
        if out.len() as i64 >= opts.max {
            break;
        }
        if let Some(g) = &m.group_id {
            if fifo && blocked_groups.contains(g) {
                continue;
            }
        }
        if let Some((dlq_q, max)) = &dlq {
            if m.receive_count >= *max {
                tx.execute(
                    "UPDATE sqs_messages SET queue_id = ?1, receive_count = 0, first_receive_at = NULL,
                        visible_at = ?2, visible = 1, dlq_source_arn = ?3, receipt_handle = ?4
                     WHERE id = ?5",
                    params![dlq_q.id, now, q.arn(), format!("pending-{}", Uuid::new_v4()), m.row_id],
                )?;
                if let Some(g) = &m.group_id {
                    if fifo {
                        // Later messages of this group wait until the next receive.
                        blocked_groups.insert(g.clone());
                    }
                }
                continue;
            }
        }
        let rh = new_receipt_handle(&q.name, &m.message_id);
        let count = m.receive_count + 1;
        let first = m.first_receive_at.unwrap_or(now);
        tx.execute(
            "UPDATE sqs_messages SET receipt_handle = ?1, receive_count = ?2, first_receive_at = ?3,
                visible_at = ?4, visible = ?5 WHERE id = ?6",
            params![rh, count, first, now + vt * 1000, (vt == 0) as i64, m.row_id],
        )?;

        let mut attributes = BTreeMap::new();
        let mut put = |name: &str, value: Option<String>| {
            if let Some(v) = value {
                if wants(sys_names, name) {
                    attributes.insert(name.to_string(), v);
                }
            }
        };
        put("SenderId", Some(ACCOUNT_ID.to_string()));
        put("SentTimestamp", Some(m.sent_at.to_string()));
        put("ApproximateReceiveCount", Some(count.to_string()));
        put("ApproximateFirstReceiveTimestamp", Some(first.to_string()));
        if fifo {
            put("MessageGroupId", m.group_id.clone());
            put("MessageDeduplicationId", m.dedup_id.clone());
            put("SequenceNumber", m.sequence_number.clone());
        } else {
            put("MessageGroupId", m.group_id.clone());
        }
        put("DeadLetterQueueSourceArn", m.dlq_source_arn.clone());
        let sys: MsgAttrs = m
            .system_attributes
            .as_deref()
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .and_then(|v| super::models::parse_attrs(Some(&v)).ok())
            .unwrap_or_default();
        if let Some(MsgAttr {
            string_value: Some(t),
            ..
        }) = sys.get("AWSTraceHeader")
        {
            put("AWSTraceHeader", Some(t.clone()));
        }

        let all_attrs: MsgAttrs = m
            .message_attributes
            .as_deref()
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .and_then(|v| super::models::parse_attrs(Some(&v)).ok())
            .unwrap_or_default();
        let message_attributes: MsgAttrs = all_attrs
            .into_iter()
            .filter(|(k, _)| wants_message_attr(&opts.message_attribute_names, k))
            .collect();
        out.push(ReceivedMessage {
            message_id: m.message_id,
            receipt_handle: rh,
            body: m.body,
            attributes,
            message_attributes,
        });
    }
    tx.commit()?;
    Ok(out)
}

fn delete_message_tx(c: &mut Connection, q: &SqsQueue, rh: &str) -> Result<()> {
    if rh.is_empty() {
        return Err(SqsError::missing_parameter("ReceiptHandle"));
    }
    let n = c.execute(
        "DELETE FROM sqs_messages WHERE queue_id = ?1 AND receipt_handle = ?2",
        params![q.id, rh],
    )?;
    if n == 0 && parse_receipt_handle(rh).is_none() {
        return Err(SqsError::receipt_handle_invalid(rh));
    }
    Ok(())
}

fn change_visibility_tx(
    c: &mut Connection,
    q: &SqsQueue,
    rh: &str,
    timeout: i64,
    now: i64,
) -> Result<()> {
    if rh.is_empty() {
        return Err(SqsError::missing_parameter("ReceiptHandle"));
    }
    if !(0..=MAX_VISIBILITY).contains(&timeout) {
        return Err(SqsError::invalid_parameter(format!(
            "Value {timeout} for parameter VisibilityTimeout is invalid. Reason: Must be between 0 and 43200, if provided."
        )));
    }
    let n = c.execute(
        "UPDATE sqs_messages SET visible_at = ?1, visible = ?2 WHERE queue_id = ?3 AND receipt_handle = ?4",
        params![now + timeout * 1000, (timeout == 0) as i64, q.id, rh],
    )?;
    if n > 0 {
        return Ok(());
    }
    let Some(message_id) = parse_receipt_handle(rh) else {
        return Err(SqsError::receipt_handle_invalid(rh));
    };
    let visible_at: Option<i64> = c
        .query_row(
            "SELECT visible_at FROM sqs_messages WHERE queue_id = ?1 AND message_id = ?2",
            params![q.id, message_id],
            |r| r.get(0),
        )
        .optional()?;
    match visible_at {
        Some(v) if v <= now => Err(SqsError::message_not_inflight()),
        _ => Err(SqsError::invalid_parameter(format!(
            "Value {rh} for parameter ReceiptHandle is invalid. Reason: Message does not exist or is not available for visibility timeout change."
        ))),
    }
}

/// Convert a queue identifier to its ARN without touching the DB.
pub fn arn_for(id: &str) -> String {
    queue_arn(queue_name_of(id))
}

//! SES email receiving: receipt rule sets/rules and inbound processing.
//!
//! Mail arriving on the SMTP listener (or `POST /_cloudtwin/ses/inbound`) is
//! matched against the active rule set. Recipients matched by an enabled rule
//! run that rule's actions (S3, SNS, AddHeader, Bounce, Stop, ...); unmatched
//! recipients fall through to the local capture inbox.

use std::sync::Arc;

use base64::Engine;
use chrono::Utc;
use rusqlite::OptionalExtension;
use serde_json::{json, Value};

use super::events::{self, common_headers};
use super::models::*;
use super::service::{bare_address, random_token, SesService, LOWER_ALNUM};
use crate::AppState;

/// SES rejects inbound messages over 40 MB (headers included).
pub const MAX_INBOUND_BYTES: usize = 40 * 1024 * 1024;
/// SNS actions only carry messages up to 150 KB.
const MAX_SNS_CONTENT_BYTES: usize = 150 * 1024;

fn rule_set_missing(name: &str) -> SesError {
    SesError::new(
        SesErrorKind::RuleSetDoesNotExist,
        format!("Rule set does not exist: {name}"),
    )
}

fn rule_missing(name: &str) -> SesError {
    SesError::new(
        SesErrorKind::RuleDoesNotExist,
        format!("Rule does not exist: {name}"),
    )
}

fn as_bool(v: Option<&Value>, default: bool) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
        _ => default,
    }
}

/// Normalize a rule from Query-decoded JSON (string booleans, missing lists).
pub fn normalize_rule(mut rule: Value) -> Value {
    if !rule.is_object() {
        rule = json!({});
    }
    let enabled = as_bool(rule.get("Enabled"), false);
    let scan = as_bool(rule.get("ScanEnabled"), false);
    rule["Enabled"] = json!(enabled);
    rule["ScanEnabled"] = json!(scan);
    if rule.get("TlsPolicy").and_then(|v| v.as_str()).is_none() {
        rule["TlsPolicy"] = json!("Optional");
    }
    for key in ["Recipients", "Actions"] {
        match rule.get(key) {
            Some(Value::Array(_)) => {}
            Some(Value::Object(_)) | Some(Value::String(_)) => {
                let v = rule[key].take();
                rule[key] = json!([v]);
            }
            _ => rule[key] = json!([]),
        }
    }
    rule
}

/// Does a receipt-rule recipient condition match `rcpt`?
/// `user@example.com` exact, `example.com` whole domain, `.example.com` subdomains.
pub fn recipient_matches(pattern: &str, rcpt: &str) -> bool {
    let p = pattern.trim().to_ascii_lowercase();
    let r = bare_address(rcpt).to_ascii_lowercase();
    let domain = r.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
    if p.contains('@') {
        p == r
    } else if p.starts_with('.') {
        domain.ends_with(&p)
    } else {
        domain == p
    }
}

fn rule_matches(rule: &Value, rcpt: &str) -> bool {
    match rule.get("Recipients").and_then(|v| v.as_array()) {
        Some(list) if !list.is_empty() => list
            .iter()
            .filter_map(|p| p.as_str())
            .any(|p| recipient_matches(p, rcpt)),
        _ => true,
    }
}

/// Result of processing one inbound message.
#[derive(Debug, Clone)]
pub struct InboundResult {
    pub message_id: Option<String>,
    /// Recipients handled by receipt rules.
    pub handled: Vec<String>,
    /// Recipients no enabled rule matched (go to the capture inbox).
    pub unhandled: Vec<String>,
}

impl SesService {
    // ── Rule sets ─────────────────────────────────────────────────────────

    async fn rule_set_created(&self, name: &str) -> SesResult<Option<String>> {
        let n = name.to_string();
        Ok(self
            .state
            .db
            .call(move |conn| {
                conn.query_row(
                    "SELECT created_at FROM ses_receipt_rule_sets WHERE name = ?1",
                    rusqlite::params![n],
                    |r| r.get(0),
                )
                .optional()
            })
            .await?)
    }

    pub async fn create_rule_set(&self, name: &str) -> SesResult<()> {
        if name.is_empty() {
            return Err(bad_request("RuleSetName is required."));
        }
        if self.rule_set_created(name).await?.is_some() {
            return Err(SesError::new(
                SesErrorKind::AlreadyExists,
                format!("Rule set already exists: {name}"),
            ));
        }
        let n = name.to_string();
        let now = Utc::now().to_rfc3339();
        self.state
            .db
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO ses_receipt_rule_sets (name, active, created_at) VALUES (?1, 0, ?2)",
                    rusqlite::params![n, now],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn delete_rule_set(&self, name: &str) -> SesResult<()> {
        if self.rule_set_created(name).await?.is_none() {
            return Err(rule_set_missing(name));
        }
        if self.active_rule_set_name().await?.as_deref() == Some(name) {
            return Err(SesError::new(
                SesErrorKind::CannotDelete,
                format!("Cannot delete active rule set: {name}"),
            ));
        }
        let n = name.to_string();
        self.state
            .db
            .call(move |conn| {
                conn.execute("DELETE FROM ses_receipt_rules WHERE rule_set = ?1", rusqlite::params![n])?;
                conn.execute("DELETE FROM ses_receipt_rule_sets WHERE name = ?1", rusqlite::params![n])?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// `(name, created_at)` for every rule set.
    pub async fn list_rule_sets(&self) -> SesResult<Vec<(String, String)>> {
        Ok(self
            .state
            .db
            .call(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT name, created_at FROM ses_receipt_rule_sets ORDER BY created_at, name",
                )?;
                let rows = stmt
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }

    /// `(created_at, rules in order)`.
    pub async fn describe_rule_set(&self, name: &str) -> SesResult<(String, Vec<Value>)> {
        let created = self
            .rule_set_created(name)
            .await?
            .ok_or_else(|| rule_set_missing(name))?;
        Ok((created, self.rules(name).await?))
    }

    async fn rules(&self, set: &str) -> SesResult<Vec<Value>> {
        let s = set.to_string();
        let rows: Vec<String> = self
            .state
            .db
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT rule FROM ses_receipt_rules WHERE rule_set = ?1 ORDER BY position",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![s], |r| r.get(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| serde_json::from_str(&r).ok())
            .collect())
    }

    async fn write_rules(&self, set: &str, rules: &[Value]) -> SesResult<()> {
        let s = set.to_string();
        let rows: Vec<(String, String)> = rules
            .iter()
            .map(|r| {
                (
                    r.get("Name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    r.to_string(),
                )
            })
            .collect();
        self.state
            .db
            .call(move |conn| {
                let tx = conn.transaction()?;
                tx.execute("DELETE FROM ses_receipt_rules WHERE rule_set = ?1", rusqlite::params![s])?;
                for (i, (name, rule)) in rows.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO ses_receipt_rules (rule_set, name, position, rule) VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![s, name, i as i64, rule],
                    )?;
                }
                tx.commit()?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn clone_rule_set(&self, name: &str, original: &str) -> SesResult<()> {
        let (_, rules) = self.describe_rule_set(original).await?;
        self.create_rule_set(name).await?;
        self.write_rules(name, &rules).await
    }

    async fn active_rule_set_name(&self) -> SesResult<Option<String>> {
        Ok(self
            .state
            .db
            .call(|conn| {
                conn.query_row(
                    "SELECT name FROM ses_receipt_rule_sets WHERE active = 1",
                    [],
                    |r| r.get(0),
                )
                .optional()
            })
            .await?)
    }

    /// Activate `name`, or deactivate all rule sets with `None`.
    pub async fn set_active_rule_set(&self, name: Option<&str>) -> SesResult<()> {
        if let Some(n) = name {
            if self.rule_set_created(n).await?.is_none() {
                return Err(rule_set_missing(n));
            }
        }
        let n = name.map(|s| s.to_string());
        self.state
            .db
            .call(move |conn| {
                conn.execute("UPDATE ses_receipt_rule_sets SET active = 0", [])?;
                if let Some(n) = n {
                    conn.execute(
                        "UPDATE ses_receipt_rule_sets SET active = 1 WHERE name = ?1",
                        rusqlite::params![n],
                    )?;
                }
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// `(name, created_at, rules)` of the active rule set.
    pub async fn active_rule_set(&self) -> SesResult<Option<(String, String, Vec<Value>)>> {
        match self.active_rule_set_name().await? {
            Some(name) => {
                let (created, rules) = self.describe_rule_set(&name).await?;
                Ok(Some((name, created, rules)))
            }
            None => Ok(None),
        }
    }

    // ── Rules ─────────────────────────────────────────────────────────────

    async fn validate_rule(&self, rule: &Value) -> SesResult<()> {
        let name = rule.get("Name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() {
            return Err(bad_request("Rule.Name is required."));
        }
        for action in rule["Actions"].as_array().into_iter().flatten() {
            if let Some(s3) = action.get("S3Action") {
                let bucket = s3.get("BucketName").and_then(|v| v.as_str()).unwrap_or("");
                if !self.bucket_exists(bucket).await? {
                    return Err(SesError::new(
                        SesErrorKind::InvalidS3Configuration,
                        format!("Could not write to bucket: {bucket}"),
                    ));
                }
            }
            for (key, value) in action.as_object().into_iter().flatten() {
                if let Some(topic) = value.get("TopicArn").and_then(|v| v.as_str()) {
                    if !self.topic_exists(topic).await? {
                        return Err(SesError::new(
                            SesErrorKind::InvalidSnsTopic,
                            format!("Invalid SNS topic <{topic}> in {key}."),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    async fn bucket_exists(&self, bucket: &str) -> SesResult<bool> {
        let b = bucket.to_string();
        Ok(self
            .state
            .db
            .call(move |conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM s3_buckets WHERE name = ?1",
                    rusqlite::params![b],
                    |r| r.get::<_, i64>(0),
                )
            })
            .await
            .map(|n| n > 0)
            .unwrap_or(false))
    }

    /// Insert after `after`, or first when `after` is `None` (SES semantics).
    pub async fn create_rule(&self, set: &str, rule: Value, after: Option<&str>) -> SesResult<()> {
        let rule = normalize_rule(rule);
        let (_, mut rules) = self.describe_rule_set(set).await?;
        let name = rule["Name"].as_str().unwrap_or("").to_string();
        if rules.iter().any(|r| r["Name"].as_str() == Some(name.as_str())) {
            return Err(SesError::new(
                SesErrorKind::AlreadyExists,
                format!("Rule already exists: {name}"),
            ));
        }
        self.validate_rule(&rule).await?;
        let pos = match after {
            Some(a) => {
                rules
                    .iter()
                    .position(|r| r["Name"].as_str() == Some(a))
                    .ok_or_else(|| rule_missing(a))?
                    + 1
            }
            None => 0,
        };
        rules.insert(pos, rule);
        self.write_rules(set, &rules).await
    }

    pub async fn update_rule(&self, set: &str, rule: Value) -> SesResult<()> {
        let rule = normalize_rule(rule);
        let (_, mut rules) = self.describe_rule_set(set).await?;
        let name = rule["Name"].as_str().unwrap_or("").to_string();
        let idx = rules
            .iter()
            .position(|r| r["Name"].as_str() == Some(name.as_str()))
            .ok_or_else(|| rule_missing(&name))?;
        self.validate_rule(&rule).await?;
        rules[idx] = rule;
        self.write_rules(set, &rules).await
    }

    pub async fn delete_rule(&self, set: &str, name: &str) -> SesResult<()> {
        let (_, mut rules) = self.describe_rule_set(set).await?;
        rules.retain(|r| r["Name"].as_str() != Some(name));
        self.write_rules(set, &rules).await
    }

    pub async fn describe_rule(&self, set: &str, name: &str) -> SesResult<Value> {
        let (_, rules) = self.describe_rule_set(set).await?;
        rules
            .into_iter()
            .find(|r| r["Name"].as_str() == Some(name))
            .ok_or_else(|| rule_missing(name))
    }

    pub async fn set_rule_position(&self, set: &str, name: &str, after: Option<&str>) -> SesResult<()> {
        let (_, mut rules) = self.describe_rule_set(set).await?;
        let idx = rules
            .iter()
            .position(|r| r["Name"].as_str() == Some(name))
            .ok_or_else(|| rule_missing(name))?;
        let rule = rules.remove(idx);
        let pos = match after {
            Some(a) => {
                rules
                    .iter()
                    .position(|r| r["Name"].as_str() == Some(a))
                    .ok_or_else(|| rule_missing(a))?
                    + 1
            }
            None => 0,
        };
        rules.insert(pos, rule);
        self.write_rules(set, &rules).await
    }

    pub async fn reorder_rule_set(&self, set: &str, names: &[String]) -> SesResult<()> {
        let (_, rules) = self.describe_rule_set(set).await?;
        if names.len() != rules.len() {
            return Err(bad_request("RuleNames must list every rule in the rule set."));
        }
        let mut ordered = Vec::with_capacity(rules.len());
        for n in names {
            let r = rules
                .iter()
                .find(|r| r["Name"].as_str() == Some(n.as_str()))
                .ok_or_else(|| rule_missing(n))?;
            ordered.push(r.clone());
        }
        self.write_rules(set, &ordered).await
    }

    // ── Inbound processing ────────────────────────────────────────────────

    /// Run the active receipt rule set over an inbound message.
    pub async fn receive(&self, mail_from: &str, rcpts: &[String], raw: &[u8]) -> SesResult<InboundResult> {
        if raw.len() > MAX_INBOUND_BYTES {
            return Err(SesError::new(
                SesErrorKind::MessageRejected,
                "Message exceeds the 40 MB SES receiving limit.",
            ));
        }
        let rules: Vec<Value> = match self.active_rule_set().await? {
            Some((_, _, rules)) => rules
                .into_iter()
                .filter(|r| as_bool(r.get("Enabled"), false))
                .collect(),
            None => Vec::new(),
        };
        let (handled, unhandled): (Vec<String>, Vec<String>) = rcpts
            .iter()
            .cloned()
            .partition(|r| rules.iter().any(|rule| rule_matches(rule, r)));
        if handled.is_empty() {
            return Ok(InboundResult {
                message_id: None,
                handled,
                unhandled,
            });
        }

        let now = Utc::now();
        let message_id = random_token(40, LOWER_ALNUM);
        let received = format!(
            "Return-Path: <{mail_from}>\r\n\
             Received: from cloudtwin.local (localhost [127.0.0.1]) by inbound-smtp.{region}.amazonaws.com with SMTP id {message_id} for {first}; {date}\r\n\
             X-SES-Spam-Verdict: PASS\r\n\
             X-SES-Virus-Verdict: PASS\r\n\
             Received-SPF: none (spfCheck: cloudtwin.local) client-ip=127.0.0.1;\r\n\
             Authentication-Results: amazonses.com; spf=none; dkim=none; dmarc=none\r\n\
             X-SES-RECEIPT: cloudtwin\r\n",
            region = crate::aws::REGION,
            first = handled[0],
            date = now.format("%a, %d %b %Y %H:%M:%S +0000"),
        );
        let mut message = received.into_bytes();
        message.extend_from_slice(raw);

        'rules: for rule in &rules {
            let matched: Vec<String> = handled
                .iter()
                .filter(|r| rule_matches(rule, r))
                .map(|r| bare_address(r))
                .collect();
            if matched.is_empty() {
                continue;
            }
            for action in rule["Actions"].as_array().into_iter().flatten() {
                let stop = self
                    .run_action(action, &mut message, mail_from, rcpts, &matched, &message_id)
                    .await;
                if stop {
                    break 'rules;
                }
            }
        }

        crate::telemetry::emit(
            &self.state.db,
            "aws",
            "ses",
            "receive_email",
            &json!({"message_id": message_id, "recipients": handled}).to_string(),
        )
        .await;

        Ok(InboundResult {
            message_id: Some(message_id),
            handled,
            unhandled,
        })
    }

    /// Execute one receipt action. Returns `true` for a StopAction.
    async fn run_action(
        &self,
        action: &Value,
        message: &mut Vec<u8>,
        mail_from: &str,
        destination: &[String],
        matched: &[String],
        message_id: &str,
    ) -> bool {
        let notify = |msg: &[u8], action_json: Value, content: Option<String>| {
            let headers = events::parse_headers(msg);
            let mut n = json!({
                "notificationType": "Received",
                "mail": {
                    "timestamp": iso_ms(Utc::now()),
                    "source": mail_from,
                    "messageId": message_id,
                    "destination": destination,
                    "headersTruncated": false,
                    "headers": headers.iter().map(|(k, v)| json!({"name": k, "value": v})).collect::<Vec<_>>(),
                    "commonHeaders": common_headers(&headers, message_id),
                },
                "receipt": {
                    "timestamp": iso_ms(Utc::now()),
                    "processingTimeMillis": 42,
                    "recipients": matched,
                    "spamVerdict": {"status": "PASS"},
                    "virusVerdict": {"status": "PASS"},
                    "spfVerdict": {"status": "PASS"},
                    "dkimVerdict": {"status": "PASS"},
                    "dmarcVerdict": {"status": "PASS"},
                    "action": action_json,
                },
            });
            if let Some(c) = content {
                n["content"] = json!(c);
            }
            n
        };

        if let Some(a) = action.get("AddHeaderAction") {
            let name = a.get("HeaderName").and_then(|v| v.as_str()).unwrap_or("");
            let value = a.get("HeaderValue").and_then(|v| v.as_str()).unwrap_or("");
            if !name.is_empty() {
                let mut m = format!("{name}: {value}\r\n").into_bytes();
                m.extend_from_slice(message);
                *message = m;
            }
            return false;
        }

        if let Some(a) = action.get("S3Action") {
            let bucket = a.get("BucketName").and_then(|v| v.as_str()).unwrap_or("");
            let prefix = a.get("ObjectKeyPrefix").and_then(|v| v.as_str()).unwrap_or("");
            let key = format!("{prefix}{message_id}");
            if let Err(e) = crate::aws::s3::put_object_internal(
                &self.state,
                bucket,
                &key,
                message.clone(),
                "application/octet-stream",
                Vec::new(),
            )
            .await
            {
                tracing::warn!("SES S3 action failed writing s3://{bucket}/{key}: {e}");
                return false;
            }
            if let Some(topic) = a.get("TopicArn").and_then(|v| v.as_str()) {
                let mut act = json!({"type": "S3", "topicArn": topic, "bucketName": bucket, "objectKey": key});
                if !prefix.is_empty() {
                    act["objectKeyPrefix"] = json!(prefix);
                }
                self.notify(topic, notify(message, act, None)).await;
            }
            return false;
        }

        if let Some(a) = action.get("SNSAction") {
            let topic = a.get("TopicArn").and_then(|v| v.as_str()).unwrap_or("");
            let encoding = a.get("Encoding").and_then(|v| v.as_str()).unwrap_or("UTF-8");
            if message.len() > MAX_SNS_CONTENT_BYTES {
                tracing::warn!("SES SNS action skipped: message exceeds 150 KB");
                return false;
            }
            let (enc, content) = if encoding.eq_ignore_ascii_case("base64") {
                ("BASE64", base64::engine::general_purpose::STANDARD.encode(&message))
            } else {
                ("UTF8", String::from_utf8_lossy(message).to_string())
            };
            let n = notify(message, json!({"type": "SNS", "topicArn": topic, "encoding": enc}), Some(content));
            self.notify(topic, n).await;
            return false;
        }

        if let Some(a) = action.get("BounceAction") {
            let sender = a.get("Sender").and_then(|v| v.as_str()).unwrap_or("MAILER-DAEMON@amazonses.com");
            let text = a.get("Message").and_then(|v| v.as_str()).unwrap_or("Mailbox does not exist");
            let code = a.get("SmtpReplyCode").and_then(|v| v.as_str()).unwrap_or("550");
            let status = a.get("StatusCode").and_then(|v| v.as_str()).unwrap_or("5.1.1");
            let raw = format!(
                "From: {sender}\r\nTo: {mail_from}\r\nSubject: Delivery Status Notification (Failure)\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n\
                 Your message to {} was rejected: {code} {status} {text}\r\n",
                matched.join(", ")
            );
            if let Err(e) = crate::mailbox::service::capture_raw(&self.state.db, raw.as_bytes(), "ses-bounce").await {
                tracing::warn!("SES bounce action capture failed: {e}");
            }
            if let Some(topic) = a.get("TopicArn").and_then(|v| v.as_str()) {
                let act = json!({"type": "Bounce", "topicArn": topic, "smtpReplyCode": code, "statusCode": status, "message": text, "sender": sender});
                self.notify(topic, notify(message, act, None)).await;
            }
            return false;
        }

        for (kind, label) in [("LambdaAction", "Lambda"), ("WorkmailAction", "WorkMail"), ("StopAction", "Stop")] {
            if let Some(a) = action.get(kind) {
                if kind != "StopAction" {
                    tracing::info!("SES receipt {label} action is not executed locally");
                }
                if let Some(topic) = a.get("TopicArn").and_then(|v| v.as_str()) {
                    let mut act = json!({"type": label, "topicArn": topic});
                    if let Some(f) = a.get("FunctionArn") {
                        act["functionArn"] = f.clone();
                        act["invocationType"] = a.get("InvocationType").cloned().unwrap_or(json!("Event"));
                    }
                    if let Some(o) = a.get("OrganizationArn") {
                        act["organizationArn"] = o.clone();
                    }
                    self.notify(topic, notify(message, act, None)).await;
                }
                return kind == "StopAction";
            }
        }
        false
    }

    async fn notify(&self, topic: &str, notification: Value) {
        if let Err(e) = crate::aws::sns::publish_internal(
            &self.state,
            topic,
            notification.to_string(),
            Some("Amazon SES Email Receipt Notification".into()),
            None,
        )
        .await
        {
            tracing::warn!("SES receipt notification to {topic} failed: {e}");
        }
    }
}

/// Entry point for the SMTP listener and the inbound control endpoint.
pub async fn receive_inbound(
    state: &Arc<AppState>,
    mail_from: &str,
    rcpts: &[String],
    raw: &[u8],
) -> SesResult<InboundResult> {
    SesService::new(state.clone()).receive(mail_from, rcpts, raw).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipient_conditions() {
        assert!(recipient_matches("a@x.test", "A@X.test"));
        assert!(!recipient_matches("a@x.test", "b@x.test"));
        assert!(recipient_matches("x.test", "anyone@x.test"));
        assert!(!recipient_matches("x.test", "anyone@sub.x.test"));
        assert!(recipient_matches(".x.test", "anyone@sub.x.test"));
        assert!(!recipient_matches(".x.test", "anyone@x.test"));
        assert!(recipient_matches("x.test", "Name <anyone@x.test>"));
    }

    #[test]
    fn rules_are_normalized() {
        let r = normalize_rule(json!({"Name": "r", "Enabled": "true", "Recipients": "x.test"}));
        assert_eq!(r["Enabled"], json!(true));
        assert_eq!(r["ScanEnabled"], json!(false));
        assert_eq!(r["TlsPolicy"], json!("Optional"));
        assert_eq!(r["Recipients"], json!(["x.test"]));
        assert_eq!(r["Actions"], json!([]));
    }
}

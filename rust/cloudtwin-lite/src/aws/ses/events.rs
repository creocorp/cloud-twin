//! SES event and receipt-notification JSON, shaped like the payloads SES
//! publishes to SNS.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use super::models::iso_ms;

/// Map a configuration-set `MatchingEventTypes` value (v2 `SEND` or v1 `send`)
/// to the `eventType` name used in published events.
pub fn event_type_name(api: &str) -> Option<&'static str> {
    match api.to_ascii_uppercase().replace(['_', ' '], "").as_str() {
        "SEND" => Some("Send"),
        "REJECT" => Some("Reject"),
        "BOUNCE" => Some("Bounce"),
        "COMPLAINT" => Some("Complaint"),
        "DELIVERY" => Some("Delivery"),
        "OPEN" => Some("Open"),
        "CLICK" => Some("Click"),
        "RENDERINGFAILURE" => Some("Rendering Failure"),
        "DELIVERYDELAY" => Some("DeliveryDelay"),
        "SUBSCRIPTION" => Some("Subscription"),
        _ => None,
    }
}

/// Key holding the per-type detail object in an event.
fn detail_key(event_type: &str) -> &'static str {
    match event_type {
        "Send" => "send",
        "Reject" => "reject",
        "Bounce" => "bounce",
        "Complaint" => "complaint",
        "Delivery" => "delivery",
        "Open" => "open",
        "Click" => "click",
        "Rendering Failure" => "failure",
        "DeliveryDelay" => "deliveryDelay",
        _ => "subscription",
    }
}

/// Everything needed to build the `mail` object of an event or notification.
#[derive(Debug, Clone)]
pub struct MailContext {
    pub timestamp: DateTime<Utc>,
    pub message_id: String,
    pub source: String,
    pub source_arn: Option<String>,
    pub destination: Vec<String>,
    pub headers: Vec<(String, String)>,
    pub tags: BTreeMap<String, Vec<String>>,
}

/// Split a raw RFC 822 message into unfolded `(name, value)` headers.
pub fn parse_headers(raw: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(raw);
    let head = text
        .split("\r\n\r\n")
        .next()
        .map(|h| h.split("\n\n").next().unwrap_or(h))
        .unwrap_or("");
    let mut out: Vec<(String, String)> = Vec::new();
    for line in head.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = out.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            out.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    out
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn split_addresses(v: &str) -> Vec<String> {
    v.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// `commonHeaders` derived from the message headers.
pub fn common_headers(headers: &[(String, String)], message_id: &str) -> Value {
    let mut ch = serde_json::Map::new();
    if let Some(v) = header(headers, "Return-Path") {
        ch.insert("returnPath".into(), json!(v.trim_matches(['<', '>'])));
    }
    for (h, key) in [("From", "from"), ("Reply-To", "replyTo"), ("To", "to"), ("Cc", "cc"), ("Bcc", "bcc")] {
        if let Some(v) = header(headers, h) {
            ch.insert(key.into(), json!(split_addresses(v)));
        }
    }
    if let Some(v) = header(headers, "Date") {
        ch.insert("date".into(), json!(v));
    }
    ch.insert("messageId".into(), json!(message_id));
    if let Some(v) = header(headers, "Subject") {
        ch.insert("subject".into(), json!(v));
    }
    Value::Object(ch)
}

impl MailContext {
    /// The `mail` object for configuration-set events.
    pub fn mail_object(&self) -> Value {
        let mut m = json!({
            "timestamp": iso_ms(self.timestamp),
            "source": self.source,
            "sendingAccountId": crate::aws::ACCOUNT_ID,
            "messageId": self.message_id,
            "destination": self.destination,
            "headersTruncated": false,
            "headers": self.headers.iter().map(|(n, v)| json!({"name": n, "value": v})).collect::<Vec<_>>(),
            "commonHeaders": common_headers(&self.headers, &self.message_id),
            "tags": self.tags,
        });
        if let Some(arn) = &self.source_arn {
            m["sourceArn"] = json!(arn);
        }
        m
    }

    /// A configuration-set event of `event_type` ("Send", "Bounce", ...).
    pub fn event(&self, event_type: &str, detail: Value) -> Value {
        let mut e = json!({
            "eventType": event_type,
            "mail": self.mail_object(),
        });
        e[detail_key(event_type)] = detail;
        e
    }
}

pub fn feedback_id() -> String {
    format!(
        "{}-{}-000000",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

pub fn delivery_detail(recipients: &[String], now: DateTime<Utc>) -> Value {
    json!({
        "timestamp": iso_ms(now),
        "processingTimeMillis": 120,
        "recipients": recipients,
        "smtpResponse": "250 2.6.0 Message received",
        "reportingMTA": "a8-50.smtp-out.amazonses.com",
        "remoteMtaIp": "127.0.0.1",
    })
}

pub fn bounce_detail(
    recipients: &[String],
    bounce_type: &str,
    sub_type: &str,
    diagnostic: &str,
    now: DateTime<Utc>,
) -> Value {
    json!({
        "feedbackId": feedback_id(),
        "bounceType": bounce_type,
        "bounceSubType": sub_type,
        "bouncedRecipients": recipients.iter().map(|r| json!({
            "emailAddress": r,
            "action": "failed",
            "status": "5.1.1",
            "diagnosticCode": diagnostic,
        })).collect::<Vec<_>>(),
        "timestamp": iso_ms(now),
        "reportingMTA": "dns; amazonses.com",
    })
}

pub fn complaint_detail(recipients: &[String], now: DateTime<Utc>) -> Value {
    json!({
        "feedbackId": feedback_id(),
        "complaintSubType": Value::Null,
        "complainedRecipients": recipients.iter().map(|r| json!({"emailAddress": r})).collect::<Vec<_>>(),
        "timestamp": iso_ms(now),
        "userAgent": "Amazon SES Mailbox Simulator",
        "complaintFeedbackType": "abuse",
        "arrivalDate": iso_ms(now),
    })
}

pub fn delivery_delay_detail(recipients: &[String], now: DateTime<Utc>) -> Value {
    json!({
        "timestamp": iso_ms(now),
        "delayType": "TransientCommunicationFailure",
        "expirationTime": iso_ms(now + chrono::Duration::hours(14)),
        "delayedRecipients": recipients.iter().map(|r| json!({
            "emailAddress": r,
            "status": "4.4.7",
            "diagnosticCode": "smtp; 421 4.4.7 Unable to connect to recipient server",
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_unfold_and_common_headers() {
        let raw = b"From: A <a@x.test>\r\nTo: b@y.test,\r\n c@y.test\r\nSubject: Hi\r\n\r\nbody: not a header";
        let h = parse_headers(raw);
        assert_eq!(h.len(), 3);
        let ch = common_headers(&h, "m1");
        assert_eq!(ch["to"], json!(["b@y.test", "c@y.test"]));
        assert_eq!(ch["from"], json!(["A <a@x.test>"]));
        assert_eq!(ch["subject"], "Hi");
        assert_eq!(event_type_name("DELIVERY_DELAY"), Some("DeliveryDelay"));
        assert_eq!(event_type_name("renderingFailure"), Some("Rendering Failure"));
        assert_eq!(event_type_name("nope"), None);
    }
}

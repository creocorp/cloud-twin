//! GCP Pub/Sub models.

use chrono::{DateTime, SecondsFormat, Utc};

#[derive(Debug, Clone)]
pub struct PubsubTopic {
    pub name: String,
    #[allow(dead_code)]
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct PubsubSubscription {
    pub name: String,
    pub topic: String,
    pub ack_deadline_seconds: i64,
    #[allow(dead_code)]
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct PubsubMessage {
    pub message_id: String,
    #[allow(dead_code)]
    pub topic: String,
    pub data: String,       // base64-encoded
    pub attributes: String, // JSON object string
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct PubsubAckable {
    pub ack_id: String,
    #[allow(dead_code)]
    pub subscription: String,
    #[allow(dead_code)]
    pub message_id: String,
}

/// Current time as an RFC 3339 UTC timestamp with a `Z` suffix.
pub fn now_ts() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// Normalise a stored timestamp to the RFC 3339 `...Z` form protobuf JSON uses.
pub fn rfc3339_z(ts: &str) -> String {
    DateTime::parse_from_rfc3339(ts)
        .map(|d| {
            d.with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Micros, true)
        })
        .unwrap_or_else(|_| ts.to_string())
}

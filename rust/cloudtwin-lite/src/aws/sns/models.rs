//! SNS topic, subscription, and message models.
#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct SnsTopic {
    pub arn: String,
    pub name: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct SnsSubscription {
    pub arn: String,
    pub topic_arn: String,
    pub protocol: String,
    pub endpoint: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct SnsMessage {
    pub message_id: String,
    pub topic_arn: String,
    pub subject: Option<String>,
    pub body: String,
    pub created_at: String,
}

//! Azure Service Bus runtime models.
#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct AsbQueue {
    pub name: String,
    pub namespace: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct AsbTopic {
    pub name: String,
    pub namespace: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct AsbSubscription {
    pub name: String,
    pub topic: String,
    pub namespace: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct AsbMessage {
    pub message_id: String,
    pub namespace: String,
    pub queue_name: String,
    pub body: String,
    pub lock_token: String,
    pub visible: bool,
    pub created_at: String,
}

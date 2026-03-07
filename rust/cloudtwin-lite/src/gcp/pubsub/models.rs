#[derive(Debug, Clone)]
pub struct PubsubTopic {
    pub name:       String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct PubsubSubscription {
    pub name:       String,
    pub topic:      String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct PubsubMessage {
    pub message_id:  String,
    pub topic:       String,
    pub data:        String, // base64-encoded
    pub attributes:  String, // JSON object string
    pub created_at:  String,
}

#[derive(Debug, Clone)]
pub struct PubsubAckable {
    pub ack_id:       String,
    pub subscription: String,
    pub message_id:   String,
    pub acked:        bool,
    pub created_at:   String,
}

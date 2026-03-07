#[derive(Debug, Clone)]
pub struct SqsQueue {
    pub id:         i64,
    pub name:       String,
    pub url:        String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct SqsMessage {
    pub message_id:     String,
    pub queue_id:       i64,
    pub body:           String,
    pub receipt_handle: String,
    pub visible:        bool,
    pub created_at:     String,
}

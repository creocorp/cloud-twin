//! Mailbox data models — captured email attachments.

/// A single attachment extracted from a captured message.
#[derive(Debug, Clone)]
pub struct MailboxAttachment {
    pub filename: String,
    pub content_type: String,
    pub size: i64,
    pub content: Vec<u8>,
}

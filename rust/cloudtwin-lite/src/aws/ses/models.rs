//! SES runtime models.
#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct SesIdentity {
    pub identity: String,
    pub kind: String, // "email" | "domain"
    pub verified: bool,
    pub token: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct SesMessage {
    pub message_id: String,
    pub source: String,
    pub destinations: Vec<String>,
    pub subject: String,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub status: String,
    pub created_at: String,
}

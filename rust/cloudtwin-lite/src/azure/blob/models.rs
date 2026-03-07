//! Azure Blob Storage models.
//!
//! These map closely to persisted rows and are intentionally lightweight.
#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct AzureContainer {
    pub name: String,
    pub account: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct AzureBlob {
    pub container: String,
    pub account: String,
    pub name: String,
    pub content: Vec<u8>,
    pub content_type: String,
    pub size: i64,
    pub etag: String,
    pub created_at: String,
}

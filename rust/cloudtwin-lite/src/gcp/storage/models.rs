//! GCP Cloud Storage models.
#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct GcsBucket {
    pub name: String,
    pub project: String,
    pub location: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct GcsObject {
    pub bucket: String,
    pub name: String,
    pub content: Vec<u8>,
    pub content_type: String,
    pub size: i64,
    pub etag: String,
    pub created_at: String,
}

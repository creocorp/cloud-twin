//! S3 bucket and object structs.
//!
//! These structs represent the runtime's persisted view of S3 state.

/// S3-compatible data models.

#[derive(Debug, Clone)]
pub struct Bucket {
    pub name: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct Object {
    pub bucket: String,
    pub key: String,
    pub content: Vec<u8>,
    pub content_type: String,
    pub size: i64,
    pub etag: String,
    pub created_at: String,
}

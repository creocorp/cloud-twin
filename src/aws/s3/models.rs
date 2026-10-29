//! S3 bucket and object structs persisted in SQLite.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct Bucket {
    pub name: String,
    pub created_at: String,
}

/// Object attributes besides content/type/size/etag, stored as JSON in the
/// `meta` column.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ObjMeta {
    /// User metadata (`x-amz-meta-<name>`), names lowercase without prefix.
    #[serde(default)]
    pub user: Vec<(String, String)>,
    /// Standard representation headers (content-encoding, cache-control, ...).
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// Server-side-encryption related response headers.
    #[serde(default)]
    pub sse: Vec<(String, String)>,
    /// `x-amz-checksum-*` headers supplied on upload.
    #[serde(default)]
    pub checksums: Vec<(String, String)>,
    /// Part sizes for objects assembled by CompleteMultipartUpload.
    #[serde(default)]
    pub parts: Vec<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website_redirect: Option<String>,
}

impl ObjMeta {
    pub fn from_json(s: &str) -> Self {
        serde_json::from_str(s).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }

    pub fn storage_class(&self) -> &str {
        self.storage_class.as_deref().unwrap_or("STANDARD")
    }
}

pub type Tags = Vec<(String, String)>;

pub fn tags_from_json(s: &str) -> Tags {
    serde_json::from_str(s).unwrap_or_default()
}

pub fn tags_to_json(t: &Tags) -> String {
    serde_json::to_string(t).unwrap_or_else(|_| "[]".into())
}

/// One object version's attributes (content is loaded separately).
#[derive(Debug, Clone)]
pub struct ObjectInfo {
    pub bucket: String,
    pub key: String,
    pub version_id: String,
    pub seq: i64,
    pub is_delete_marker: bool,
    pub content_type: String,
    pub size: i64,
    pub etag: String,
    pub created_at: String,
    pub meta: ObjMeta,
    pub tags: Tags,
    pub is_latest: bool,
}

/// An object about to be written.
#[derive(Debug, Clone, Default)]
pub struct NewObject {
    pub content: Vec<u8>,
    pub content_type: String,
    pub etag: String,
    pub meta: ObjMeta,
    pub tags: Tags,
}

/// Bucket versioning state: `""` (never configured), `Enabled`, `Suspended`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Versioning {
    Unversioned,
    Enabled,
    Suspended,
}

impl Versioning {
    pub fn parse(s: &str) -> Self {
        match s {
            "Enabled" => Versioning::Enabled,
            "Suspended" => Versioning::Suspended,
            _ => Versioning::Unversioned,
        }
    }

    pub fn is_configured(self) -> bool {
        self != Versioning::Unversioned
    }
}

#[derive(Debug, Clone)]
pub struct WriteResult {
    pub version_id: String,
    pub seq: i64,
    pub etag: String,
    pub size: i64,
    pub created_at: String,
    pub versioning: Versioning,
    pub meta: ObjMeta,
}

#[derive(Debug, Clone)]
pub struct DeleteOutcome {
    /// Version id that was removed or created (delete marker).
    pub version_id: Option<String>,
    pub delete_marker: bool,
    pub seq: i64,
    pub versioning: Versioning,
}

/// A row of a key listing (ListObjects / ListObjectsV2).
#[derive(Debug, Clone)]
pub struct ListEntry {
    pub key: String,
    pub size: i64,
    pub etag: String,
    pub created_at: String,
    pub storage_class: String,
}

#[derive(Debug, Clone, Default)]
pub struct ListPage {
    pub contents: Vec<ListEntry>,
    pub common_prefixes: Vec<String>,
    pub truncated: bool,
    /// Last key or common prefix returned (paging marker).
    pub last: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VersionEntry {
    pub key: String,
    pub version_id: String,
    pub is_delete_marker: bool,
    pub is_latest: bool,
    pub size: i64,
    pub etag: String,
    pub created_at: String,
    pub storage_class: String,
}

#[derive(Debug, Clone, Default)]
pub struct VersionPage {
    pub entries: Vec<VersionEntry>,
    pub common_prefixes: Vec<String>,
    pub truncated: bool,
    pub next_key_marker: Option<String>,
    pub next_version_marker: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Upload {
    pub upload_id: String,
    pub bucket: String,
    pub key: String,
    pub content_type: String,
    pub meta: ObjMeta,
    pub tags: Tags,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct Part {
    pub part_number: i64,
    pub size: i64,
    pub etag: String,
    pub created_at: String,
    pub meta: ObjMeta,
}

//! GCP Cloud Storage business logic.
//!
//! Conceptually this mirrors the S3 service: buckets and objects are persisted
//! in SQLite, while HTTP and JSON concerns stay in the handler layer.

use std::collections::HashSet;

use base64::Engine;
use hex::encode as hex_encode;
use md5::{Digest, Md5};
use rusqlite::OptionalExtension;

use super::models::{now_ts, GcsBucket, GcsObject};
use crate::db::Database;

/// Domain errors, mapped onto Google JSON API errors by the handlers.
#[derive(Debug)]
pub enum GcsError {
    BucketNotFound(String),
    BucketExists(String),
    BucketNotEmpty(String),
    ObjectNotFound(String, String),
    PreconditionFailed(String),
    Internal(String),
}

impl From<anyhow::Error> for GcsError {
    fn from(e: anyhow::Error) -> Self {
        GcsError::Internal(e.to_string())
    }
}

pub type GcsResult<T> = Result<T, GcsError>;

/// Options for listing objects (`GET /storage/v1/b/{bucket}/o`).
#[derive(Debug, Default, Clone)]
pub struct ListQuery {
    pub prefix: String,
    pub delimiter: Option<String>,
    pub start_offset: Option<String>,
    pub end_offset: Option<String>,
    pub max_results: Option<usize>,
    pub page_token: Option<String>,
}

#[derive(Debug, Default)]
pub struct ObjectListing {
    pub items: Vec<GcsObject>,
    pub prefixes: Vec<String>,
    pub next_page_token: Option<String>,
}

const OBJ_COLS: &str = "bucket, name, content, content_type, size, etag, created_at, props";

fn row_to_object(r: &rusqlite::Row<'_>) -> rusqlite::Result<GcsObject> {
    let props: String = r.get(7)?;
    Ok(GcsObject {
        bucket: r.get(0)?,
        name: r.get(1)?,
        content: r.get(2)?,
        content_type: r.get(3)?,
        size: r.get(4)?,
        etag: r.get(5)?,
        created_at: r.get(6)?,
        props: serde_json::from_str(&props).unwrap_or_else(|_| serde_json::json!({})),
    })
}

fn row_to_bucket(r: &rusqlite::Row<'_>) -> rusqlite::Result<GcsBucket> {
    Ok(GcsBucket {
        name: r.get(0)?,
        project: r.get(1)?,
        location: r.get(2)?,
        created_at: r.get(3)?,
    })
}

fn bucket_exists(conn: &rusqlite::Connection, name: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT 1 FROM gcs_buckets WHERE name=?1",
        [name],
        |_| Ok(()),
    )
    .optional()
    .map(|o| o.is_some())
}

fn load_object(
    conn: &rusqlite::Connection,
    bucket: &str,
    name: &str,
) -> rusqlite::Result<Option<GcsObject>> {
    conn.query_row(
        &format!("SELECT {OBJ_COLS} FROM gcs_objects WHERE bucket=?1 AND name=?2"),
        rusqlite::params![bucket, name],
        row_to_object,
    )
    .optional()
}

/// Check an `ifGenerationMatch` precondition (`0` means "must not exist").
fn check_generation(existing: Option<&GcsObject>, want: Option<i64>, what: &str) -> GcsResult<()> {
    match want {
        None => Ok(()),
        Some(0) if existing.is_none() => Ok(()),
        Some(g) if existing.map(|o| o.generation()) == Some(g) => Ok(()),
        Some(_) => Err(GcsError::PreconditionFailed(what.to_string())),
    }
}

/// Implements Cloud Storage bucket and object behavior.
pub struct StorageService {
    db: Database,
    project: String,
}

impl StorageService {
    pub fn new(db: Database, project: String) -> Self {
        StorageService { db, project }
    }

    fn compute_etag(data: &[u8]) -> String {
        let mut h = Md5::new();
        h.update(data);
        hex_encode(h.finalize())
    }

    /// Run `f` on the connection thread.
    async fn run<T, F>(&self, f: F) -> GcsResult<T>
    where
        F: FnOnce(&mut rusqlite::Connection) -> rusqlite::Result<GcsResult<T>> + Send + 'static,
        T: Send + 'static,
    {
        self.db.call(f).await?
    }

    // ── Buckets ───────────────────────────────────────────────────────────────

    pub async fn create_bucket(&self, name: &str, location: Option<&str>) -> GcsResult<GcsBucket> {
        let bucket = GcsBucket {
            name: name.to_string(),
            project: self.project.clone(),
            location: location.unwrap_or("US").to_uppercase(),
            created_at: now_ts(),
        };
        let b = bucket.clone();
        self.run(move |conn| {
            let inserted = conn.execute(
                "INSERT INTO gcs_buckets (name, project, location, created_at)
                 VALUES (?1, ?2, ?3, ?4) ON CONFLICT(name) DO NOTHING",
                rusqlite::params![b.name, b.project, b.location, b.created_at],
            )?;
            Ok(if inserted == 0 {
                Err(GcsError::BucketExists(b.name))
            } else {
                Ok(())
            })
        })
        .await?;
        Ok(bucket)
    }

    pub async fn list_buckets(&self) -> GcsResult<Vec<GcsBucket>> {
        let p = self.project.clone();
        self.run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT name, project, location, created_at FROM gcs_buckets
                 WHERE project=?1 ORDER BY name",
            )?;
            let rows = stmt
                .query_map([p], row_to_bucket)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(Ok(rows))
        })
        .await
    }

    pub async fn get_bucket(&self, name: &str) -> GcsResult<GcsBucket> {
        let n = name.to_string();
        self.run(move |conn| {
            let b = conn
                .query_row(
                    "SELECT name, project, location, created_at FROM gcs_buckets WHERE name=?1",
                    [&n],
                    row_to_bucket,
                )
                .optional()?;
            Ok(b.ok_or(GcsError::BucketNotFound(n)))
        })
        .await
    }

    /// Delete an empty bucket (GCS refuses to delete non-empty buckets).
    pub async fn delete_bucket(&self, name: &str) -> GcsResult<()> {
        let n = name.to_string();
        self.run(move |conn| {
            if !bucket_exists(conn, &n)? {
                return Ok(Err(GcsError::BucketNotFound(n)));
            }
            let objects: i64 = conn.query_row(
                "SELECT COUNT(*) FROM gcs_objects WHERE bucket=?1",
                [&n],
                |r| r.get(0),
            )?;
            if objects > 0 {
                return Ok(Err(GcsError::BucketNotEmpty(n)));
            }
            conn.execute("DELETE FROM gcs_buckets WHERE name=?1", [&n])?;
            Ok(Ok(()))
        })
        .await
    }

    // ── Objects ───────────────────────────────────────────────────────────────

    /// Create or overwrite an object. Overwriting starts a new generation.
    pub async fn upload_object(
        &self,
        bucket: &str,
        name: &str,
        content: Vec<u8>,
        content_type: &str,
        props: serde_json::Value,
        if_generation_match: Option<i64>,
    ) -> GcsResult<GcsObject> {
        let obj = GcsObject {
            bucket: bucket.to_string(),
            name: name.to_string(),
            etag: Self::compute_etag(&content),
            size: content.len() as i64,
            content,
            content_type: content_type.to_string(),
            created_at: now_ts(),
            props,
        };
        self.run(move |conn| {
            if !bucket_exists(conn, &obj.bucket)? {
                return Ok(Err(GcsError::BucketNotFound(obj.bucket)));
            }
            let existing = load_object(conn, &obj.bucket, &obj.name)?;
            if let Err(e) = check_generation(existing.as_ref(), if_generation_match, &obj.name) {
                return Ok(Err(e));
            }
            conn.execute(
                "INSERT INTO gcs_objects (bucket, name, content, content_type, size, etag, created_at, props)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(bucket, name) DO UPDATE SET
                   content=excluded.content, content_type=excluded.content_type,
                   size=excluded.size, etag=excluded.etag,
                   created_at=excluded.created_at, props=excluded.props",
                rusqlite::params![
                    obj.bucket,
                    obj.name,
                    obj.content,
                    obj.content_type,
                    obj.size,
                    obj.etag,
                    obj.created_at,
                    obj.props.to_string()
                ],
            )?;
            Ok(Ok(obj))
        })
        .await
    }

    pub async fn list_objects(&self, bucket: &str, q: ListQuery) -> GcsResult<ObjectListing> {
        let b = bucket.to_string();
        let rows = self
            .run(move |conn| {
                if !bucket_exists(conn, &b)? {
                    return Ok(Err(GcsError::BucketNotFound(b)));
                }
                let mut stmt = conn.prepare(&format!(
                    "SELECT {OBJ_COLS} FROM gcs_objects
                     WHERE bucket=?1 AND substr(name, 1, length(?2)) = ?2 ORDER BY name"
                ))?;
                let rows = stmt
                    .query_map(rusqlite::params![b, q.prefix], row_to_object)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(Ok((rows, q)))
            })
            .await?;
        Ok(paginate(rows.0, &rows.1))
    }

    pub async fn get_object(&self, bucket: &str, name: &str) -> GcsResult<GcsObject> {
        let b = bucket.to_string();
        let n = name.to_string();
        self.run(move |conn| {
            if !bucket_exists(conn, &b)? {
                return Ok(Err(GcsError::BucketNotFound(b)));
            }
            Ok(load_object(conn, &b, &n)?.ok_or(GcsError::ObjectNotFound(b, n)))
        })
        .await
    }

    /// Update mutable object properties (`contentType`, `metadata`, ...).
    /// Like GCS, `metadata` keys are merged and `null` values remove keys.
    pub async fn patch_object(
        &self,
        bucket: &str,
        name: &str,
        patch: serde_json::Value,
    ) -> GcsResult<GcsObject> {
        let b = bucket.to_string();
        let n = name.to_string();
        self.run(move |conn| {
            if !bucket_exists(conn, &b)? {
                return Ok(Err(GcsError::BucketNotFound(b)));
            }
            let Some(mut obj) = load_object(conn, &b, &n)? else {
                return Ok(Err(GcsError::ObjectNotFound(b, n)));
            };
            if let Some(ct) = patch.get("contentType").and_then(|v| v.as_str()) {
                obj.content_type = ct.to_string();
            }
            merge_props(&mut obj.props, &patch);
            conn.execute(
                "UPDATE gcs_objects SET content_type=?3, props=?4 WHERE bucket=?1 AND name=?2",
                rusqlite::params![b, n, obj.content_type, obj.props.to_string()],
            )?;
            Ok(Ok(obj))
        })
        .await
    }

    pub async fn delete_object(
        &self,
        bucket: &str,
        name: &str,
        if_generation_match: Option<i64>,
    ) -> GcsResult<()> {
        let b = bucket.to_string();
        let n = name.to_string();
        self.run(move |conn| {
            if !bucket_exists(conn, &b)? {
                return Ok(Err(GcsError::BucketNotFound(b)));
            }
            let Some(existing) = load_object(conn, &b, &n)? else {
                return Ok(Err(GcsError::ObjectNotFound(b, n)));
            };
            if let Err(e) = check_generation(Some(&existing), if_generation_match, &n) {
                return Ok(Err(e));
            }
            conn.execute(
                "DELETE FROM gcs_objects WHERE bucket=?1 AND name=?2",
                rusqlite::params![b, n],
            )?;
            Ok(Ok(()))
        })
        .await
    }
}

/// Apply the stored-property subset of a metadata/patch document.
pub fn merge_props(props: &mut serde_json::Value, patch: &serde_json::Value) {
    if !props.is_object() {
        *props = serde_json::json!({});
    }
    let target = props.as_object_mut().expect("object");
    for key in super::models::OBJECT_PROPS {
        let Some(v) = patch.get(*key) else { continue };
        if *key == "metadata" {
            let meta = target
                .entry("metadata")
                .or_insert_with(|| serde_json::json!({}));
            match (meta.as_object_mut(), v.as_object()) {
                (Some(m), Some(updates)) => {
                    for (k, val) in updates {
                        if val.is_null() {
                            m.remove(k);
                        } else {
                            m.insert(k.clone(), val.clone());
                        }
                    }
                }
                _ if v.is_null() => {
                    target.remove("metadata");
                }
                _ => {}
            }
        } else if v.is_null() {
            target.remove(*key);
        } else {
            target.insert(key.to_string(), v.clone());
        }
    }
}

enum Entry {
    Item(Box<GcsObject>),
    Prefix(String),
}

impl Entry {
    fn key(&self) -> &str {
        match self {
            Entry::Item(o) => &o.name,
            Entry::Prefix(p) => p,
        }
    }
}

/// Apply offsets, delimiter grouping and page-token pagination to objects
/// already filtered by prefix and sorted by name.
fn paginate(rows: Vec<GcsObject>, q: &ListQuery) -> ObjectListing {
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let after = q
        .page_token
        .as_deref()
        .and_then(|t| b64.decode(t).ok())
        .and_then(|v| String::from_utf8(v).ok());
    let delimiter = q.delimiter.as_deref().filter(|d| !d.is_empty());

    let mut entries: Vec<Entry> = Vec::new();
    let mut seen_prefixes = HashSet::new();
    for obj in rows {
        if q.start_offset
            .as_deref()
            .is_some_and(|s| obj.name.as_str() < s)
            || q.end_offset
                .as_deref()
                .is_some_and(|e| obj.name.as_str() >= e)
        {
            continue;
        }
        let rest = &obj.name[q.prefix.len()..];
        if let Some(pos) = delimiter.and_then(|d| rest.find(d).map(|p| p + d.len())) {
            let p = format!("{}{}", q.prefix, &rest[..pos]);
            if seen_prefixes.insert(p.clone()) {
                entries.push(Entry::Prefix(p));
            }
        } else {
            entries.push(Entry::Item(Box::new(obj)));
        }
    }

    let max = q.max_results.filter(|m| *m > 0).unwrap_or(1000);
    let mut remaining = entries
        .into_iter()
        .filter(|e| after.as_deref().is_none_or(|a| e.key() > a))
        .peekable();
    let mut out = ObjectListing::default();
    let mut last_key = None;
    for e in remaining.by_ref().take(max) {
        last_key = Some(e.key().to_string());
        match e {
            Entry::Item(o) => out.items.push(*o),
            Entry::Prefix(p) => out.prefixes.push(p),
        }
    }
    if remaining.peek().is_some() {
        out.next_page_token = last_key.map(|k| b64.encode(k));
    }
    out
}

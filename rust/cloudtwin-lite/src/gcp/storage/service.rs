//! GCP Cloud Storage business logic.
//!
//! Conceptually this mirrors the S3 service: buckets and objects are persisted
//! in SQLite, while HTTP and JSON concerns stay in the handler layer.

use anyhow::{bail, Result};
use chrono::Utc;
use hex::encode as hex_encode;
use md5::{Digest, Md5};
use rusqlite::OptionalExtension;

use super::models::{GcsBucket, GcsObject};
use crate::db::Database;

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

    // ── Buckets ───────────────────────────────────────────────────────────────

    pub async fn create_bucket(&self, name: &str, location: Option<&str>) -> Result<GcsBucket> {
        let n = name.to_string();
        let p = self.project.clone();
        let loc = location.unwrap_or("US").to_string();
        let now = Utc::now().to_rfc3339();
        let n2 = n.clone();
        let p2 = p.clone();
        let loc2 = loc.clone();
        let n3 = now.clone();
        self.db
            .conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO gcs_buckets (name, project, location, created_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(name) DO NOTHING",
                    rusqlite::params![n, p, loc, now],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(GcsBucket {
            name: n2,
            project: p2,
            location: loc2,
            created_at: n3,
        })
    }

    pub async fn list_buckets(&self) -> Result<Vec<GcsBucket>> {
        let p = self.project.clone();
        self.db
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT name, project, location, created_at FROM gcs_buckets
                 WHERE project=?1 ORDER BY name",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![p], |r| {
                        Ok(GcsBucket {
                            name: r.get(0)?,
                            project: r.get(1)?,
                            location: r.get(2)?,
                            created_at: r.get(3)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn get_bucket(&self, name: &str) -> Result<Option<GcsBucket>> {
        let n = name.to_string();
        self.db
            .conn
            .call(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT name, project, location, created_at FROM gcs_buckets WHERE name=?1",
                        rusqlite::params![n],
                        |r| {
                            Ok(GcsBucket {
                                name: r.get(0)?,
                                project: r.get(1)?,
                                location: r.get(2)?,
                                created_at: r.get(3)?,
                            })
                        },
                    )
                    .optional()?)
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn delete_bucket(&self, name: &str) -> Result<()> {
        let n = name.to_string();
        let deleted = self
            .db
            .conn
            .call(move |conn| {
                Ok(conn.execute(
                    "DELETE FROM gcs_buckets WHERE name=?1",
                    rusqlite::params![n],
                )?)
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        if deleted == 0 {
            bail!("BucketNotFound");
        }
        Ok(())
    }

    // ── Objects ───────────────────────────────────────────────────────────────

    pub async fn upload_object(
        &self,
        bucket: &str,
        name: &str,
        content: Vec<u8>,
        content_type: &str,
    ) -> Result<GcsObject> {
        let etag = Self::compute_etag(&content);
        let size = content.len() as i64;
        let b = bucket.to_string();
        let n = name.to_string();
        let ct = content_type.to_string();
        let etag2 = etag.clone();
        let now = Utc::now().to_rfc3339();
        let n2 = n.clone();
        let b2 = b.clone();
        let ct2 = ct.clone();
        let now2 = now.clone();
        self.db.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO gcs_objects (bucket, name, content, content_type, size, etag, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(bucket, name) DO UPDATE SET
                   content=excluded.content, content_type=excluded.content_type,
                   size=excluded.size, etag=excluded.etag",
                rusqlite::params![b, n, content, ct, size, etag2, now],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(GcsObject {
            bucket: b2,
            name: n2,
            content: Vec::new(),
            content_type: ct2,
            size,
            etag,
            created_at: now2,
        })
    }

    pub async fn list_objects(&self, bucket: &str, prefix: Option<&str>) -> Result<Vec<GcsObject>> {
        let b = bucket.to_string();
        let pref = prefix
            .map(|s| format!("{s}%"))
            .unwrap_or_else(|| "%".to_string());
        self.db
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT bucket, name, content, content_type, size, etag, created_at
                 FROM gcs_objects WHERE bucket=?1 AND name LIKE ?2 ORDER BY name",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![b, pref], |r| {
                        Ok(GcsObject {
                            bucket: r.get(0)?,
                            name: r.get(1)?,
                            content: r.get(2)?,
                            content_type: r.get(3)?,
                            size: r.get(4)?,
                            etag: r.get(5)?,
                            created_at: r.get(6)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn get_object(&self, bucket: &str, name: &str) -> Result<GcsObject> {
        let b = bucket.to_string();
        let n = name.to_string();
        let obj: Option<GcsObject> = self
            .db
            .conn
            .call(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT bucket, name, content, content_type, size, etag, created_at
                 FROM gcs_objects WHERE bucket=?1 AND name=?2",
                        rusqlite::params![b, n],
                        |r| {
                            Ok(GcsObject {
                                bucket: r.get(0)?,
                                name: r.get(1)?,
                                content: r.get(2)?,
                                content_type: r.get(3)?,
                                size: r.get(4)?,
                                etag: r.get(5)?,
                                created_at: r.get(6)?,
                            })
                        },
                    )
                    .optional()?)
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        obj.ok_or_else(|| anyhow::anyhow!("ObjectNotFound"))
    }

    pub async fn delete_object(&self, bucket: &str, name: &str) -> Result<()> {
        let b = bucket.to_string();
        let n = name.to_string();
        self.db
            .conn
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM gcs_objects WHERE bucket=?1 AND name=?2",
                    rusqlite::params![b, n],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }
}

//! S3 domain logic backed by SQLite.
//!
//! The service owns bucket/object behavior and leaves HTTP concerns to
//! `handlers.rs`.

use anyhow::{bail, Result};
use chrono::Utc;
use hex::encode as hex_encode;
use md5::{Digest, Md5};

use crate::db::Database;
use super::models::{Bucket, Object};

/// Implements S3 bucket and object semantics on top of SQLite.
pub struct S3Service {
    db: Database,
}

impl S3Service {
    pub fn new(db: Database) -> Self {
        S3Service { db }
    }

    // ── Bucket operations ─────────────────────────────────────────────────

    pub async fn list_buckets(&self) -> Result<Vec<Bucket>> {
        self.db
            .conn
            .call(|conn| {
                let mut stmt =
                    conn.prepare("SELECT name, created_at FROM s3_buckets ORDER BY created_at")?;
                let rows = stmt
                    .query_map([], |r| {
                        Ok(Bucket {
                            name: r.get(0)?,
                            created_at: r.get(1)?,
                        })
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn create_bucket(&self, name: &str) -> Result<()> {
        let name = name.to_string();
        let now = Utc::now().to_rfc3339();
        self.db
            .conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO s3_buckets (name, created_at) VALUES (?1, ?2)",
                    rusqlite::params![name, now],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn delete_bucket(&self, name: &str) -> Result<()> {
        let name = name.to_string();
        let deleted = self.db
            .conn
            .call(move |conn| {
                Ok(conn.execute(
                    "DELETE FROM s3_buckets WHERE name = ?1",
                    rusqlite::params![name],
                )?)
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        if deleted == 0 {
            bail!("NoSuchBucket");
        }
        Ok(())
    }

    pub async fn bucket_exists(&self, name: &str) -> Result<bool> {
        let name = name.to_string();
        self.db
            .conn
            .call(move |conn| {
                let n: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM s3_buckets WHERE name = ?1",
                    rusqlite::params![name],
                    |r| r.get(0),
                )?;
                Ok(n > 0)
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    // ── Object operations ─────────────────────────────────────────────────

    pub async fn list_objects(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        max_keys: i64,
    ) -> Result<Vec<Object>> {
        if !self.bucket_exists(bucket).await? {
            bail!("NoSuchBucket");
        }
        let bucket = bucket.to_string();
        let prefix = prefix.map(|s| s.to_string()).unwrap_or_default();
        self.db
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT bucket, key, content, content_type, size, etag, created_at
                     FROM s3_objects
                     WHERE bucket = ?1 AND key LIKE ?2
                     ORDER BY key
                     LIMIT ?3",
                )?;
                let like = format!("{prefix}%");
                let rows = stmt
                    .query_map(rusqlite::params![bucket, like, max_keys], |r| {
                        Ok(Object {
                            bucket: r.get(0)?,
                            key: r.get(1)?,
                            content: r.get(2)?,
                            content_type: r.get(3)?,
                            size: r.get(4)?,
                            etag: r.get(5)?,
                            created_at: r.get(6)?,
                        })
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        content: Vec<u8>,
        content_type: &str,
    ) -> Result<String> {
        if !self.bucket_exists(bucket).await? {
            bail!("NoSuchBucket");
        }
        let etag = format!("\"{}\"", hex_encode(Md5::digest(&content)));
        let size = content.len() as i64;
        let now = Utc::now().to_rfc3339();
        let bucket = bucket.to_string();
        let key = key.to_string();
        let content_type = content_type.to_string();
        let etag_clone = etag.clone();

        self.db
            .conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO s3_objects (bucket, key, content, content_type, size, etag, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT (bucket, key) DO UPDATE SET
                       content      = excluded.content,
                       content_type = excluded.content_type,
                       size         = excluded.size,
                       etag         = excluded.etag,
                       created_at   = excluded.created_at",
                    rusqlite::params![bucket, key, content, content_type, size, etag_clone, now],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        Ok(etag)
    }

    pub async fn get_object(&self, bucket: &str, key: &str) -> Result<Object> {
        if !self.bucket_exists(bucket).await? {
            bail!("NoSuchBucket");
        }
        let bucket = bucket.to_string();
        let key = key.to_string();
        self.db
            .conn
            .call(move |conn| {
                Ok(conn.query_row(
                    "SELECT bucket, key, content, content_type, size, etag, created_at
                     FROM s3_objects WHERE bucket = ?1 AND key = ?2",
                    rusqlite::params![bucket, key],
                    |r| {
                        Ok(Object {
                            bucket: r.get(0)?,
                            key: r.get(1)?,
                            content: r.get(2)?,
                            content_type: r.get(3)?,
                            size: r.get(4)?,
                            etag: r.get(5)?,
                            created_at: r.get(6)?,
                        })
                    },
                )?)
            })
            .await
            .map_err(|e| {
                // QueryReturnedNoRows → user-visible "NoSuchKey"
                if matches!(
                    e,
                    tokio_rusqlite::Error::Rusqlite(rusqlite::Error::QueryReturnedNoRows)
                ) {
                    anyhow::anyhow!("NoSuchKey")
                } else {
                    anyhow::anyhow!("{e}")
                }
            })
    }

    pub async fn delete_object(&self, bucket: &str, key: &str) -> Result<()> {
        let bucket = bucket.to_string();
        let key = key.to_string();
        self.db
            .conn
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM s3_objects WHERE bucket = ?1 AND key = ?2",
                    rusqlite::params![bucket, key],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }
}

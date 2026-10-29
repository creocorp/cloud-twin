//! Azure Blob Storage business logic.
//!
//! This service hides the SQL details behind storage-style operations such as
//! create container, put blob, and fetch blob metadata/content.

use anyhow::Result;
use chrono::Utc;
use hex::encode as hex_encode;
use md5::{Digest, Md5};
use rusqlite::OptionalExtension;

use super::models::{AzureBlob, AzureContainer};
use crate::db::Database;

/// Outcome of a conditional or existence-checked write.
#[derive(Debug)]
pub enum PutOutcome {
    Stored(AzureBlob),
    ContainerNotFound,
    AlreadyExists,
}

/// Implements Azure Blob semantics over the shared SQLite store.
pub struct BlobService {
    db: Database,
    account: String,
}

fn map_err(e: tokio_rusqlite::Error) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}

fn blob_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AzureBlob> {
    Ok(AzureBlob {
        container: r.get(0)?,
        account: r.get(1)?,
        name: r.get(2)?,
        content: r.get(3)?,
        content_type: r.get(4)?,
        size: r.get(5)?,
        etag: r.get(6)?,
        created_at: r.get(7)?,
    })
}

impl BlobService {
    pub fn new(db: Database, account: String) -> Self {
        BlobService { db, account }
    }

    fn compute_etag(data: &[u8]) -> String {
        let mut h = Md5::new();
        h.update(data);
        format!("\"{}\"", hex_encode(h.finalize()))
    }

    // ── Containers ────────────────────────────────────────────────────────────

    /// Create a container; returns `false` when it already exists.
    pub async fn create_container(&self, container: &str) -> Result<bool> {
        let c = container.to_string();
        let acc = self.account.clone();
        let now = Utc::now().to_rfc3339();
        let inserted = self
            .db
            .conn
            .call(move |conn| {
                Ok(conn.execute(
                    "INSERT INTO azure_containers (name, account, created_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(account, name) DO NOTHING",
                    rusqlite::params![c, acc, now],
                )?)
            })
            .await
            .map_err(map_err)?;
        Ok(inserted > 0)
    }

    pub async fn list_containers(&self) -> Result<Vec<AzureContainer>> {
        let acc = self.account.clone();
        self.db
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT name, account, created_at FROM azure_containers
                 WHERE account = ?1 ORDER BY name",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![acc], |r| {
                        Ok(AzureContainer {
                            name: r.get(0)?,
                            account: r.get(1)?,
                            created_at: r.get(2)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
            .map_err(map_err)
    }

    pub async fn get_container(&self, container: &str) -> Result<Option<AzureContainer>> {
        let c = container.to_string();
        let acc = self.account.clone();
        self.db
            .conn
            .call(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT name, account, created_at FROM azure_containers
                         WHERE account=?1 AND name=?2",
                        rusqlite::params![acc, c],
                        |r| {
                            Ok(AzureContainer {
                                name: r.get(0)?,
                                account: r.get(1)?,
                                created_at: r.get(2)?,
                            })
                        },
                    )
                    .optional()?)
            })
            .await
            .map_err(map_err)
    }

    /// Delete a container and its blobs; returns `false` when it did not exist.
    pub async fn delete_container(&self, container: &str) -> Result<bool> {
        let c = container.to_string();
        let acc = self.account.clone();
        let deleted = self
            .db
            .conn
            .call(move |conn| {
                let tx = conn.transaction()?;
                let n = tx.execute(
                    "DELETE FROM azure_containers WHERE account=?1 AND name=?2",
                    rusqlite::params![acc, c],
                )?;
                tx.execute(
                    "DELETE FROM azure_blobs WHERE account=?1 AND container=?2",
                    rusqlite::params![acc, c],
                )?;
                tx.commit()?;
                Ok(n)
            })
            .await
            .map_err(map_err)?;
        Ok(deleted > 0)
    }

    // ── Blobs ─────────────────────────────────────────────────────────────────

    /// Store a block blob. With `if_none_match_any`, an existing blob is left
    /// untouched and `AlreadyExists` is returned (SDK `overwrite=False`).
    pub async fn put_blob(
        &self,
        container: &str,
        blob_name: &str,
        content: Vec<u8>,
        content_type: &str,
        if_none_match_any: bool,
    ) -> Result<PutOutcome> {
        let etag = Self::compute_etag(&content);
        let c = container.to_string();
        let bn = blob_name.to_string();
        let acc = self.account.clone();
        let ct = content_type.to_string();
        let now = Utc::now().to_rfc3339();
        self.db
            .conn
            .call(move |conn| {
                let tx = conn.transaction()?;
                let exists: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM azure_containers WHERE account=?1 AND name=?2",
                    rusqlite::params![acc, c],
                    |r| r.get(0),
                )?;
                if exists == 0 {
                    return Ok(PutOutcome::ContainerNotFound);
                }
                if if_none_match_any {
                    let n: i64 = tx.query_row(
                        "SELECT COUNT(*) FROM azure_blobs WHERE account=?1 AND container=?2 AND name=?3",
                        rusqlite::params![acc, c, bn],
                        |r| r.get(0),
                    )?;
                    if n > 0 {
                        return Ok(PutOutcome::AlreadyExists);
                    }
                }
                let size = content.len() as i64;
                tx.execute(
                    "INSERT INTO azure_blobs (container, account, name, content, content_type, size, etag, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(account, container, name) DO UPDATE SET
                       content=excluded.content, content_type=excluded.content_type,
                       size=excluded.size, etag=excluded.etag",
                    rusqlite::params![c, acc, bn, content, ct, size, etag, now],
                )?;
                let blob = tx.query_row(
                    "SELECT container, account, name, X'', content_type, size, etag, created_at
                     FROM azure_blobs WHERE account=?1 AND container=?2 AND name=?3",
                    rusqlite::params![acc, c, bn],
                    blob_from_row,
                )?;
                tx.commit()?;
                Ok(PutOutcome::Stored(blob))
            })
            .await
            .map_err(map_err)
    }

    pub async fn get_blob(&self, container: &str, blob_name: &str) -> Result<Option<AzureBlob>> {
        let c = container.to_string();
        let bn = blob_name.to_string();
        let acc = self.account.clone();
        self.db
            .conn
            .call(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT container, account, name, content, content_type, size, etag, created_at
                         FROM azure_blobs WHERE account=?1 AND container=?2 AND name=?3",
                        rusqlite::params![acc, c, bn],
                        blob_from_row,
                    )
                    .optional()?)
            })
            .await
            .map_err(map_err)
    }

    /// List blob metadata (content is not loaded) whose names start with `prefix`.
    pub async fn list_blobs(&self, container: &str, prefix: &str) -> Result<Vec<AzureBlob>> {
        let c = container.to_string();
        let acc = self.account.clone();
        let pref = prefix.to_string();
        self.db
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT container, account, name, X'', content_type, size, etag, created_at
                     FROM azure_blobs
                     WHERE account=?1 AND container=?2 AND substr(name, 1, length(?3)) = ?3
                     ORDER BY name",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![acc, c, pref], blob_from_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
            .map_err(map_err)
    }

    /// Delete a blob; returns `false` when it did not exist.
    pub async fn delete_blob(&self, container: &str, blob_name: &str) -> Result<bool> {
        let c = container.to_string();
        let bn = blob_name.to_string();
        let acc = self.account.clone();
        let n = self
            .db
            .conn
            .call(move |conn| {
                Ok(conn.execute(
                    "DELETE FROM azure_blobs WHERE account=?1 AND container=?2 AND name=?3",
                    rusqlite::params![acc, c, bn],
                )?)
            })
            .await
            .map_err(map_err)?;
        Ok(n > 0)
    }
}

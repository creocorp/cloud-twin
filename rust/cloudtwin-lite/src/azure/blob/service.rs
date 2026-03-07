use anyhow::{bail, Result};
use chrono::Utc;
use hex::encode as hex_encode;
use md5::{Digest, Md5};
use rusqlite::OptionalExtension;

use crate::db::Database;
use super::models::{AzureBlob, AzureContainer};

pub struct BlobService {
    db:      Database,
    account: String,
}

impl BlobService {
    pub fn new(db: Database, account: String) -> Self { BlobService { db, account } }

    fn compute_etag(data: &[u8]) -> String {
        let mut h = Md5::new();
        h.update(data);
        format!("\"{}\"", hex_encode(h.finalize()))
    }

    // ── Containers ────────────────────────────────────────────────────────────

    pub async fn create_container(&self, container: &str) -> Result<()> {
        let c   = container.to_string();
        let acc = self.account.clone();
        let now = Utc::now().to_rfc3339();
        self.db.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO azure_containers (name, account, created_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(account, name) DO NOTHING",
                rusqlite::params![c, acc, now],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn list_containers(&self) -> Result<Vec<AzureContainer>> {
        let acc = self.account.clone();
        self.db.conn.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT name, account, created_at FROM azure_containers
                 WHERE account = ?1 ORDER BY name"
            )?;
            let rows = stmt.query_map(rusqlite::params![acc], |r| Ok(AzureContainer {
                name:       r.get(0)?,
                account:    r.get(1)?,
                created_at: r.get(2)?,
            }))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn container_exists(&self, container: &str) -> Result<bool> {
        let c   = container.to_string();
        let acc = self.account.clone();
        self.db.conn.call(move |conn| {
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM azure_containers WHERE account=?1 AND name=?2",
                rusqlite::params![acc, c], |r| r.get(0),
            )?;
            Ok(n > 0)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn delete_container(&self, container: &str) -> Result<()> {
        let c   = container.to_string();
        let acc = self.account.clone();
        let deleted = self.db.conn.call(move |conn| {
            Ok(conn.execute(
                "DELETE FROM azure_containers WHERE account=?1 AND name=?2",
                rusqlite::params![acc, c],
            )?)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        if deleted == 0 { bail!("ContainerNotFound"); }
        Ok(())
    }

    // ── Blobs ─────────────────────────────────────────────────────────────────

    pub async fn put_blob(
        &self,
        container: &str,
        blob_name: &str,
        content: Vec<u8>,
        content_type: &str,
    ) -> Result<String> {
        if !self.container_exists(container).await? {
            bail!("ContainerNotFound");
        }
        let etag         = Self::compute_etag(&content);
        let size         = content.len() as i64;
        let c            = container.to_string();
        let bn           = blob_name.to_string();
        let acc          = self.account.clone();
        let ct           = content_type.to_string();
        let etag_clone   = etag.clone();
        let now          = Utc::now().to_rfc3339();
        self.db.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO azure_blobs (container, account, name, content, content_type, size, etag, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(account, container, name) DO UPDATE SET
                   content=excluded.content, content_type=excluded.content_type,
                   size=excluded.size, etag=excluded.etag",
                rusqlite::params![c, acc, bn, content, ct, size, etag_clone, now],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(etag)
    }

    pub async fn get_blob(&self, container: &str, blob_name: &str) -> Result<AzureBlob> {
        let c   = container.to_string();
        let bn  = blob_name.to_string();
        let acc = self.account.clone();
        let blob: Option<AzureBlob> = self.db.conn.call(move |conn| {
            Ok(conn.query_row(
                "SELECT container, account, name, content, content_type, size, etag, created_at
                 FROM azure_blobs WHERE account=?1 AND container=?2 AND name=?3",
                rusqlite::params![acc, c, bn],
                |r| Ok(AzureBlob {
                    container:    r.get(0)?,
                    account:      r.get(1)?,
                    name:         r.get(2)?,
                    content:      r.get(3)?,
                    content_type: r.get(4)?,
                    size:         r.get(5)?,
                    etag:         r.get(6)?,
                    created_at:   r.get(7)?,
                }),
            ).optional()?)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        blob.ok_or_else(|| anyhow::anyhow!("BlobNotFound"))
    }

    pub async fn list_blobs(&self, container: &str, prefix: Option<&str>) -> Result<Vec<AzureBlob>> {
        let c    = container.to_string();
        let acc  = self.account.clone();
        let pref = prefix.map(|s| format!("{s}%")).unwrap_or_else(|| "%".to_string());
        self.db.conn.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT container, account, name, content, content_type, size, etag, created_at
                 FROM azure_blobs WHERE account=?1 AND container=?2 AND name LIKE ?3
                 ORDER BY name"
            )?;
            let rows = stmt.query_map(rusqlite::params![acc, c, pref], |r| Ok(AzureBlob {
                container:    r.get(0)?,
                account:      r.get(1)?,
                name:         r.get(2)?,
                content:      r.get(3)?,
                content_type: r.get(4)?,
                size:         r.get(5)?,
                etag:         r.get(6)?,
                created_at:   r.get(7)?,
            }))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn delete_blob(&self, container: &str, blob_name: &str) -> Result<()> {
        let c   = container.to_string();
        let bn  = blob_name.to_string();
        let acc = self.account.clone();
        self.db.conn.call(move |conn| {
            conn.execute(
                "DELETE FROM azure_blobs WHERE account=?1 AND container=?2 AND name=?3",
                rusqlite::params![acc, c, bn],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }
}

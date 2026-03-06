use anyhow::Result;
use tokio_rusqlite::Connection;

/// Thin async wrapper around an SQLite connection.
#[derive(Clone)]
pub struct Database {
    pub conn: Connection,
}

impl Database {
    pub async fn open(path: &str) -> Result<Self> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory().await?
        } else {
            // Ensure parent directory exists
            if let Some(parent) = std::path::Path::new(path).parent() {
                std::fs::create_dir_all(parent)?;
            }
            Connection::open(path).await?
        };
        Ok(Database { conn })
    }

    /// Run all DDL migrations.
    pub async fn migrate(&self) -> Result<()> {
        self.conn
            .call(|conn| {
                conn.execute_batch(
                    "PRAGMA journal_mode = WAL;
                     PRAGMA foreign_keys = ON;

                     CREATE TABLE IF NOT EXISTS s3_buckets (
                         name       TEXT PRIMARY KEY,
                         created_at TEXT NOT NULL
                     );

                     CREATE TABLE IF NOT EXISTS s3_objects (
                         bucket       TEXT    NOT NULL,
                         key          TEXT    NOT NULL,
                         content      BLOB    NOT NULL DEFAULT X'',
                         content_type TEXT    NOT NULL DEFAULT 'application/octet-stream',
                         size         INTEGER NOT NULL DEFAULT 0,
                         etag         TEXT    NOT NULL DEFAULT '',
                         created_at   TEXT    NOT NULL,
                         PRIMARY KEY (bucket, key)
                     );",
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }
}

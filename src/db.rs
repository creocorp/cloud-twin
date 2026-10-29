//! Shared database bootstrapping and schema creation.
//!
//! This file is the closest thing CloudTwin has to an infrastructure
//! composition root for persistence. Each service talks to SQLite through simple
//! SQL statements, and this module owns the one connection plus the global DDL.

use anyhow::Result;
use tokio_rusqlite::Connection;

/// Thin async wrapper around an SQLite connection.
///
/// `tokio_rusqlite` lets us keep a synchronous SQLite driver while exposing an
/// async API to the rest of the app. You can think of this like a tiny async
/// repository host that serializes DB work onto the connection thread.
#[derive(Clone)]
pub struct Database {
    pub conn: Connection,
}

impl Database {
    /// Open either an in-memory database or a file-backed database.
    pub async fn open(path: &str) -> Result<Self> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory().await?
        } else {
            if let Some(parent) = std::path::Path::new(path).parent() {
                std::fs::create_dir_all(parent)?;
            }
            Connection::open(path).await?
        };
        Ok(Database { conn })
    }

    /// Run all DDL migrations.
    ///
    /// This is deliberately simplistic: CloudTwin is optimized for local
    /// development and CI, so we use idempotent `CREATE TABLE IF NOT EXISTS`
    /// statements rather than a versioned migration framework.
    pub async fn migrate(&self) -> Result<()> {
        let legacy = self
            .conn
            .call(|conn| Ok(legacy_python_tables(conn)?))
            .await?;
        if !legacy.is_empty() {
            anyhow::bail!(
                "this database was created by the retired Python CloudTwin backend \
                 (found tables: {}). Its schema is not compatible; point CLOUDTWIN_DB_PATH \
                 at a new file or remove the old one",
                legacy.join(", ")
            );
        }
        self.conn
            .call(|conn| {
                conn.execute_batch(DDL)?;
                // Each AWS module owns its tables and additive migrations.
                crate::aws::s3::schema::migrate(conn)?;
                crate::aws::ses::schema::migrate(conn)?;
                crate::aws::sqs::schema::migrate(conn)?;
                crate::aws::sns::schema::migrate(conn)?;
                crate::aws::kms::schema::migrate(conn)?;
                crate::aws::cognito::schema::migrate(conn)?;
                crate::gcp::storage::schema::migrate(conn)?;
                crate::gcp::pubsub::schema::migrate(conn)?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Run a closure on the connection thread, mapping errors to `anyhow`.
    pub async fn call<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut rusqlite::Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        self.conn
            .call(move |conn| f(conn).map_err(tokio_rusqlite::Error::from))
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }
}

/// Tables only the retired Python backend created. Their presence means the
/// file uses the Python schema, whose shared tables (`sqs_messages`, ...) have
/// incompatible constraints.
const LEGACY_PYTHON_TABLES: &[&str] = &[
    "lambda_functions",
    "sm_secrets",
    "kv_secrets",
    "firestore_documents",
    "cloudtasks_queues",
    "eg_topics",
];

fn legacy_python_tables(conn: &rusqlite::Connection) -> rusqlite::Result<Vec<String>> {
    let mut found = Vec::new();
    for t in LEGACY_PYTHON_TABLES {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [t],
            |r| r.get(0),
        )?;
        if n > 0 {
            found.push(t.to_string());
        }
    }
    Ok(found)
}

/// Add `column` to `table` when an older database file predates it.
///
/// `CREATE TABLE IF NOT EXISTS` never alters existing tables, so modules call
/// this for every column added after a table first shipped.
pub fn ensure_column(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
    decl: &str,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let exists = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(|r| r.ok())
        .any(|c| c == column);
    if !exists {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"))?;
    }
    Ok(())
}

/// Read a value from the `cloudtwin_meta` key/value table (never reset).
pub fn meta_get(conn: &rusqlite::Connection, key: &str) -> rusqlite::Result<Option<String>> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT value FROM cloudtwin_meta WHERE key = ?1",
        rusqlite::params![key],
        |r| r.get(0),
    )
    .optional()
}

/// Write a value to the `cloudtwin_meta` key/value table.
pub fn meta_set(conn: &rusqlite::Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO cloudtwin_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

/// Shared schema bootstrapped on startup. AWS S3/SES/SQS/SNS/KMS/Cognito
/// tables live in each module's `schema.rs`.
const DDL: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- Internal key/value state that survives `/_cloudtwin/reset` (e.g. signing keys).
CREATE TABLE IF NOT EXISTS cloudtwin_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- ─── AWS DynamoDB ─────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS dynamo_tables (
    name                  TEXT PRIMARY KEY,
    key_schema            TEXT NOT NULL DEFAULT '[]',
    attribute_definitions TEXT NOT NULL DEFAULT '[]',
    created_at            TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS dynamo_items (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    table_name TEXT NOT NULL,
    pk         TEXT NOT NULL,
    sk         TEXT NOT NULL DEFAULT '',
    item       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (table_name, pk, sk)
);

-- ─── AWS Secrets Manager ──────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS secrets (
    name          TEXT PRIMARY KEY,
    arn           TEXT NOT NULL,
    secret_string TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

-- ─── Azure Blob Storage ───────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS azure_containers (
    name       TEXT NOT NULL,
    account    TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (account, name)
);
CREATE TABLE IF NOT EXISTS azure_blobs (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    container    TEXT    NOT NULL,
    account      TEXT    NOT NULL,
    name         TEXT    NOT NULL,
    content      BLOB    NOT NULL DEFAULT X'',
    content_type TEXT    NOT NULL DEFAULT 'application/octet-stream',
    size         INTEGER NOT NULL DEFAULT 0,
    etag         TEXT    NOT NULL DEFAULT '',
    created_at   TEXT    NOT NULL,
    UNIQUE (account, container, name)
);

-- ─── Azure Service Bus ────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS asb_queues (
    name       TEXT NOT NULL,
    namespace  TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (namespace, name)
);
CREATE TABLE IF NOT EXISTS asb_topics (
    name       TEXT NOT NULL,
    namespace  TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (namespace, name)
);
CREATE TABLE IF NOT EXISTS asb_subscriptions (
    name       TEXT NOT NULL,
    topic      TEXT NOT NULL,
    namespace  TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (namespace, topic, name)
);
CREATE TABLE IF NOT EXISTS asb_messages (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id   TEXT    NOT NULL,
    namespace    TEXT    NOT NULL,
    queue_name   TEXT    NOT NULL,
    body         TEXT    NOT NULL,
    lock_token   TEXT    NOT NULL UNIQUE,
    visible      INTEGER NOT NULL DEFAULT 1,
    created_at   TEXT    NOT NULL
);

-- ─── GCP Cloud Storage ────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS gcs_buckets (
    name       TEXT PRIMARY KEY,
    project    TEXT NOT NULL DEFAULT 'cloudtwin',
    location   TEXT NOT NULL DEFAULT 'US',
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS gcs_objects (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    bucket       TEXT    NOT NULL,
    name         TEXT    NOT NULL,
    content      BLOB    NOT NULL DEFAULT X'',
    content_type TEXT    NOT NULL DEFAULT 'application/octet-stream',
    size         INTEGER NOT NULL DEFAULT 0,
    etag         TEXT    NOT NULL DEFAULT '',
    created_at   TEXT    NOT NULL,
    UNIQUE (bucket, name)
);

-- ─── GCP Pub/Sub ─────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS pubsub_topics (
    name       TEXT PRIMARY KEY,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS pubsub_subscriptions (
    name       TEXT PRIMARY KEY,
    topic      TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS pubsub_messages (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id TEXT NOT NULL,
    topic      TEXT NOT NULL,
    data       TEXT NOT NULL DEFAULT '',
    attributes TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS pubsub_ackables (
    ack_id       TEXT PRIMARY KEY,
    subscription TEXT NOT NULL,
    message_id   TEXT NOT NULL,
    acked        INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT NOT NULL
);

-- ─── Telemetry events ─────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS events (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    provider   TEXT    NOT NULL,
    service    TEXT    NOT NULL,
    action     TEXT    NOT NULL,
    payload    TEXT    NOT NULL DEFAULT '{}',
    created_at TEXT    NOT NULL
);
-- ─── Mailbox (local email capture) ─────────────────────────────────
CREATE TABLE IF NOT EXISTS mailbox_messages (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id TEXT    NOT NULL UNIQUE,
    from_addr  TEXT    NOT NULL DEFAULT '',
    to_addrs   TEXT    NOT NULL DEFAULT '[]',
    cc_addrs   TEXT    NOT NULL DEFAULT '[]',
    bcc_addrs  TEXT    NOT NULL DEFAULT '[]',
    subject    TEXT    NOT NULL DEFAULT '',
    text_body  TEXT,
    html_body  TEXT,
    raw        TEXT    NOT NULL DEFAULT '',
    source     TEXT    NOT NULL DEFAULT 'smtp',
    size       INTEGER NOT NULL DEFAULT 0,
    created_at TEXT    NOT NULL
);
CREATE TABLE IF NOT EXISTS mailbox_attachments (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id   TEXT    NOT NULL,
    filename     TEXT    NOT NULL DEFAULT '',
    content_type TEXT    NOT NULL DEFAULT 'application/octet-stream',
    size         INTEGER NOT NULL DEFAULT 0,
    content      BLOB    NOT NULL DEFAULT X'',
    created_at   TEXT    NOT NULL
);";

#[cfg(test)]
mod tests {
    use super::Database;

    #[tokio::test]
    async fn refuses_python_era_databases() {
        let db = Database::open(":memory:").await.unwrap();
        db.conn
            .call(|c| {
                c.execute_batch("CREATE TABLE lambda_functions (name TEXT PRIMARY KEY);")?;
                Ok(())
            })
            .await
            .unwrap();
        let err = db.migrate().await.unwrap_err().to_string();
        assert!(err.contains("retired Python CloudTwin backend"), "{err}");

        let fresh = Database::open(":memory:").await.unwrap();
        fresh.migrate().await.unwrap();
        fresh.migrate().await.unwrap();
    }
}

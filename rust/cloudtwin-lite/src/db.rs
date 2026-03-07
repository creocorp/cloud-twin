//! Shared database bootstrapping and schema creation.
//!
//! This file is the closest thing the Lite binary has to an infrastructure
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
    /// This is deliberately simplistic: the Lite binary is optimized for local
    /// development and CI, so we use idempotent `CREATE TABLE IF NOT EXISTS`
    /// statements rather than a versioned migration framework.
    pub async fn migrate(&self) -> Result<()> {
        self.conn.call(|conn| { conn.execute_batch(DDL)?; Ok(()) }).await?;
        Ok(())
    }
}

/// Entire schema bootstrapped on startup.
///
/// Keeping the DDL in one place makes the binary easy to reason about: every
/// supported service is visible in a single schema block.
const DDL: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- ─── AWS S3 ───────────────────────────────────────────────────────────────────
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
);

-- ─── AWS SES ──────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS ses_identities (
    identity   TEXT PRIMARY KEY,
    kind       TEXT    NOT NULL DEFAULT 'email',
    verified   INTEGER NOT NULL DEFAULT 1,
    token      TEXT,
    created_at TEXT    NOT NULL
);
CREATE TABLE IF NOT EXISTS ses_messages (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id   TEXT    NOT NULL UNIQUE,
    source       TEXT    NOT NULL,
    destinations TEXT    NOT NULL DEFAULT '[]',
    subject      TEXT    NOT NULL DEFAULT '',
    text_body    TEXT,
    html_body    TEXT,
    status       TEXT    NOT NULL DEFAULT 'sent',
    created_at   TEXT    NOT NULL
);

-- ─── AWS SQS ──────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS sqs_queues (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT    NOT NULL UNIQUE,
    url        TEXT    NOT NULL,
    created_at TEXT    NOT NULL
);
CREATE TABLE IF NOT EXISTS sqs_messages (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id     TEXT    NOT NULL,
    queue_id       INTEGER NOT NULL,
    body           TEXT    NOT NULL,
    receipt_handle TEXT    NOT NULL UNIQUE,
    visible        INTEGER NOT NULL DEFAULT 1,
    created_at     TEXT    NOT NULL
);

-- ─── AWS SNS ──────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS sns_topics (
    arn        TEXT PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sns_subscriptions (
    arn        TEXT PRIMARY KEY,
    topic_arn  TEXT NOT NULL,
    protocol   TEXT NOT NULL,
    endpoint   TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sns_messages (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id TEXT    NOT NULL,
    topic_arn  TEXT    NOT NULL,
    subject    TEXT,
    body       TEXT    NOT NULL,
    created_at TEXT    NOT NULL
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
";

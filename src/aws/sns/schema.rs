//! SNS tables and additive migrations.

use crate::db::ensure_column;

/// Create SNS tables and add any columns introduced after first release.
pub fn migrate(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
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
);",
    )?;
    ensure_column(
        conn,
        "sns_topics",
        "attributes",
        "TEXT NOT NULL DEFAULT '{}'",
    )?;
    ensure_column(conn, "sns_topics", "tags", "TEXT NOT NULL DEFAULT '{}'")?;
    ensure_column(conn, "sns_topics", "fifo_seq", "INTEGER NOT NULL DEFAULT 0")?;
    ensure_column(
        conn,
        "sns_subscriptions",
        "attributes",
        "TEXT NOT NULL DEFAULT '{}'",
    )?;
    ensure_column(conn, "sns_messages", "message_attributes", "TEXT")?;
    ensure_column(
        conn,
        "sns_messages",
        "delivered",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    Ok(())
}

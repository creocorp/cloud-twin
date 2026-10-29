//! SQS tables and additive migrations.

use crate::db::ensure_column;

/// Create SQS tables and add any columns introduced after first release.
pub fn migrate(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
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
CREATE TABLE IF NOT EXISTS sqs_fifo_dedup (
    queue_id        INTEGER NOT NULL,
    dedup_id        TEXT    NOT NULL,
    message_id      TEXT    NOT NULL,
    sequence_number TEXT    NOT NULL,
    expires_at      INTEGER NOT NULL,
    PRIMARY KEY (queue_id, dedup_id)
);",
    )?;
    // Queue configuration (JSON maps of AWS attribute name -> string value).
    ensure_column(
        conn,
        "sqs_queues",
        "attributes",
        "TEXT NOT NULL DEFAULT '{}'",
    )?;
    ensure_column(conn, "sqs_queues", "tags", "TEXT NOT NULL DEFAULT '{}'")?;
    ensure_column(
        conn,
        "sqs_queues",
        "created_ts",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "sqs_queues",
        "modified_ts",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(conn, "sqs_queues", "fifo_seq", "INTEGER NOT NULL DEFAULT 0")?;
    // Message state; all timestamps are epoch milliseconds.
    ensure_column(
        conn,
        "sqs_messages",
        "visible_at",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "sqs_messages",
        "sent_at",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "sqs_messages",
        "receive_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(conn, "sqs_messages", "first_receive_at", "INTEGER")?;
    ensure_column(conn, "sqs_messages", "message_attributes", "TEXT")?;
    ensure_column(conn, "sqs_messages", "system_attributes", "TEXT")?;
    ensure_column(conn, "sqs_messages", "group_id", "TEXT")?;
    ensure_column(conn, "sqs_messages", "dedup_id", "TEXT")?;
    ensure_column(conn, "sqs_messages", "sequence_number", "TEXT")?;
    ensure_column(conn, "sqs_messages", "dlq_source_arn", "TEXT")?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS sqs_messages_queue_visible ON sqs_messages (queue_id, visible_at);",
    )?;
    Ok(())
}

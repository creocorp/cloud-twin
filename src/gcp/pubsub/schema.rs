//! Additive Pub/Sub schema migrations.
//!
//! The base `pubsub_*` tables are created by `db::DDL`; columns added later are
//! applied here with `ensure_column` so older database files keep working.

use crate::db::ensure_column;

/// Apply additive migrations for the Pub/Sub tables (idempotent).
pub fn migrate(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    ensure_column(
        conn,
        "pubsub_subscriptions",
        "ack_deadline_seconds",
        "INTEGER NOT NULL DEFAULT 10",
    )?;
    // Epoch milliseconds until which a pulled message is leased (hidden).
    ensure_column(conn, "pubsub_ackables", "lease_until", "INTEGER")?;
    ensure_column(
        conn,
        "pubsub_ackables",
        "delivery_attempt",
        "INTEGER NOT NULL DEFAULT 0",
    )
}

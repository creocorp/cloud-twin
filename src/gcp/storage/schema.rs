//! Additive Cloud Storage schema migrations.
//!
//! The base `gcs_*` tables are created by `db::DDL`; columns added later are
//! applied here with `ensure_column` so older database files keep working.

use crate::db::ensure_column;

/// Apply additive migrations for the Cloud Storage tables (idempotent).
pub fn migrate(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    // Optional object properties (user metadata, cacheControl, ...) as JSON.
    ensure_column(conn, "gcs_objects", "props", "TEXT NOT NULL DEFAULT '{}'")
}

//! S3 tables and additive migrations.

use crate::db::ensure_column;

/// Create S3 tables and add any columns introduced after first release.
pub fn migrate(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
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
-- Noncurrent object versions and delete markers of versioned buckets.
-- The current (visible) version of every key always lives in s3_objects.
CREATE TABLE IF NOT EXISTS s3_object_versions (
    bucket           TEXT    NOT NULL,
    key              TEXT    NOT NULL,
    version_id       TEXT    NOT NULL,
    seq              INTEGER NOT NULL,
    is_delete_marker INTEGER NOT NULL DEFAULT 0,
    content          BLOB    NOT NULL DEFAULT X'',
    content_type     TEXT    NOT NULL DEFAULT 'application/octet-stream',
    size             INTEGER NOT NULL DEFAULT 0,
    etag             TEXT    NOT NULL DEFAULT '',
    created_at       TEXT    NOT NULL,
    meta             TEXT    NOT NULL DEFAULT '{}',
    tags             TEXT    NOT NULL DEFAULT '[]',
    PRIMARY KEY (bucket, key, version_id)
);
CREATE INDEX IF NOT EXISTS s3_object_versions_key ON s3_object_versions (bucket, key, seq);
-- Per-bucket sub-resource documents (cors, lifecycle, notification, ...).
CREATE TABLE IF NOT EXISTS s3_bucket_configs (
    bucket TEXT NOT NULL,
    kind   TEXT NOT NULL,
    body   TEXT NOT NULL,
    PRIMARY KEY (bucket, kind)
);
CREATE TABLE IF NOT EXISTS s3_multipart_uploads (
    upload_id    TEXT PRIMARY KEY,
    bucket       TEXT NOT NULL,
    key          TEXT NOT NULL,
    content_type TEXT NOT NULL,
    meta         TEXT NOT NULL DEFAULT '{}',
    tags         TEXT NOT NULL DEFAULT '[]',
    created_at   TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS s3_multipart_parts (
    upload_id   TEXT    NOT NULL,
    part_number INTEGER NOT NULL,
    content     BLOB    NOT NULL,
    size        INTEGER NOT NULL,
    etag        TEXT    NOT NULL,
    meta        TEXT    NOT NULL DEFAULT '{}',
    created_at  TEXT    NOT NULL,
    PRIMARY KEY (upload_id, part_number)
);
CREATE TABLE IF NOT EXISTS s3_counters (
    name  TEXT PRIMARY KEY,
    value INTEGER NOT NULL
);",
    )?;
    ensure_column(
        conn,
        "s3_objects",
        "version_id",
        "TEXT NOT NULL DEFAULT 'null'",
    )?;
    ensure_column(conn, "s3_objects", "seq", "INTEGER NOT NULL DEFAULT 0")?;
    ensure_column(conn, "s3_objects", "meta", "TEXT NOT NULL DEFAULT '{}'")?;
    ensure_column(conn, "s3_objects", "tags", "TEXT NOT NULL DEFAULT '[]'")?;
    Ok(())
}

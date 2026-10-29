//! KMS tables and additive migrations.

/// Create KMS tables. Table names must start with `kms_` so reset can find them.
pub fn migrate(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS kms_keys (
            id                   TEXT PRIMARY KEY,
            description          TEXT NOT NULL DEFAULT '',
            key_usage            TEXT NOT NULL,
            key_spec             TEXT NOT NULL,
            origin               TEXT NOT NULL DEFAULT 'AWS_KMS',
            key_state            TEXT NOT NULL,
            created_at           REAL NOT NULL,
            deletion_date        REAL,
            pending_window_days  INTEGER,
            material             BLOB,
            rotation_enabled     INTEGER NOT NULL DEFAULT 0,
            rotation_period_days INTEGER NOT NULL DEFAULT 365,
            policy               TEXT NOT NULL,
            tags                 TEXT NOT NULL DEFAULT '[]',
            multi_region         INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS kms_aliases (
            name          TEXT PRIMARY KEY,
            target_key_id TEXT NOT NULL,
            created_at    REAL NOT NULL,
            updated_at    REAL NOT NULL
        );
        CREATE INDEX IF NOT EXISTS kms_aliases_target ON kms_aliases(target_key_id);",
    )
}

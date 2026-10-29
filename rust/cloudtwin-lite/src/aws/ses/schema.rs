//! SES tables and additive migrations.

use crate::db::ensure_column;

/// Create SES tables and add any columns introduced after first release.
pub fn migrate(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
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
CREATE TABLE IF NOT EXISTS ses_suppressed_destinations (
    email            TEXT PRIMARY KEY,
    reason           TEXT NOT NULL,
    message_id       TEXT,
    feedback_id      TEXT,
    last_update_time TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS ses_configuration_sets (
    name       TEXT PRIMARY KEY,
    options    TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS ses_event_destinations (
    configuration_set TEXT NOT NULL,
    name              TEXT NOT NULL,
    definition        TEXT NOT NULL DEFAULT '{}',
    PRIMARY KEY (configuration_set, name)
);
CREATE TABLE IF NOT EXISTS ses_receipt_rule_sets (
    name       TEXT PRIMARY KEY,
    active     INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS ses_receipt_rules (
    rule_set TEXT    NOT NULL,
    name     TEXT    NOT NULL,
    position INTEGER NOT NULL,
    rule     TEXT    NOT NULL DEFAULT '{}',
    PRIMARY KEY (rule_set, name)
);
CREATE TABLE IF NOT EXISTS ses_account_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS ses_events (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id        TEXT NOT NULL,
    event_type        TEXT NOT NULL,
    configuration_set TEXT,
    payload           TEXT NOT NULL,
    created_at        TEXT NOT NULL
);",
    )?;

    // Identity verification lifecycle, DKIM, MAIL FROM, and per-identity settings.
    ensure_column(conn, "ses_identities", "status", "TEXT NOT NULL DEFAULT 'SUCCESS'")?;
    ensure_column(conn, "ses_identities", "verification", "TEXT NOT NULL DEFAULT '{}'")?;
    ensure_column(conn, "ses_identities", "dkim_tokens", "TEXT NOT NULL DEFAULT '[]'")?;
    ensure_column(conn, "ses_identities", "dkim_signing_enabled", "INTEGER NOT NULL DEFAULT 1")?;
    ensure_column(conn, "ses_identities", "dkim_origin", "TEXT NOT NULL DEFAULT 'AWS_SES'")?;
    ensure_column(conn, "ses_identities", "mail_from_domain", "TEXT")?;
    ensure_column(
        conn,
        "ses_identities",
        "mail_from_behavior",
        "TEXT NOT NULL DEFAULT 'USE_DEFAULT_VALUE'",
    )?;
    ensure_column(conn, "ses_identities", "mail_from_verification", "TEXT NOT NULL DEFAULT '{}'")?;
    ensure_column(conn, "ses_identities", "feedback_forwarding", "INTEGER NOT NULL DEFAULT 1")?;
    ensure_column(conn, "ses_identities", "configuration_set", "TEXT")?;
    ensure_column(conn, "ses_identities", "tags", "TEXT NOT NULL DEFAULT '[]'")?;
    ensure_column(conn, "ses_identities", "last_checked_at", "TEXT")?;
    ensure_column(conn, "ses_identities", "last_success_at", "TEXT")?;

    // Outbound message details.
    ensure_column(conn, "ses_messages", "raw", "TEXT")?;
    ensure_column(conn, "ses_messages", "reply_to", "TEXT NOT NULL DEFAULT '[]'")?;
    ensure_column(conn, "ses_messages", "tags", "TEXT NOT NULL DEFAULT '[]'")?;
    ensure_column(conn, "ses_messages", "configuration_set", "TEXT")?;
    ensure_column(conn, "ses_messages", "feedback_forwarding_address", "TEXT")?;
    ensure_column(conn, "ses_messages", "recipient_status", "TEXT NOT NULL DEFAULT '{}'")?;
    Ok(())
}

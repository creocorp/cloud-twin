//! Cognito tables and additive migrations.

/// Create Cognito tables. Table names must start with `cognito_` so reset can
/// find them. Signing keys belong in `cloudtwin_meta`, which reset preserves.
pub fn migrate(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS cognito_user_pools (
            id       TEXT PRIMARY KEY,
            name     TEXT NOT NULL,
            data     TEXT NOT NULL DEFAULT '{}',
            domain   TEXT,
            created  REAL NOT NULL,
            modified REAL NOT NULL
        );
        CREATE TABLE IF NOT EXISTS cognito_clients (
            client_id TEXT PRIMARY KEY,
            pool_id   TEXT NOT NULL,
            name      TEXT NOT NULL,
            secret    TEXT,
            data      TEXT NOT NULL DEFAULT '{}',
            created   REAL NOT NULL,
            modified  REAL NOT NULL
        );
        CREATE INDEX IF NOT EXISTS cognito_clients_pool ON cognito_clients(pool_id);
        CREATE TABLE IF NOT EXISTS cognito_users (
            pool_id            TEXT NOT NULL,
            username           TEXT NOT NULL,
            sub                TEXT NOT NULL,
            attributes         TEXT NOT NULL DEFAULT '{}',
            password_hash      TEXT,
            password_salt      TEXT,
            status             TEXT NOT NULL,
            enabled            INTEGER NOT NULL DEFAULT 1,
            created            REAL NOT NULL,
            modified           REAL NOT NULL,
            tokens_valid_after REAL NOT NULL DEFAULT 0,
            PRIMARY KEY (pool_id, username)
        );
        CREATE INDEX IF NOT EXISTS cognito_users_sub ON cognito_users(sub);
        CREATE TABLE IF NOT EXISTS cognito_auth_codes (
            code                  TEXT PRIMARY KEY,
            pool_id               TEXT NOT NULL,
            client_id             TEXT NOT NULL,
            username              TEXT NOT NULL,
            redirect_uri          TEXT NOT NULL,
            scope                 TEXT NOT NULL DEFAULT '',
            code_challenge        TEXT,
            code_challenge_method TEXT,
            nonce                 TEXT,
            auth_time             INTEGER NOT NULL,
            expires_at            REAL NOT NULL,
            used                  INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS cognito_sessions (
            id         TEXT PRIMARY KEY,
            pool_id    TEXT NOT NULL,
            username   TEXT NOT NULL,
            auth_time  INTEGER NOT NULL,
            created    REAL NOT NULL,
            expires_at REAL NOT NULL
        );
        CREATE TABLE IF NOT EXISTS cognito_origins (
            origin_jti TEXT PRIMARY KEY,
            pool_id    TEXT NOT NULL,
            client_id  TEXT NOT NULL,
            username   TEXT NOT NULL,
            created    REAL NOT NULL,
            revoked    INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS cognito_refresh_tokens (
            token      TEXT PRIMARY KEY,
            pool_id    TEXT NOT NULL,
            client_id  TEXT NOT NULL,
            username   TEXT NOT NULL,
            origin_jti TEXT NOT NULL,
            scope      TEXT NOT NULL DEFAULT '',
            auth_time  INTEGER NOT NULL,
            created    REAL NOT NULL,
            expires_at REAL NOT NULL,
            revoked    INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS cognito_challenges (
            session    TEXT PRIMARY KEY,
            pool_id    TEXT NOT NULL,
            client_id  TEXT NOT NULL,
            username   TEXT NOT NULL,
            challenge  TEXT NOT NULL,
            expires_at REAL NOT NULL
        );
        CREATE TABLE IF NOT EXISTS cognito_codes (
            pool_id    TEXT NOT NULL,
            username   TEXT NOT NULL,
            kind       TEXT NOT NULL,
            code       TEXT NOT NULL,
            expires_at REAL NOT NULL,
            PRIMARY KEY (pool_id, username, kind)
        );",
    )
}

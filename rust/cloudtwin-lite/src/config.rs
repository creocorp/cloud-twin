use std::env;

/// Runtime configuration resolved from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// TCP port to listen on (default: 4793).
    pub port: u16,
    /// SQLite database path. Use `:memory:` for an in-process store.
    pub db_path: String,
}

impl Config {
    pub fn from_env() -> Self {
        Config {
            port: env::var("CLOUDTWIN_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(4793),
            db_path: env::var("CLOUDTWIN_DB_PATH")
                .unwrap_or_else(|_| "/data/cloudtwin-lite.db".to_string()),
        }
    }
}

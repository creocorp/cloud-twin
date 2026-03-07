//! Runtime configuration for the Lite binary.
//!
//! Think of this as a tiny hand-rolled `IOptions<T>` object from C# or a Python
//! settings object built from `os.environ`. The crate keeps configuration simple
//! and explicit rather than introducing a full config framework.

use std::env;

/// Runtime configuration resolved from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// TCP port to listen on (default: 4793).
    pub port: u16,
    /// SQLite database path. Use `:memory:` for an in-process store.
    pub db_path: String,
    /// Public base URL used for constructing SQS queue URLs.
    pub base_url: String,
    /// Azure storage account name (default: devstoreaccount1).
    pub azure_account: String,
    /// Azure Service Bus namespace (default: cloudtwin).
    pub azure_namespace: String,
    /// GCP project ID (default: cloudtwin).
    pub gcp_project: String,
}

impl Config {
    /// Resolve configuration from environment variables.
    ///
    /// The method is intentionally eager: read everything once, parse it once,
    /// and then pass an owned `Config` through the application.
    pub fn from_env() -> Self {
        let port: u16 = env::var("CLOUDTWIN_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(4793);
        Config {
            port,
            db_path: env::var("CLOUDTWIN_DB_PATH")
                .unwrap_or_else(|_| "/data/cloudtwin-lite.db".to_string()),
            base_url: env::var("CLOUDTWIN_BASE_URL")
                .unwrap_or_else(|_| format!("http://localhost:{port}")),
            azure_account: env::var("CLOUDTWIN_AZURE_ACCOUNT")
                .unwrap_or_else(|_| "devstoreaccount1".to_string()),
            azure_namespace: env::var("CLOUDTWIN_AZURE_NAMESPACE")
                .unwrap_or_else(|_| "cloudtwin".to_string()),
            gcp_project: env::var("CLOUDTWIN_GCP_PROJECT")
                .unwrap_or_else(|_| "cloudtwin".to_string()),
        }
    }
}

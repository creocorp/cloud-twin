//! Runtime configuration for the Lite binary.
//!
//! Matches the Python backend's priority chain (highest → lowest):
//!   1. Environment variables  (`CLOUDTWIN_*`)
//!   2. YAML file at `CLOUDTWIN_CONFIG_PATH`  (default `/config/cloudtwin.yml`)
//!   3. Built-in defaults
//!
//! The YAML schema is the same `cloudtwin:` document that the Python backend reads,
//! so both processes share a single config file.

use std::env;

use serde::Deserialize;

use crate::aws::bedrock::BedrockSimConfig;

// ─────────────────────────────────────────────────────────────────────────────
// Resolved (post-merge) config handed to AppState
// ─────────────────────────────────────────────────────────────────────────────

/// Runtime configuration handed to every handler via `AppState`.
#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub db_path: String,
    pub base_url: String,
    pub azure_account: String,
    pub azure_namespace: String,
    pub gcp_project: String,
    pub dashboard_static_path: Option<String>,
    /// Bedrock simulation config loaded from the `bedrock:` YAML section.
    pub bedrock: BedrockSimConfig,
}

impl Config {
    /// Load configuration: env vars override YAML values which override defaults.
    pub fn load() -> Self {
        let yaml = YamlRoot::load();

        let port: u16 = env::var("CLOUDTWIN_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(yaml.api_port.unwrap_or(4793));

        let db_path = env::var("CLOUDTWIN_DB_PATH").unwrap_or_else(|_| {
            yaml.storage
                .as_ref()
                .and_then(|s| s.path.clone())
                .unwrap_or_else(|| "/data/cloudtwin-lite.db".to_string())
        });

        let base_url =
            env::var("CLOUDTWIN_BASE_URL").unwrap_or_else(|_| format!("http://localhost:{port}"));

        let azure_account = env::var("CLOUDTWIN_AZURE_ACCOUNT").unwrap_or_else(|_| {
            yaml.providers
                .as_ref()
                .and_then(|p| p.azure.as_ref())
                .and_then(|a| a.blob.as_ref())
                .and_then(|b| b.account_name.clone())
                .unwrap_or_else(|| "devstoreaccount1".to_string())
        });

        let azure_namespace = env::var("CLOUDTWIN_AZURE_NAMESPACE").unwrap_or_else(|_| {
            yaml.providers
                .as_ref()
                .and_then(|p| p.azure.as_ref())
                .and_then(|a| a.servicebus.as_ref())
                .and_then(|s| s.namespace.clone())
                .unwrap_or_else(|| "cloudtwin".to_string())
        });

        let gcp_project = env::var("CLOUDTWIN_GCP_PROJECT").unwrap_or_else(|_| {
            yaml.providers
                .as_ref()
                .and_then(|p| p.gcp.as_ref())
                .and_then(|g| g.project.clone())
                .unwrap_or_else(|| "cloudtwin".to_string())
        });

        let dashboard_static_path = env::var("CLOUDTWIN_DASHBOARD_STATIC").ok();

        let bedrock = yaml.bedrock.unwrap_or_default();

        Config {
            port,
            db_path,
            base_url,
            azure_account,
            azure_namespace,
            gcp_project,
            dashboard_static_path,
            bedrock,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Raw YAML shape  (mirrors the Python `cloudtwin:` document)
// ─────────────────────────────────────────────────────────────────────────────

/// The `cloudtwin:` top-level block (or bare root) from the YAML file.
#[derive(Debug, Default, Deserialize)]
struct YamlRoot {
    storage: Option<YamlStorage>,
    providers: Option<YamlProviders>,
    #[allow(dead_code)]
    dashboard: Option<YamlDashboard>,
    #[allow(dead_code)]
    logging: Option<YamlLogging>,
    api_port: Option<u16>,
    bedrock: Option<BedrockSimConfig>,
}

impl YamlRoot {
    fn load() -> Self {
        let path = env::var("CLOUDTWIN_CONFIG_PATH")
            .unwrap_or_else(|_| "/config/cloudtwin.yml".to_string());

        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };

        // The Python backend wraps everything under a `cloudtwin:` key.
        // Support both the wrapped form and the bare form.
        let raw: serde_yaml::Value = match serde_yaml::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Failed to parse {path}: {e}");
                return Self::default();
            }
        };

        let inner = match raw.get("cloudtwin") {
            Some(v) => v.clone(),
            None => raw,
        };

        match serde_yaml::from_value(inner) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::warn!("Failed to deserialize config from {path}: {e}");
                Self::default()
            }
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct YamlStorage {
    #[allow(dead_code)]
    mode: Option<String>,
    path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct YamlProviders {
    azure: Option<YamlAzure>,
    gcp: Option<YamlGcp>,
}

#[derive(Debug, Default, Deserialize)]
struct YamlAzure {
    blob: Option<YamlAzureBlob>,
    servicebus: Option<YamlServiceBus>,
}

#[derive(Debug, Default, Deserialize)]
struct YamlAzureBlob {
    account_name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct YamlServiceBus {
    namespace: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct YamlGcp {
    project: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[allow(dead_code)]
struct YamlDashboard {
    enabled: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[allow(dead_code)]
struct YamlLogging {
    level: Option<String>,
}

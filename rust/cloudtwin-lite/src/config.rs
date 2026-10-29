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
use crate::aws::cognito::CognitoConfig;
use crate::aws::s3::S3Config;
use crate::aws::ses::SesConfig;

/// Environment lookup used while building config (swappable in tests).
pub type EnvFn<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Parse a boolean env value (`true/false/1/0/yes/no`).
pub fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

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
    /// TCP port for the mailbox SMTP capture listener.
    pub smtp_port: u16,
    /// Whether the mailbox feature is enabled.
    pub mailbox_enabled: bool,
    /// Bedrock simulation config loaded from the `bedrock:` YAML section.
    pub bedrock: BedrockSimConfig,
    /// `providers.aws.ses` — verification, sending account, enforcement.
    pub ses: SesConfig,
    /// `providers.aws.s3` — addressing options.
    pub s3: S3Config,
    /// `providers.aws.cognito` — OIDC issuer, signing key, token lifetimes.
    pub cognito: CognitoConfig,
    /// Resources to provision at startup (`seed:` block or `CLOUDTWIN_SEED_PATH`).
    pub seed: Option<serde_json::Value>,
}

impl Config {
    /// Load configuration: env vars override YAML values which override defaults.
    pub fn load() -> Self {
        let env_fn = |k: &str| env::var(k).ok();
        Self::build(YamlRoot::load(), &env_fn)
    }

    /// Defaults only — no YAML file, no environment. Used by tests.
    #[cfg(test)]
    pub fn test_defaults() -> Self {
        let mut cfg = Self::build(YamlRoot::default(), &|_| None);
        cfg.db_path = ":memory:".into();
        cfg.base_url = "http://localhost:4793".into();
        cfg
    }

    fn build(yaml: YamlRoot, env: EnvFn) -> Self {
        let port: u16 = env("CLOUDTWIN_PORT")
            .and_then(|s| s.parse().ok())
            .unwrap_or(yaml.api_port.unwrap_or(4793));

        let db_path = env("CLOUDTWIN_DB_PATH").unwrap_or_else(|| {
            yaml.storage
                .as_ref()
                .and_then(|s| s.path.clone())
                .unwrap_or_else(|| "/data/cloudtwin-lite.db".to_string())
        });

        let base_url =
            env("CLOUDTWIN_BASE_URL").unwrap_or_else(|| format!("http://localhost:{port}"));

        let azure_account = env("CLOUDTWIN_AZURE_ACCOUNT").unwrap_or_else(|| {
            yaml.providers
                .as_ref()
                .and_then(|p| p.azure.as_ref())
                .and_then(|a| a.blob.as_ref())
                .and_then(|b| b.account_name.clone())
                .unwrap_or_else(|| "devstoreaccount1".to_string())
        });

        let azure_namespace = env("CLOUDTWIN_AZURE_NAMESPACE").unwrap_or_else(|| {
            yaml.providers
                .as_ref()
                .and_then(|p| p.azure.as_ref())
                .and_then(|a| a.servicebus.as_ref())
                .and_then(|s| s.namespace.clone())
                .unwrap_or_else(|| "cloudtwin".to_string())
        });

        let gcp_project = env("CLOUDTWIN_GCP_PROJECT").unwrap_or_else(|| {
            yaml.providers
                .as_ref()
                .and_then(|p| p.gcp.as_ref())
                .and_then(|g| g.project.clone())
                .unwrap_or_else(|| "cloudtwin".to_string())
        });

        let dashboard_static_path = env("CLOUDTWIN_DASHBOARD_STATIC");

        let smtp_port: u16 = env("CLOUDTWIN_SMTP_PORT")
            .and_then(|s| s.parse().ok())
            .unwrap_or(yaml.mailbox.as_ref().and_then(|m| m.smtp_port).unwrap_or(1025));

        // Default to disabled (false) for mailbox
        let mailbox_enabled: bool = env("CLOUDTWIN_MAILBOX_ENABLED")
            .and_then(|s| parse_bool(&s))
            .unwrap_or(yaml.mailbox.as_ref().and_then(|m| m.enabled).unwrap_or(false));

        let bedrock = yaml.bedrock.unwrap_or_default();

        let aws = yaml.providers.as_ref().and_then(|p| p.aws.clone()).unwrap_or_default();
        let mut ses = aws.ses.unwrap_or_default();
        ses.apply_env(env);
        let mut s3 = aws.s3.unwrap_or_default();
        s3.apply_env(env);
        let mut cognito = aws.cognito.unwrap_or_default();
        cognito.apply_env(env);

        let seed = match env("CLOUDTWIN_SEED_PATH") {
            Some(path) => load_seed_file(&path).or(yaml.seed),
            None => yaml.seed,
        };

        Config {
            port,
            db_path,
            base_url,
            azure_account,
            azure_namespace,
            gcp_project,
            dashboard_static_path,
            smtp_port,
            mailbox_enabled,
            bedrock,
            ses,
            s3,
            cognito,
            seed,
        }
    }
}

/// Read a YAML or JSON seed document (optionally wrapped in `seed:`).
fn load_seed_file(path: &str) -> Option<serde_json::Value> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("Failed to read seed file {path}: {e}");
            return None;
        }
    };
    match serde_yaml::from_str::<serde_json::Value>(&text) {
        Ok(v) => Some(v.get("seed").cloned().unwrap_or(v)),
        Err(e) => {
            tracing::warn!("Failed to parse seed file {path}: {e}");
            None
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
    mailbox: Option<YamlMailbox>,
    seed: Option<serde_json::Value>,
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
    aws: Option<YamlAws>,
    azure: Option<YamlAzure>,
    gcp: Option<YamlGcp>,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct YamlAws {
    ses: Option<SesConfig>,
    s3: Option<S3Config>,
    cognito: Option<CognitoConfig>,
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

/// The `mailbox:` block — configures the built-in SMTP capture server.
#[derive(Debug, Default, Deserialize)]
struct YamlMailbox {
    enabled: Option<bool>,
    smtp_port: Option<u16>,
}
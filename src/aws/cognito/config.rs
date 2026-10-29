//! Cognito runtime configuration (`providers.aws.cognito` in `cloudtwin.yml`).

use serde::Deserialize;

use crate::config::EnvFn;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CognitoConfig {
    /// Public base URL used as the OIDC issuer prefix (`{issuer_base_url}/{userPoolId}`).
    /// Defaults to the server's `base_url`.
    pub issuer_base_url: Option<String>,
    /// PEM (PKCS#8 or PKCS#1) RSA private key used to sign tokens. When unset a
    /// key is generated once and persisted in the database.
    pub signing_key_path: Option<String>,
    /// Default token lifetimes in seconds when a client does not set its own.
    pub id_token_ttl_seconds: Option<u64>,
    pub access_token_ttl_seconds: Option<u64>,
    pub refresh_token_ttl_seconds: Option<u64>,
}

impl CognitoConfig {
    pub fn apply_env(&mut self, env: EnvFn) {
        if let Some(v) = env("CLOUDTWIN_COGNITO_ISSUER_BASE_URL") {
            self.issuer_base_url = Some(v);
        }
        if let Some(v) = env("CLOUDTWIN_COGNITO_SIGNING_KEY_PATH") {
            self.signing_key_path = Some(v);
        }
        if let Some(v) = env("CLOUDTWIN_COGNITO_ID_TOKEN_TTL_SECONDS").and_then(|s| s.parse().ok())
        {
            self.id_token_ttl_seconds = Some(v);
        }
        if let Some(v) =
            env("CLOUDTWIN_COGNITO_ACCESS_TOKEN_TTL_SECONDS").and_then(|s| s.parse().ok())
        {
            self.access_token_ttl_seconds = Some(v);
        }
        if let Some(v) =
            env("CLOUDTWIN_COGNITO_REFRESH_TOKEN_TTL_SECONDS").and_then(|s| s.parse().ok())
        {
            self.refresh_token_ttl_seconds = Some(v);
        }
    }
}

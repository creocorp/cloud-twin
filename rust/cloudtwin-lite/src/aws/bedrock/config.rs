//! Bedrock simulation configuration structs.
//!
//! Deserialized from the `bedrock:` section of the shared `cloudtwin.yml`
//! file (resolved via `CLOUDTWIN_CONFIG_PATH`, default `/config/cloudtwin.yml`)
//! by `crate::config::Config::load`. Unknown models fall back to the global
//! defaults so the server is useful even with no configuration.

use std::collections::HashMap;

use serde::Deserialize;

// ─────────────────────────────────────────────────────────────────────────────
// Top-level config
// ─────────────────────────────────────────────────────────────────────────────

/// Top-level parsed Bedrock simulation configuration.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct BedrockSimConfig {
    #[serde(default)]
    pub defaults: DefaultsConfig,
    #[serde(default)]
    pub models: HashMap<String, ModelSimConfig>,
}

impl BedrockSimConfig {
    /// Return the effective config for a model, falling back to a stub that uses
    /// the global default mode when the model has no explicit entry.
    pub fn get_model_config<'a>(&'a self, model_id: &str) -> ModelSimConfigRef<'a> {
        if let Some(m) = self.models.get(model_id) {
            ModelSimConfigRef::Owned(m)
        } else {
            ModelSimConfigRef::Default(&self.defaults)
        }
    }
}

/// Thin accessor that avoids cloning the full model config.
pub enum ModelSimConfigRef<'a> {
    Owned(&'a ModelSimConfig),
    Default(&'a DefaultsConfig),
}

impl<'a> ModelSimConfigRef<'a> {
    pub fn mode(&self) -> &str {
        match self {
            Self::Owned(m) => m.mode.as_deref().unwrap_or("text"),
            Self::Default(d) => &d.mode,
        }
    }
    pub fn static_payload(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Owned(m) => m.static_payload.as_ref(),
            Self::Default(_) => None,
        }
    }
    pub fn schema(&self) -> Option<&SchemaNodeConfig> {
        match self {
            Self::Owned(m) => m.schema.as_ref(),
            Self::Default(_) => None,
        }
    }
    pub fn text_config(&self) -> Option<&TextConfig> {
        match self {
            Self::Owned(m) => m.text.as_ref(),
            Self::Default(_) => None,
        }
    }
    pub fn sequence(&self) -> Option<&SequenceConfig> {
        match self {
            Self::Owned(m) => m.sequence.as_ref(),
            Self::Default(_) => None,
        }
    }
    pub fn rules(&self) -> &[RuleConfig] {
        match self {
            Self::Owned(m) => &m.rules,
            Self::Default(_) => &[],
        }
    }
    pub fn error_injections(&self) -> &[ErrorInjectConfig] {
        match self {
            Self::Owned(m) => &m.errors,
            Self::Default(_) => &[],
        }
    }
    pub fn streaming(&self) -> Option<&StreamingConfig> {
        match self {
            Self::Owned(m) => m.streaming.as_ref(),
            Self::Default(_) => None,
        }
    }
    pub fn latency(&self) -> Option<&LatencyConfig> {
        match self {
            Self::Owned(m) => m.latency.as_ref(),
            Self::Default(d) => d.latency.as_ref(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Sub-configs
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct DefaultsConfig {
    #[serde(default = "default_mode")]
    pub mode: String,
    pub latency: Option<LatencyConfig>,
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self {
            mode: "text".to_string(),
            latency: None,
        }
    }
}

fn default_mode() -> String {
    "text".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LatencyConfig {
    pub min_ms: u64,
    pub max_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct StreamingConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_chunk_mode")]
    pub chunk_mode: String,
    #[serde(default = "default_fixed_chunk_size")]
    pub fixed_chunk_size: usize,
    #[serde(default)]
    pub first_chunk_delay_ms: u64,
    #[serde(default)]
    pub chunk_delay_ms: u64,
    pub fail_after_chunks: Option<usize>,
}

fn default_chunk_mode() -> String {
    "word".to_string()
}

fn default_fixed_chunk_size() -> usize {
    10
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TextConfig {
    #[serde(default = "default_template")]
    pub template: String,
    #[serde(default = "default_min_words")]
    pub min_words: usize,
    #[serde(default = "default_max_words")]
    pub max_words: usize,
}

fn default_template() -> String {
    "lipsum".to_string()
}
fn default_min_words() -> usize {
    5
}
fn default_max_words() -> usize {
    15
}

/// Simplified JSON Schema node for recursive fake-data generation.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SchemaNodeConfig {
    #[serde(rename = "type", default = "default_schema_type")]
    pub kind: String,
    #[serde(default)]
    pub properties: HashMap<String, SchemaNodeConfig>,
    pub items: Option<Box<SchemaNodeConfig>>,
    #[serde(rename = "enum", default)]
    pub items_enum: Option<Vec<serde_json::Value>>,
}

fn default_schema_type() -> String {
    "string".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SequenceConfig {
    #[serde(default = "default_sequence_mode")]
    pub mode: String,
    #[serde(default)]
    pub responses: Vec<ResponseEntry>,
}

fn default_sequence_mode() -> String {
    "sequence".to_string()
}

/// One entry in a sequence list or a rule response.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResponseEntry {
    #[serde(rename = "static")]
    pub static_payload: Option<serde_json::Value>,
    pub mode: Option<String>,
    pub text: Option<TextConfig>,
    pub error: Option<ErrorEntry>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ErrorEntry {
    #[serde(rename = "type", default = "default_error_type")]
    pub kind: String,
    #[serde(default = "default_error_message")]
    pub message: String,
}

fn default_error_type() -> String {
    "ThrottlingException".to_string()
}
fn default_error_message() -> String {
    "Synthetic error".to_string()
}

/// Prompt-based matching rule.  First match wins.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RuleConfig {
    pub contains: Option<String>,
    pub response: Option<ResponseEntry>,
    pub error: Option<ErrorEntry>,
}

/// Periodic error injection — fires when `request_count % every == 0`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ErrorInjectConfig {
    #[serde(default = "default_every")]
    pub every: u64,
    #[serde(rename = "type", default = "default_error_type")]
    pub kind: String,
    #[serde(default = "default_error_message")]
    pub message: String,
}

fn default_every() -> u64 {
    5
}

/// Per-model configuration.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ModelSimConfig {
    /// Optional display name shown in ListFoundationModels.
    pub name: Option<String>,
    /// Optional provider name; derived from the model_id prefix if omitted.
    pub provider: Option<String>,
    pub mode: Option<String>,
    #[serde(rename = "static")]
    pub static_payload: Option<serde_json::Value>,
    pub schema: Option<SchemaNodeConfig>,
    pub text: Option<TextConfig>,
    pub sequence: Option<SequenceConfig>,
    #[serde(default)]
    pub rules: Vec<RuleConfig>,
    #[serde(default)]
    pub errors: Vec<ErrorInjectConfig>,
    pub streaming: Option<StreamingConfig>,
    pub latency: Option<LatencyConfig>,
}

impl ModelSimConfig {
    /// Effective display name: explicit `name` field or the model_id itself.
    pub fn effective_name<'a>(&'a self, model_id: &'a str) -> &'a str {
        self.name.as_deref().unwrap_or(model_id)
    }

    /// Effective provider: explicit `provider` field or the first `.`-delimited
    /// segment of the model_id (e.g. `"anthropic"` from `"anthropic.claude-…"`).
    pub fn effective_provider<'a>(&'a self, model_id: &'a str) -> &'a str {
        if let Some(p) = self.provider.as_deref() {
            return p;
        }
        model_id.split('.').next().unwrap_or("cloudtwin")
    }
}

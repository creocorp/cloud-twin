//! Scenario resolution pipeline.
//!
//! Resolution order (first match wins):
//!   1. Periodic error injection (errors[].every)
//!   2. Prompt-matching rules (rules[].contains)
//!   3. Sequence / cycle  
//!   4. Fallback based on mode (text / schema / static)

use serde_json::Value;

use super::{
    config::{LatencyConfig, ResponseEntry, StreamingConfig},
    generator::{generate_from_schema, generate_text},
    state::BedrockState,
};

// ─────────────────────────────────────────────────────────────────────────────
// Output type
// ─────────────────────────────────────────────────────────────────────────────

/// The outcome returned by `resolve()`.
#[derive(Debug)]
pub struct ResolvedResponse {
    /// "success" | "error"
    pub kind: String,
    /// Structured JSON body (schema/static outcomes).
    pub json_body: Option<Value>,
    /// Plain text body (text-mode outcomes).
    pub text_body: Option<String>,
    #[allow(dead_code)]
    pub model_id: String,
    #[allow(dead_code)]
    pub request_count: u64,
    /// Human-readable label indicating how the response was resolved.
    #[allow(dead_code)]
    pub source: String,
    #[allow(dead_code)]
    pub streaming: bool,
    pub streaming_config: Option<StreamingConfig>,
    /// Effective latency to apply before sending the response.
    pub latency_config: Option<LatencyConfig>,
    /// Error details when `kind == "error"`.
    pub error_type: Option<String>,
    pub error_message: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Main entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Resolve a response for `model_id` given the current simulation `state`.
pub fn resolve(state: &BedrockState, model_id: &str, prompt_text: &str) -> ResolvedResponse {
    let cfg_ref = state.config.get_model_config(model_id);
    let request_count = state.increment(model_id);

    let streaming_cfg = cfg_ref.streaming().cloned();
    let streaming_enabled = streaming_cfg.as_ref().is_some_and(|s| s.enabled);
    let latency_cfg = cfg_ref.latency().cloned();

    // ── 1. Periodic error injection ─────────────────────────────────────────
    for inject in cfg_ref.error_injections() {
        if inject.every > 0 && request_count.is_multiple_of(inject.every) {
            return error_response(
                model_id,
                request_count,
                &inject.kind,
                &inject.message,
                "error_injection",
                streaming_enabled,
                streaming_cfg,
                latency_cfg,
            );
        }
    }

    // ── 2. Prompt-matching rules ─────────────────────────────────────────────
    let prompt_lower = prompt_text.to_lowercase();
    for rule in cfg_ref.rules() {
        let matches = rule
            .contains
            .as_ref()
            .is_some_and(|needle| prompt_lower.contains(needle.as_str()));
        if matches {
            if let Some(err) = &rule.error {
                return error_response(
                    model_id,
                    request_count,
                    &err.kind,
                    &err.message,
                    "rule_error",
                    streaming_enabled,
                    streaming_cfg,
                    latency_cfg,
                );
            }
            if let Some(resp) = &rule.response {
                return resolve_response_entry(
                    resp,
                    model_id,
                    request_count,
                    "rule",
                    streaming_enabled,
                    streaming_cfg,
                    latency_cfg,
                );
            }
        }
    }

    // ── 3. Sequence / cycle ──────────────────────────────────────────────────
    if let Some(seq) = cfg_ref.sequence() {
        if !seq.responses.is_empty() {
            let idx = if seq.mode == "cycle" {
                ((request_count - 1) as usize) % seq.responses.len()
            } else {
                // sequence: clamp to last entry
                ((request_count - 1) as usize).min(seq.responses.len() - 1)
            };
            let entry = &seq.responses[idx];
            return resolve_response_entry(
                entry,
                model_id,
                request_count,
                &format!("sequence[{idx}]"),
                streaming_enabled,
                streaming_cfg,
                latency_cfg,
            );
        }
    }

    // ── 4. Fallback based on mode ────────────────────────────────────────────
    let mode = cfg_ref.mode();
    match mode {
        "schema" => {
            let schema = cfg_ref.schema().cloned().unwrap_or_default();
            let value =
                generate_from_schema(&schema, model_id, request_count, "");
            ResolvedResponse {
                kind: "success".into(),
                json_body: Some(value),
                text_body: None,
                model_id: model_id.to_string(),
                request_count,
                source: "schema".into(),
                streaming: streaming_enabled,
                streaming_config: streaming_cfg,
                latency_config: latency_cfg,
                error_type: None,
                error_message: None,
            }
        }
        "static" => {
            let payload = cfg_ref.static_payload().cloned().unwrap_or(Value::Null);
            ResolvedResponse {
                kind: "success".into(),
                json_body: Some(payload),
                text_body: None,
                model_id: model_id.to_string(),
                request_count,
                source: "static".into(),
                streaming: streaming_enabled,
                streaming_config: streaming_cfg,
                latency_config: latency_cfg,
                error_type: None,
                error_message: None,
            }
        }
        _ => {
            // default: text
            let text = generate_text(model_id, request_count, cfg_ref.text_config());
            ResolvedResponse {
                kind: "success".into(),
                json_body: None,
                text_body: Some(text),
                model_id: model_id.to_string(),
                request_count,
                source: "text".into(),
                streaming: streaming_enabled,
                streaming_config: streaming_cfg,
                latency_config: latency_cfg,
                error_type: None,
                error_message: None,
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn error_response(
    model_id: &str,
    request_count: u64,
    error_type: &str,
    error_message: &str,
    source: &str,
    streaming: bool,
    streaming_config: Option<StreamingConfig>,
    latency_config: Option<LatencyConfig>,
) -> ResolvedResponse {
    ResolvedResponse {
        kind: "error".into(),
        json_body: None,
        text_body: None,
        model_id: model_id.to_string(),
        request_count,
        source: source.to_string(),
        streaming,
        streaming_config,
        latency_config,
        error_type: Some(error_type.to_string()),
        error_message: Some(error_message.to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_response_entry(
    entry: &ResponseEntry,
    model_id: &str,
    request_count: u64,
    source: &str,
    streaming: bool,
    streaming_config: Option<StreamingConfig>,
    latency_config: Option<LatencyConfig>,
) -> ResolvedResponse {
    if let Some(err) = &entry.error {
        return error_response(
            model_id,
            request_count,
            &err.kind,
            &err.message,
            source,
            streaming,
            streaming_config,
            latency_config,
        );
    }

    if let Some(payload) = &entry.static_payload {
        return ResolvedResponse {
            kind: "success".into(),
            json_body: Some(payload.clone()),
            text_body: None,
            model_id: model_id.to_string(),
            request_count,
            source: source.to_string(),
            streaming,
            streaming_config,
            latency_config,
            error_type: None,
            error_message: None,
        };
    }

    // text mode (default within entry)
    let text = generate_text(model_id, request_count, entry.text.as_ref());
    ResolvedResponse {
        kind: "success".into(),
        json_body: None,
        text_body: Some(text),
        model_id: model_id.to_string(),
        request_count,
        source: source.to_string(),
        streaming,
        streaming_config,
        latency_config,
        error_type: None,
        error_message: None,
    }
}

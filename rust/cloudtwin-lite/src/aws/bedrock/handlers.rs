//! Bedrock REST handlers.
//!
//! Three endpoints are exposed:
//!   GET  /foundation-models                          → ListFoundationModels
//!   POST /model/:model_id/invoke                     → InvokeModel
//!   POST /model/:model_id/invoke-with-response-stream → InvokeModelWithResponseStream

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use serde_json::json;

use super::{
    scenario_engine::resolve,
    streaming::{build_error_streaming_body, build_streaming_body},
};
use crate::AppState;

// ─────────────────────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────────────────────

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/foundation-models", get(list_foundation_models))
        .route("/model/:model_id/invoke", post(invoke_model))
        .route(
            "/model/:model_id/invoke-with-response-stream",
            post(invoke_model_stream),
        )
}

// ─────────────────────────────────────────────────────────────────────────────
// Static foundation-model stubs
// ─────────────────────────────────────────────────────────────────────────────

static FOUNDATION_MODELS: &[(&str, &str, &str)] = &[
    ("amazon.titan-text-express-v1", "Amazon Titan Text Express", "amazon"),
    ("anthropic.claude-3-sonnet-20240229-v1:0", "Claude 3 Sonnet", "anthropic"),
    ("anthropic.claude-3-haiku-20240307-v1:0", "Claude 3 Haiku", "anthropic"),
    ("meta.llama3-8b-instruct-v1:0", "Llama 3 8B Instruct", "meta"),
    ("mistral.mistral-7b-instruct-v0:2", "Mistral 7B Instruct", "mistral"),
];

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

async fn list_foundation_models(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Include configured models so tests can discover their test stubs.
    let mut model_summaries: Vec<serde_json::Value> = FOUNDATION_MODELS
        .iter()
        .map(|(id, name, provider)| {
            json!({
                "modelId": id,
                "modelName": name,
                "providerName": provider,
                "responseStreamingSupported": true,
                "inputModalities": ["TEXT"],
                "outputModalities": ["TEXT"],
                "modelLifecycle": { "status": "ACTIVE" },
            })
        })
        .collect();

    // Append any models from config that aren't in the static list.
    let static_ids: Vec<&str> = FOUNDATION_MODELS.iter().map(|(id, _, _)| *id).collect();
    for model_id in state.bedrock.config.models.keys() {
        if !static_ids.contains(&model_id.as_str()) {
            model_summaries.push(json!({
                "modelId": model_id,
                "modelName": model_id,
                "providerName": "cloudtwin",
                "responseStreamingSupported": true,
                "inputModalities": ["TEXT"],
                "outputModalities": ["TEXT"],
                "modelLifecycle": { "status": "ACTIVE" },
            }));
        }
    }

    json_response(StatusCode::OK, &json!({ "modelSummaries": model_summaries }))
}

async fn invoke_model(
    State(state): State<Arc<AppState>>,
    Path(model_id): Path<String>,
    body: Bytes,
) -> Response {
    let body_val: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => serde_json::Value::Object(Default::default()),
    };

    let prompt = extract_prompt(&body_val);
    let resolved = resolve(&state.bedrock, &model_id, &prompt);

    if resolved.kind == "error" {
        let error_type = resolved.error_type.as_deref().unwrap_or("ThrottlingException");
        let error_message = resolved.error_message.as_deref().unwrap_or("Synthetic error");
        return (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "application/json")],
            serde_json::to_string(&json!({
                "message": error_message,
                "__type": error_type,
            }))
            .unwrap(),
        )
            .into_response();
    }

    let response_body = if let Some(text) = resolved.text_body {
        json!({
            "content": text,
            "stop_reason": "end_turn",
            "model": model_id,
            "usage": { "input_tokens": 10, "output_tokens": 20 },
        })
    } else {
        resolved.json_body.unwrap_or(serde_json::Value::Null)
    };

    json_response(StatusCode::OK, &response_body)
}

async fn invoke_model_stream(
    State(state): State<Arc<AppState>>,
    Path(model_id): Path<String>,
    body: Bytes,
) -> Response {
    let body_val: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => serde_json::Value::Object(Default::default()),
    };

    let prompt = extract_prompt(&body_val);
    let resolved = resolve(&state.bedrock, &model_id, &prompt);

    if resolved.kind == "error" {
        let error_type = resolved.error_type.as_deref().unwrap_or("ThrottlingException");
        let error_message = resolved.error_message.as_deref().unwrap_or("Synthetic error");
        let event_bytes = build_error_streaming_body(error_type, error_message);
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/vnd.amazon.eventstream")],
            event_bytes,
        )
            .into_response();
    }

    // Determine content to stream
    let content = if let Some(text) = resolved.text_body {
        text
    } else if let Some(json_body) = resolved.json_body {
        serde_json::to_string(&json_body).unwrap_or_default()
    } else {
        String::new()
    };

    let default_streaming = super::config::StreamingConfig {
        enabled: true,
        chunk_mode: "word".to_string(),
        ..Default::default()
    };
    let streaming_cfg = resolved
        .streaming_config
        .as_ref()
        .unwrap_or(&default_streaming);

    let event_bytes = build_streaming_body(&content, streaming_cfg);

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/vnd.amazon.eventstream")],
        event_bytes,
    )
        .into_response()
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn json_response(status: StatusCode, body: &serde_json::Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(body).unwrap(),
    )
        .into_response()
}

/// Extract a text prompt from various Bedrock request body formats.
fn extract_prompt(body: &serde_json::Value) -> String {
    if let Some(p) = body.get("prompt").and_then(|v| v.as_str()) {
        return p.to_string();
    }
    if let Some(p) = body.get("inputText").and_then(|v| v.as_str()) {
        return p.to_string();
    }
    if let Some(messages) = body.get("messages").and_then(|v| v.as_array()) {
        let mut parts = Vec::new();
        for msg in messages {
            let content = msg.get("content");
            if let Some(c) = content {
                if let Some(s) = c.as_str() {
                    parts.push(s.to_string());
                } else if let Some(arr) = c.as_array() {
                    for item in arr {
                        if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                            if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                                parts.push(t.to_string());
                            }
                        }
                    }
                }
            }
        }
        if !parts.is_empty() {
            return parts.join(" ");
        }
    }
    serde_json::to_string(body).unwrap_or_default()
}

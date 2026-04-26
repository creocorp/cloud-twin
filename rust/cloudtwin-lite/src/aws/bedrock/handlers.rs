//! Bedrock REST handlers.
//!
//! Three endpoints are exposed:
//!   GET  /foundation-models                          → ListFoundationModels
//!   POST /model/:model_id/invoke                     → InvokeModel
//!   POST /model/:model_id/invoke-with-response-stream → InvokeModelWithResponseStream

use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use rand::Rng;
use serde_json::json;

use super::{
    config::LatencyConfig,
    scenario_engine::{resolve, ResolvedResponse},
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
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

async fn list_foundation_models(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let model_summaries: Vec<serde_json::Value> = state
        .bedrock
        .config
        .models
        .iter()
        .map(|(model_id, model)| {
            json!({
                "modelId":                    model_id,
                "modelName":                  model.effective_name(model_id),
                "providerName":               model.effective_provider(model_id),
                "responseStreamingSupported": true,
                "inputModalities":            ["TEXT"],
                "outputModalities":           ["TEXT"],
                "modelLifecycle":             { "status": "ACTIVE" },
            })
        })
        .collect();

    json_response(StatusCode::OK, &json!({ "modelSummaries": model_summaries }))
}

async fn invoke_model(
    State(state): State<Arc<AppState>>,
    Path(model_id): Path<String>,
    body: Bytes,
) -> Response {
    let body_val: serde_json::Value =
        serde_json::from_slice(&body).unwrap_or_else(|_| serde_json::Value::Object(Default::default()));

    let prompt = extract_prompt(&body_val);
    let resolved = resolve(&state.bedrock, &model_id, &prompt);

    apply_latency(resolved.latency_config.as_ref()).await;

    let parity_headers = build_parity_headers(&resolved);

    if resolved.kind == "error" {
        let error_type = resolved.error_type.as_deref().unwrap_or("ThrottlingException");
        let error_message = resolved.error_message.as_deref().unwrap_or("Synthetic error");
        return (
            StatusCode::BAD_REQUEST,
            parity_headers,
            [(header::CONTENT_TYPE, "application/json")],
            serde_json::to_string(&json!({
                "message": error_message,
                "__type": error_type,
            }))
            .unwrap(),
        )
            .into_response();
    }

    let response_body = if let Some(text) = &resolved.text_body {
        json!({
            "content": text,
            "stop_reason": "end_turn",
            "model": model_id,
            "usage": { "input_tokens": 10, "output_tokens": 20 },
        })
    } else {
        resolved.json_body.clone().unwrap_or(serde_json::Value::Null)
    };

    (
        StatusCode::OK,
        parity_headers,
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&response_body).unwrap(),
    )
        .into_response()
}

async fn invoke_model_stream(
    State(state): State<Arc<AppState>>,
    Path(model_id): Path<String>,
    body: Bytes,
) -> Response {
    let body_val: serde_json::Value =
        serde_json::from_slice(&body).unwrap_or_else(|_| serde_json::Value::Object(Default::default()));

    let prompt = extract_prompt(&body_val);
    let resolved = resolve(&state.bedrock, &model_id, &prompt);

    apply_latency(resolved.latency_config.as_ref()).await;

    let parity_headers = build_parity_headers(&resolved);

    if resolved.kind == "error" {
        let error_type = resolved.error_type.as_deref().unwrap_or("ThrottlingException");
        let error_message = resolved.error_message.as_deref().unwrap_or("Synthetic error");
        let event_bytes = build_error_streaming_body(error_type, error_message);
        return (
            StatusCode::OK,
            parity_headers,
            [(header::CONTENT_TYPE, "application/vnd.amazon.eventstream")],
            event_bytes,
        )
            .into_response();
    }

    // Determine content to stream
    let content = if let Some(text) = &resolved.text_body {
        text.clone()
    } else if let Some(json_body) = &resolved.json_body {
        serde_json::to_string(json_body).unwrap_or_default()
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
        parity_headers,
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

/// Build the `x-cloudtwin-*` headers that mirror the Python backend so the
/// dashboard's Bedrock chat panel can show request counts and resolution source.
fn build_parity_headers(resolved: &ResolvedResponse) -> HeaderMap {
    let mut h = HeaderMap::new();
    if let Ok(v) = HeaderValue::from_str(&resolved.request_count.to_string()) {
        h.insert(HeaderName::from_static("x-cloudtwin-request-count"), v);
    }
    if let Ok(v) = HeaderValue::from_str(&resolved.source) {
        h.insert(HeaderName::from_static("x-cloudtwin-response-source"), v);
    }
    h
}

/// Sleep for a random duration within the configured latency range.
async fn apply_latency(latency: Option<&LatencyConfig>) {
    let Some(l) = latency else { return };
    if l.min_ms == 0 && l.max_ms == 0 {
        return;
    }
    let ms = if l.min_ms >= l.max_ms {
        l.min_ms
    } else {
        rand::thread_rng().gen_range(l.min_ms..=l.max_ms)
    };
    tokio::time::sleep(Duration::from_millis(ms)).await;
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

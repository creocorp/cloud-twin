//! AWS EventStream binary framing for Bedrock streaming responses.
//!
//! Frame layout (all integers big-endian):
//!   [total_length: 4]  [headers_length: 4]  [prelude_crc: 4]
//!   [headers: headers_length]  [payload: variable]
//!   [message_crc: 4]
//!
//! Header encoding:
//!   [name_len: 1]  [name: name_len]  [value_type: 1 (=7 for string)]
//!   [value_len: 2]  [value: value_len]

use super::config::StreamingConfig;

// ─────────────────────────────────────────────────────────────────────────────
// Low-level frame encoding
// ─────────────────────────────────────────────────────────────────────────────

fn encode_header(name: &str, value: &str) -> Vec<u8> {
    let n = name.as_bytes();
    let v = value.as_bytes();
    let mut buf = Vec::with_capacity(1 + n.len() + 1 + 2 + v.len());
    buf.push(n.len() as u8);
    buf.extend_from_slice(n);
    buf.push(7u8); // value type: string
    let vlen = v.len() as u16;
    buf.extend_from_slice(&vlen.to_be_bytes());
    buf.extend_from_slice(v);
    buf
}

/// Encode a single EventStream message frame.
pub fn encode_event_stream_message(headers: &[(&str, &str)], payload: &[u8]) -> Vec<u8> {
    let headers_data: Vec<u8> = headers
        .iter()
        .flat_map(|(name, value)| encode_header(name, value))
        .collect();

    let total_length = (16 + headers_data.len() + payload.len()) as u32;
    let headers_length = headers_data.len() as u32;

    let mut prelude = [0u8; 8];
    prelude[..4].copy_from_slice(&total_length.to_be_bytes());
    prelude[4..8].copy_from_slice(&headers_length.to_be_bytes());

    let prelude_crc = crc32fast::hash(&prelude);

    // Build message without trailing CRC
    let mut msg = Vec::with_capacity(total_length as usize);
    msg.extend_from_slice(&prelude);
    msg.extend_from_slice(&prelude_crc.to_be_bytes());
    msg.extend_from_slice(&headers_data);
    msg.extend_from_slice(payload);

    let message_crc = crc32fast::hash(&msg);
    msg.extend_from_slice(&message_crc.to_be_bytes());

    msg
}

// ─────────────────────────────────────────────────────────────────────────────
// Chunk splitters
// ─────────────────────────────────────────────────────────────────────────────

/// Split `text` into chunks according to the streaming config.
pub fn split_chunks(text: &str, cfg: &StreamingConfig) -> Vec<String> {
    match cfg.chunk_mode.as_str() {
        "word" => text
            .split_whitespace()
            .map(|w| format!("{w} "))
            .collect(),
        "char" => text.chars().map(|c| c.to_string()).collect(),
        "fixed" => {
            let sz = cfg.fixed_chunk_size.max(1);
            let bytes = text.as_bytes();
            bytes
                .chunks(sz)
                .map(|c| String::from_utf8_lossy(c).into_owned())
                .collect()
        }
        _ => vec![text.to_string()],
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// High-level helpers
// ─────────────────────────────────────────────────────────────────────────────

fn chunk_event(text: &str) -> Vec<u8> {
    let payload_json = serde_json::json!({
        "bytes": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, serde_json::to_string(&serde_json::json!({
            "type": "content_block_delta",
            "delta": { "type": "text_delta", "text": text }
        })).unwrap())
    });
    let payload_bytes = serde_json::to_vec(&payload_json).unwrap();
    encode_event_stream_message(
        &[
            (":event-type", "chunk"),
            (":content-type", "application/json"),
            (":message-type", "event"),
        ],
        &payload_bytes,
    )
}

fn stop_event() -> Vec<u8> {
    let payload_json = serde_json::json!({
        "bytes": base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            serde_json::to_string(&serde_json::json!({
                "type": "message_stop",
                "stop_reason": "end_turn"
            })).unwrap()
        )
    });
    let payload_bytes = serde_json::to_vec(&payload_json).unwrap();
    encode_event_stream_message(
        &[
            (":event-type", "chunk"),
            (":content-type", "application/json"),
            (":message-type", "event"),
        ],
        &payload_bytes,
    )
}

fn error_event(error_type: &str, error_message: &str) -> Vec<u8> {
    let payload_json = serde_json::json!({
        "message": error_message,
        "__type": error_type,
    });
    let payload_bytes = serde_json::to_vec(&payload_json).unwrap();
    encode_event_stream_message(
        &[
            (":event-type", "modelStreamErrorException"),
            (":content-type", "application/json"),
            (":message-type", "exception"),
            (":exception-type", error_type),
        ],
        &payload_bytes,
    )
}

/// Build the complete EventStream body as a byte vector.
///
/// No true streaming is required — the body is collected in memory and
/// returned as a single bytes response, which satisfies the boto3 SDK.
pub fn build_streaming_body(content: &str, cfg: &StreamingConfig) -> Vec<u8> {
    let chunks = split_chunks(content, cfg);
    let mut body: Vec<u8> = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        if let Some(fail_after) = cfg.fail_after_chunks {
            if i >= fail_after {
                body.extend_from_slice(&error_event("ModelStreamErrorException", "Forced stream error"));
                return body;
            }
        }
        body.extend_from_slice(&chunk_event(chunk));
    }
    body.extend_from_slice(&stop_event());
    body
}

/// Build an error-only EventStream body.
pub fn build_error_streaming_body(error_type: &str, error_message: &str) -> Vec<u8> {
    error_event(error_type, error_message)
}

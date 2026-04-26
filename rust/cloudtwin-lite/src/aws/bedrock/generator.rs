//! Deterministic fake-data generator seeded by (model_id, field_path, request_count).
//!
//! Results are consistent for the same inputs within a process but intentionally
//! differ from the Python implementation — integration tests only check types,
//! not exact values.

use std::collections::HashMap;

use serde_json::Value;

use super::config::{SchemaNodeConfig, TextConfig};

static LIPSUM: &[&str] = &[
    "lorem", "ipsum", "dolor", "sit", "amet", "consectetur",
    "adipiscing", "elit", "sed", "do", "eiusmod", "tempor",
    "incididunt", "ut", "labore", "et", "dolore", "magna",
    "aliqua", "enim", "ad", "minim", "veniam", "quis",
    "nostrud", "exercitation", "ullamco", "laboris", "nisi",
    "aliquip", "ex", "ea", "commodo", "consequat",
];

static FLOATS: &[f64] = &[0.42, 0.73, 0.17, 0.85, 0.61, 0.33, 0.94, 0.28, 0.55, 0.79];
static INTEGERS: &[i64] = &[1, 7, 42, 13, 99, 5, 28, 64, 3, 17];

/// FNV-1a hash for a stable, deterministic seed value.
fn compute_seed(model_id: &str, field_path: &str, request_count: u64) -> u64 {
    let input = format!("{model_id}:{field_path}:{request_count}");
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for byte in input.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    hash
}

/// Pick an element from a non-empty slice using the given seed.
fn pick<T>(slice: &[T], seed: u64) -> &T {
    &slice[(seed as usize) % slice.len()]
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Generate lorem-ipsum text for a text-mode model.
pub fn generate_text(model_id: &str, request_count: u64, cfg: Option<&TextConfig>) -> String {
    let min_words = cfg.map_or(5, |c| c.min_words);
    let max_words = cfg.map_or(15, |c| c.max_words);

    let seed = compute_seed(model_id, "text", request_count);
    let range = (max_words - min_words + 1).max(1);
    let word_count = min_words + (seed as usize % range);

    (0..word_count)
        .map(|i| {
            let s = compute_seed(model_id, &format!("text.word.{i}"), request_count);
            *pick(LIPSUM, s)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Recursively generate a JSON value from a JSON-Schema-like node.
pub fn generate_from_schema(
    schema: &SchemaNodeConfig,
    model_id: &str,
    request_count: u64,
    path: &str,
) -> Value {
    // enum takes priority
    if let Some(values) = &schema.items_enum {
        if !values.is_empty() {
            let seed = compute_seed(model_id, path, request_count);
            return pick(values, seed).clone();
        }
    }

    match schema.kind.as_str() {
        "object" => {
            let mut map = serde_json::Map::new();
            let mut keys: Vec<&str> = schema.properties.keys().map(|s| s.as_str()).collect();
            keys.sort(); // stable ordering
            for key in keys {
                let child_schema = &schema.properties[key];
                let child_path = if path.is_empty() {
                    key.to_string()
                } else {
                    format!("{path}.{key}")
                };
                map.insert(
                    key.to_string(),
                    generate_from_schema(child_schema, model_id, request_count, &child_path),
                );
            }
            Value::Object(map)
        }
        "array" => {
            let seed = compute_seed(model_id, &format!("{path}.len"), request_count);
            let len = 1 + (seed as usize % 3); // 1..=3 items
            let default_item = SchemaNodeConfig {
                kind: "string".to_string(),
                properties: HashMap::new(),
                items: None,
                items_enum: None,
            };
            let item_schema = schema.items.as_deref().unwrap_or(&default_item);
            let items: Vec<Value> = (0..len)
                .map(|i| generate_from_schema(item_schema, model_id, request_count, &format!("{path}[{i}]")))
                .collect();
            Value::Array(items)
        }
        "number" => {
            let seed = compute_seed(model_id, path, request_count);
            Value::from(*pick(FLOATS, seed))
        }
        "integer" => {
            let seed = compute_seed(model_id, path, request_count);
            Value::from(*pick(INTEGERS, seed))
        }
        "boolean" => {
            let seed = compute_seed(model_id, path, request_count);
            Value::Bool(seed.is_multiple_of(2))
        }
        _ => {
            // string
            let seed = compute_seed(model_id, path, request_count);
            Value::String((*pick(LIPSUM, seed)).to_string())
        }
    }
}

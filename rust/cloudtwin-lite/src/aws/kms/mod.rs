//! KMS feature module (AWS JSON 1.1 protocol, `TrentService.*` targets).
//!
//! Symmetric keys hold real 256-bit AES-GCM material; see `service.rs` for the
//! ciphertext blob format.

mod handlers;
mod models;
pub mod schema;
mod service;
#[cfg(test)]
mod tests;

pub use handlers::handle_json;

use std::sync::Arc;

use anyhow::{bail, Context};
use serde_json::Value;

use crate::AppState;
use service::{seeded_material, CreateKeyInput, KmsService};

/// Provision keys:
/// `{"keys": [{"id"?: "<uuid>", "alias"?: "alias/name", "aliases"?: [...],
///   "description"?: "...", "enabled"?: true, "tags"?: {"k": "v"}}]}`.
///
/// Idempotent: an entry whose `id` already exists (or, without `id`, whose
/// first existing alias resolves to a key, or whose description matches an
/// existing key when it has neither) is not re-created; missing aliases are
/// added. Keys seeded with an explicit `id` get deterministic key material
/// derived from the id, so ciphertexts survive restarts and resets.
pub async fn seed(state: &Arc<AppState>, doc: &Value) -> anyhow::Result<()> {
    let svc = KmsService::new(state.db.clone());
    for entry in doc
        .get("keys")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let mut aliases: Vec<String> = Vec::new();
        if let Some(a) = entry.get("alias").and_then(Value::as_str) {
            aliases.push(a.to_string());
        }
        for a in entry
            .get("aliases")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(a) = a.as_str() {
                aliases.push(a.to_string());
            }
        }
        for a in &aliases {
            service::validate_alias_name(a).with_context(|| format!("kms seed alias {a}"))?;
        }
        let description = entry
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let id = match entry.get("id").and_then(Value::as_str) {
            Some(id) => {
                let parsed = uuid::Uuid::parse_str(id)
                    .with_context(|| format!("kms seed id {id} is not a UUID"))?;
                Some(parsed.hyphenated().to_string())
            }
            None => None,
        };

        let existing = match &id {
            Some(id) => svc.load_key(id).await?.map(|k| k.id),
            None => {
                let mut found = None;
                for a in &aliases {
                    if let Some(alias) = svc.get_alias(a).await? {
                        found = Some(alias.target_key_id);
                        break;
                    }
                }
                if found.is_none() && aliases.is_empty() {
                    for kid in svc.list_key_ids().await? {
                        if let Some(k) = svc.load_key(&kid).await? {
                            if k.description == description {
                                found = Some(k.id);
                                break;
                            }
                        }
                    }
                }
                found
            }
        };

        let key_id = match existing {
            Some(k) => k,
            None => {
                let tags: Vec<(String, String)> = entry
                    .get("tags")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flatten()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            v.as_str()
                                .map(String::from)
                                .unwrap_or_else(|| v.to_string()),
                        )
                    })
                    .collect();
                let key = svc
                    .create_key(CreateKeyInput {
                        material: id.as_deref().map(seeded_material),
                        id,
                        description: Some(description),
                        tags,
                        ..Default::default()
                    })
                    .await?;
                if !entry
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
                {
                    svc.set_enabled(&key.id, false).await?;
                }
                key.id
            }
        };

        for a in &aliases {
            if svc.get_alias(a).await?.is_none() {
                if let Err(e) = svc.create_alias(a, &key_id).await {
                    match e.code() {
                        "AlreadyExistsException" => {}
                        "KMSInvalidStateException" => tracing::warn!("kms seed alias {a}: {e}"),
                        _ => bail!("kms seed alias {a}: {e}"),
                    }
                }
            }
        }
    }
    Ok(())
}

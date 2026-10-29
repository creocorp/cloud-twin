//! KMS AWS JSON 1.1 protocol adapter (`X-Amz-Target: TrentService.<Op>`).

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::{http::StatusCode, response::Response};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};

use super::models::key_arn;
use super::service::{
    require_default_policy_name, CreateKeyInput, KmsError, KmsResult, KmsService,
};
use crate::proto::{json_error_response, json_ok};
use crate::AppState;

const MUTATING: &[&str] = &[
    "CreateKey",
    "CreateAlias",
    "UpdateAlias",
    "DeleteAlias",
    "EnableKey",
    "DisableKey",
    "ScheduleKeyDeletion",
    "CancelKeyDeletion",
    "EnableKeyRotation",
    "DisableKeyRotation",
    "TagResource",
    "UntagResource",
    "UpdateKeyDescription",
    "PutKeyPolicy",
];

pub async fn handle_json(state: &Arc<AppState>, target: &str, body: Value) -> Response {
    let op = target.strip_prefix("TrentService.").unwrap_or(target);
    let svc = KmsService::new(state.db.clone());
    let result = match svc.purge_expired().await {
        Ok(()) => dispatch(&svc, op, &body).await,
        Err(e) => Err(e),
    };
    match result {
        Ok(Some(resp)) => {
            if MUTATING.contains(&op) {
                let payload = telemetry_payload(op, &body, &resp);
                crate::telemetry::emit(&state.db, "aws", "kms", &to_snake(op), &payload).await;
            }
            json_ok(resp)
        }
        Ok(None) => json_error_response(
            StatusCode::BAD_REQUEST,
            "UnsupportedOperationException",
            &format!("Unsupported KMS operation: {op}"),
        ),
        Err(e) => {
            let status = if matches!(e, KmsError::Internal(_)) {
                StatusCode::INTERNAL_SERVER_ERROR
            } else {
                StatusCode::BAD_REQUEST
            };
            json_error_response(status, e.code(), e.message())
        }
    }
}

async fn dispatch(svc: &KmsService, op: &str, b: &Value) -> KmsResult<Option<Value>> {
    let resp = match op {
        "CreateKey" => {
            let spec = opt_str(b, "KeySpec").or_else(|| opt_str(b, "CustomerMasterKeySpec"));
            let key = svc
                .create_key(CreateKeyInput {
                    description: opt_str(b, "Description").map(String::from),
                    key_usage: opt_str(b, "KeyUsage").map(String::from),
                    key_spec: spec.map(String::from),
                    origin: opt_str(b, "Origin").map(String::from),
                    policy: opt_str(b, "Policy").map(String::from),
                    tags: tags(b, "Tags")?,
                    multi_region: b
                        .get("MultiRegion")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    ..Default::default()
                })
                .await?;
            json!({"KeyMetadata": key.metadata()})
        }
        "DescribeKey" => {
            let key = svc.resolve(req_str(b, "KeyId")?).await?;
            json!({"KeyMetadata": key.metadata()})
        }
        "ListKeys" => {
            let ids = svc.list_key_ids().await?;
            let (page, next) = paginate(ids, b, 1000, 100)?;
            let keys: Vec<Value> = page
                .iter()
                .map(|id| json!({"KeyId": id, "KeyArn": key_arn(id)}))
                .collect();
            paged(json!({"Keys": keys}), next)
        }
        "EnableKey" | "DisableKey" => {
            svc.set_enabled(req_str(b, "KeyId")?, op == "EnableKey")
                .await?;
            json!({})
        }
        "ScheduleKeyDeletion" => {
            let key = svc
                .schedule_deletion(req_str(b, "KeyId")?, opt_i64(b, "PendingWindowInDays")?)
                .await?;
            json!({
                "KeyId": key.arn(),
                "DeletionDate": key.deletion_date,
                "KeyState": key.key_state,
                "PendingWindowInDays": key.pending_window_days,
            })
        }
        "CancelKeyDeletion" => {
            let key = svc.cancel_deletion(req_str(b, "KeyId")?).await?;
            json!({"KeyId": key.arn()})
        }
        "EnableKeyRotation" => {
            svc.set_rotation(
                req_str(b, "KeyId")?,
                true,
                opt_i64(b, "RotationPeriodInDays")?,
            )
            .await?;
            json!({})
        }
        "DisableKeyRotation" => {
            svc.set_rotation(req_str(b, "KeyId")?, false, None).await?;
            json!({})
        }
        "GetKeyRotationStatus" => {
            let key = svc.resolve(req_str(b, "KeyId")?).await?;
            let mut v = json!({
                "KeyRotationEnabled": key.rotation_enabled,
                "KeyId": key.arn(),
            });
            if key.rotation_enabled {
                let next = key.created_at + (key.rotation_period_days as f64) * 86400.0;
                v["RotationPeriodInDays"] = json!(key.rotation_period_days);
                v["NextRotationDate"] = json!(next);
            }
            v
        }
        "UpdateKeyDescription" => {
            let desc = opt_str(b, "Description")
                .ok_or_else(|| missing("Description"))?
                .to_string();
            svc.update_description(req_str(b, "KeyId")?, desc).await?;
            json!({})
        }
        "TagResource" => {
            svc.tag(req_str(b, "KeyId")?, tags(b, "Tags")?).await?;
            json!({})
        }
        "UntagResource" => {
            let keys: Vec<String> = b
                .get("TagKeys")
                .and_then(Value::as_array)
                .ok_or_else(|| missing("TagKeys"))?
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            svc.untag(req_str(b, "KeyId")?, keys).await?;
            json!({})
        }
        "ListResourceTags" => {
            let key = svc.resolve(req_str(b, "KeyId")?).await?;
            let tags: Vec<Value> = key
                .tags
                .iter()
                .map(|(k, v)| json!({"TagKey": k, "TagValue": v}))
                .collect();
            let (page, next) = paginate(tags, b, 50, 50)?;
            paged(json!({"Tags": page}), next)
        }
        "GetKeyPolicy" => {
            let name = opt_str(b, "PolicyName").unwrap_or("default");
            require_default_policy_name(name)?;
            let key = svc.resolve(req_str(b, "KeyId")?).await?;
            json!({"Policy": key.policy, "PolicyName": "default"})
        }
        "PutKeyPolicy" => {
            let policy = req_str(b, "Policy")?.to_string();
            let name = opt_str(b, "PolicyName").unwrap_or("default");
            svc.put_policy(req_str(b, "KeyId")?, name, policy).await?;
            json!({})
        }
        "ListKeyPolicies" => {
            svc.resolve(req_str(b, "KeyId")?).await?;
            json!({"PolicyNames": ["default"], "Truncated": false})
        }
        "CreateAlias" => {
            svc.create_alias(req_str(b, "AliasName")?, req_str(b, "TargetKeyId")?)
                .await?;
            json!({})
        }
        "UpdateAlias" => {
            svc.update_alias(req_str(b, "AliasName")?, req_str(b, "TargetKeyId")?)
                .await?;
            json!({})
        }
        "DeleteAlias" => {
            svc.delete_alias(req_str(b, "AliasName")?).await?;
            json!({})
        }
        "ListAliases" => {
            let filter = match opt_str(b, "KeyId") {
                Some(r) => Some(svc.resolve(r).await?.id),
                None => None,
            };
            let aliases: Vec<Value> = svc
                .list_aliases(filter)
                .await?
                .iter()
                .map(|a| a.to_json())
                .collect();
            let (page, next) = paginate(aliases, b, 100, 50)?;
            paged(json!({"Aliases": page}), next)
        }
        "Encrypt" => {
            check_algorithm(b, "EncryptionAlgorithm")?;
            let plaintext = req_blob(b, "Plaintext")?;
            let ctx = context(b, "EncryptionContext")?;
            let (key, blob) = svc.encrypt(req_str(b, "KeyId")?, &plaintext, &ctx).await?;
            json!({
                "CiphertextBlob": B64.encode(blob),
                "KeyId": key.arn(),
                "EncryptionAlgorithm": "SYMMETRIC_DEFAULT",
            })
        }
        "Decrypt" => {
            check_algorithm(b, "EncryptionAlgorithm")?;
            let blob = ciphertext(b, "CiphertextBlob")?;
            let ctx = context(b, "EncryptionContext")?;
            let (key, plaintext) = svc.decrypt(&blob, &ctx, opt_str(b, "KeyId")).await?;
            json!({
                "KeyId": key.arn(),
                "Plaintext": B64.encode(plaintext),
                "EncryptionAlgorithm": "SYMMETRIC_DEFAULT",
            })
        }
        "ReEncrypt" => {
            check_algorithm(b, "SourceEncryptionAlgorithm")?;
            check_algorithm(b, "DestinationEncryptionAlgorithm")?;
            let blob = ciphertext(b, "CiphertextBlob")?;
            let (source, dest, out) = svc
                .re_encrypt(
                    &blob,
                    &context(b, "SourceEncryptionContext")?,
                    opt_str(b, "SourceKeyId"),
                    req_str(b, "DestinationKeyId")?,
                    &context(b, "DestinationEncryptionContext")?,
                )
                .await?;
            json!({
                "CiphertextBlob": B64.encode(out),
                "SourceKeyId": source.arn(),
                "KeyId": dest.arn(),
                "SourceEncryptionAlgorithm": "SYMMETRIC_DEFAULT",
                "DestinationEncryptionAlgorithm": "SYMMETRIC_DEFAULT",
            })
        }
        "GenerateDataKey" | "GenerateDataKeyWithoutPlaintext" => {
            let n = data_key_len(b)?;
            let ctx = context(b, "EncryptionContext")?;
            let (key, plaintext, blob) =
                svc.generate_data_key(req_str(b, "KeyId")?, n, &ctx).await?;
            let mut v = json!({"CiphertextBlob": B64.encode(blob), "KeyId": key.arn()});
            if op == "GenerateDataKey" {
                v["Plaintext"] = json!(B64.encode(plaintext));
            }
            v
        }
        "GenerateRandom" => {
            let n = opt_i64(b, "NumberOfBytes")?.ok_or_else(|| missing("NumberOfBytes"))?;
            if !(1..=1024).contains(&n) {
                return Err(KmsError::Validation(format!(
                    "1 validation error detected: Value '{n}' at 'numberOfBytes' failed to satisfy \
                     constraint: Member must have value between 1 and 1024"
                )));
            }
            json!({"Plaintext": B64.encode(super::service::random_bytes(n as usize))})
        }
        _ => return Ok(None),
    };
    Ok(Some(resp))
}

// ── Request parsing helpers ──────────────────────────────────────────────────

fn missing(field: &str) -> KmsError {
    KmsError::Validation(format!(
        "1 validation error detected: Value null at '{}' failed to satisfy constraint: Member must not be null",
        lower_first(field)
    ))
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn opt_str<'a>(b: &'a Value, field: &str) -> Option<&'a str> {
    b.get(field).and_then(Value::as_str)
}

fn req_str<'a>(b: &'a Value, field: &str) -> KmsResult<&'a str> {
    match opt_str(b, field) {
        Some(s) if !s.is_empty() => Ok(s),
        _ => Err(missing(field)),
    }
}

fn opt_i64(b: &Value, field: &str) -> KmsResult<Option<i64>> {
    match b.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_i64()
            .map(Some)
            .ok_or_else(|| KmsError::Validation(format!("{field} must be an integer"))),
    }
}

fn req_blob(b: &Value, field: &str) -> KmsResult<Vec<u8>> {
    let s = opt_str(b, field).ok_or_else(|| missing(field))?;
    B64.decode(s)
        .map_err(|_| KmsError::Validation(format!("{field} is not valid base64")))
}

fn ciphertext(b: &Value, field: &str) -> KmsResult<Vec<u8>> {
    let s = opt_str(b, field).ok_or_else(|| missing(field))?;
    B64.decode(s)
        .map_err(|_| KmsError::InvalidCiphertext(String::new()))
}

fn context(b: &Value, field: &str) -> KmsResult<BTreeMap<String, String>> {
    match b.get(field) {
        None | Some(Value::Null) => Ok(BTreeMap::new()),
        Some(Value::Object(m)) => m
            .iter()
            .map(|(k, v)| {
                v.as_str()
                    .map(|s| (k.clone(), s.to_string()))
                    .ok_or_else(|| KmsError::Validation(format!("{field} values must be strings")))
            })
            .collect(),
        Some(_) => Err(KmsError::Validation(format!(
            "{field} must be a map of strings"
        ))),
    }
}

fn tags(b: &Value, field: &str) -> KmsResult<Vec<(String, String)>> {
    match b.get(field) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|t| {
                let k = t
                    .get("TagKey")
                    .and_then(Value::as_str)
                    .ok_or_else(|| missing("TagKey"))?;
                let v = t.get("TagValue").and_then(Value::as_str).unwrap_or("");
                Ok((k.to_string(), v.to_string()))
            })
            .collect(),
        Some(_) => Err(KmsError::Validation(format!("{field} must be a list"))),
    }
}

fn check_algorithm(b: &Value, field: &str) -> KmsResult<()> {
    match opt_str(b, field) {
        None | Some("SYMMETRIC_DEFAULT") => Ok(()),
        Some(other) => Err(KmsError::InvalidKeyUsage(format!(
            "Encryption algorithm {other} is not supported by symmetric KMS keys."
        ))),
    }
}

fn data_key_len(b: &Value) -> KmsResult<usize> {
    let spec = opt_str(b, "KeySpec");
    let nbytes = opt_i64(b, "NumberOfBytes")?;
    match (spec, nbytes) {
        (Some(_), Some(_)) => Err(KmsError::Validation(
            "Please specify either number of bytes or key spec, but not both.".into(),
        )),
        (None, None) => Err(KmsError::Validation(
            "Please specify either number of bytes or key spec.".into(),
        )),
        (Some("AES_256"), None) => Ok(32),
        (Some("AES_128"), None) => Ok(16),
        (Some(other), None) => Err(KmsError::Validation(format!(
            "1 validation error detected: Value '{other}' at 'keySpec' failed to satisfy constraint: \
             Member must satisfy enum value set: [AES_256, AES_128]"
        ))),
        (None, Some(n)) if (1..=1024).contains(&n) => Ok(n as usize),
        (None, Some(n)) => Err(KmsError::Validation(format!(
            "1 validation error detected: Value '{n}' at 'numberOfBytes' failed to satisfy constraint: \
             Member must have value between 1 and 1024"
        ))),
    }
}

/// Offset-based paging with an opaque decimal `Marker`.
fn paginate<T>(
    items: Vec<T>,
    b: &Value,
    max: i64,
    default: i64,
) -> KmsResult<(Vec<T>, Option<String>)> {
    let limit = opt_i64(b, "Limit")?.unwrap_or(default);
    if !(1..=max).contains(&limit) {
        return Err(KmsError::Validation(format!(
            "1 validation error detected: Value '{limit}' at 'limit' failed to satisfy constraint: \
             Member must have value between 1 and {max}"
        )));
    }
    let start = match opt_str(b, "Marker") {
        None => 0,
        Some(m) => m
            .parse::<usize>()
            .ok()
            .filter(|s| *s <= items.len())
            .ok_or_else(|| KmsError::InvalidMarker(format!("Invalid marker: {m}")))?,
    };
    let end = (start + limit as usize).min(items.len());
    let next = (end < items.len()).then(|| end.to_string());
    let page = items.into_iter().skip(start).take(end - start).collect();
    Ok((page, next))
}

fn paged(mut v: Value, next: Option<String>) -> Value {
    v["Truncated"] = json!(next.is_some());
    if let Some(n) = next {
        v["NextMarker"] = json!(n);
    }
    v
}

fn to_snake(op: &str) -> String {
    let mut out = String::new();
    for (i, c) in op.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn telemetry_payload(op: &str, req: &Value, resp: &Value) -> String {
    let key_id = resp
        .pointer("/KeyMetadata/KeyId")
        .or_else(|| req.get("KeyId"))
        .or_else(|| req.get("TargetKeyId"))
        .cloned()
        .unwrap_or(Value::Null);
    let mut v = json!({"key_id": key_id});
    if let Some(alias) = req.get("AliasName") {
        v["alias"] = alias.clone();
    }
    if op == "ScheduleKeyDeletion" {
        v["deletion_date"] = resp.get("DeletionDate").cloned().unwrap_or(Value::Null);
    }
    v.to_string()
}

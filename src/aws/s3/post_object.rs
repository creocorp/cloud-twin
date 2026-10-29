//! Browser-based uploads: `POST /:bucket` with `multipart/form-data` and an
//! optional base64 POST policy.

use std::sync::Arc;

use axum::{
    extract::{FromRequest, Multipart, Request},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use super::errors::{S3Error, S3Result};
use super::handlers::{emit_json, empty, object_attrs, with_headers};
use super::object::{fire_created, write_headers};
use super::service::S3Service;
use super::xmlutil::{parse_tagging, tag, url_encode, XML_DECL};
use crate::AppState;

struct Form {
    /// Lowercase field names.
    fields: Vec<(String, String)>,
    filename: String,
    content: Vec<u8>,
}

impl Form {
    fn get(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

fn malformed(msg: impl Into<String>) -> S3Error {
    S3Error::new("MalformedPOSTRequest", msg)
}

async fn read_form(req: Request) -> S3Result<Form> {
    let mut mp = Multipart::from_request(req, &()).await.map_err(|e| {
        malformed(format!(
            "The body of your POST request is not well-formed multipart/form-data: {e}"
        ))
    })?;
    let mut fields = Vec::new();
    loop {
        let field = mp.next_field().await.map_err(|e| {
            malformed(format!(
                "The body of your POST request is not well-formed multipart/form-data: {e}"
            ))
        })?;
        let Some(field) = field else { break };
        let name = field.name().unwrap_or("").to_ascii_lowercase();
        if name == "file" {
            let filename = field.file_name().unwrap_or("").to_string();
            let content = field
                .bytes()
                .await
                .map_err(|e| malformed(e.to_string()))?
                .to_vec();
            // Fields after the file are ignored, like S3.
            return Ok(Form {
                fields,
                filename,
                content,
            });
        }
        let value = field.text().await.map_err(|e| malformed(e.to_string()))?;
        fields.push((name, value));
    }
    Err(
        S3Error::invalid_argument("POST requires exactly one file upload per request.")
            .with("ArgumentName", "file")
            .with("ArgumentValue", "0"),
    )
}

fn denied(cond: &Value) -> S3Error {
    S3Error::new(
        "AccessDenied",
        format!("Invalid according to Policy: Policy Condition failed: {cond}"),
    )
}

/// Enforce expiration, `eq` / `starts-with` and `content-length-range`.
fn check_policy(policy_b64: &str, form: &Form, bucket: &str, key: &str) -> S3Result<()> {
    let invalid = |m: &str| S3Error::new("InvalidPolicyDocument", format!("Invalid Policy: {m}"));
    let raw = base64::engine::general_purpose::STANDARD
        .decode(policy_b64.trim())
        .map_err(|_| invalid("Invalid Base64 Encoding."))?;
    let policy: Value = serde_json::from_slice(&raw).map_err(|_| invalid("Invalid JSON."))?;
    if let Some(exp) = policy.get("expiration").and_then(|e| e.as_str()) {
        let exp: DateTime<Utc> = DateTime::parse_from_rfc3339(exp)
            .map_err(|_| invalid("Invalid 'expiration' value."))?
            .with_timezone(&Utc);
        if Utc::now() > exp {
            return Err(S3Error::new(
                "AccessDenied",
                "Invalid according to Policy: Policy expired.",
            ));
        }
    }
    let value_of = |field: &str| -> String {
        let f = field.trim_start_matches('$').to_ascii_lowercase();
        match f.as_str() {
            "bucket" => bucket.to_string(),
            "key" => key.to_string(),
            _ => form.get(&f).unwrap_or("").to_string(),
        }
    };
    let size = form.content.len() as i64;
    for cond in policy
        .get("conditions")
        .and_then(|c| c.as_array())
        .into_iter()
        .flatten()
    {
        match cond {
            Value::Object(m) => {
                for (k, v) in m {
                    if value_of(k) != v.as_str().unwrap_or_default() {
                        return Err(denied(cond));
                    }
                }
            }
            Value::Array(a) if !a.is_empty() => {
                let op = a[0].as_str().unwrap_or("").to_ascii_lowercase();
                match op.as_str() {
                    "content-length-range" => {
                        let min = a.get(1).and_then(|v| v.as_i64()).unwrap_or(0);
                        let max = a.get(2).and_then(|v| v.as_i64()).unwrap_or(i64::MAX);
                        if size > max {
                            return Err(S3Error::new(
                                "EntityTooLarge",
                                "Your proposed upload exceeds the maximum allowed size",
                            )
                            .with("ProposedSize", size.to_string())
                            .with("MaxSizeAllowed", max.to_string()));
                        }
                        if size < min {
                            return Err(S3Error::new(
                                "EntityTooSmall",
                                "Your proposed upload is smaller than the minimum allowed size",
                            )
                            .with("ProposedSize", size.to_string())
                            .with("MinSizeAllowed", min.to_string()));
                        }
                    }
                    "eq" | "starts-with" => {
                        let field = a.get(1).and_then(|v| v.as_str()).unwrap_or("");
                        let expected = a.get(2).and_then(|v| v.as_str()).unwrap_or("");
                        let actual = value_of(field);
                        let ok = if op == "eq" {
                            actual == expected
                        } else if field.eq_ignore_ascii_case("$content-type") {
                            actual.split(',').all(|t| t.trim().starts_with(expected))
                        } else {
                            actual.starts_with(expected)
                        };
                        if !ok {
                            return Err(denied(cond));
                        }
                    }
                    _ => return Err(invalid(&format!("Invalid condition operator {op}"))),
                }
            }
            _ => return Err(invalid("Invalid condition")),
        }
    }
    Ok(())
}

pub async fn handle(
    state: &Arc<AppState>,
    bucket: &str,
    req: Request,
    request_id: &str,
) -> S3Result<Response> {
    let form = read_form(req).await?;
    let key = form
        .get("key")
        .ok_or_else(|| {
            S3Error::invalid_argument("Bucket POST must contain a field named 'key'.  If it is specified, please check the order of the fields.")
                .with("ArgumentName", "key")
                .with("ArgumentValue", "")
        })?
        .replace("${filename}", &form.filename);
    let svc = S3Service::new(state.db.clone());
    if !svc.bucket_exists(bucket).await? {
        return Err(S3Error::no_such_bucket(bucket));
    }
    if let Some(policy) = form.get("policy") {
        check_policy(policy, &form, bucket, &key)?;
    }
    let attrs = object_attrs(&form.fields)?;
    let tags = match form.get("tagging") {
        Some(t) => {
            let tags = parse_tagging(t.as_bytes())?;
            super::xmlutil::validate_tags(&tags, 10)?;
            tags
        }
        None => Vec::new(),
    };
    let wr = svc
        .put_object(
            bucket,
            &key,
            form.content.clone(),
            attrs
                .content_type
                .as_deref()
                .unwrap_or("binary/octet-stream"),
            attrs.meta,
            tags,
        )
        .await?;
    emit_json(
        state,
        "post_object",
        json!({"bucket": bucket, "key": key, "size": wr.size}),
    )
    .await;
    fire_created(state, bucket, &key, "ObjectCreated:Post", &wr, request_id).await;

    let location = format!(
        "{}/{}/{}",
        state.cfg.base_url.trim_end_matches('/'),
        bucket,
        url_encode(&key)
    );
    let mut h = write_headers(&wr);
    h.push(("location".into(), location.clone()));
    let redirect = form
        .get("success_action_redirect")
        .or_else(|| form.get("redirect"))
        .filter(|r| !r.is_empty());
    if let Some(url) = redirect {
        let sep = if url.contains('?') { '&' } else { '?' };
        let target = format!(
            "{url}{sep}bucket={}&key={}&etag={}",
            url_encode(bucket),
            url_encode(&key),
            url_encode(&wr.etag)
        );
        h.retain(|(k, _)| k != "location");
        h.push(("location".into(), target));
        return Ok(with_headers(empty(StatusCode::SEE_OTHER), &h));
    }
    let resp = match form.get("success_action_status") {
        Some("200") => empty(StatusCode::OK),
        Some("201") => (
            StatusCode::CREATED,
            [(header::CONTENT_TYPE, "application/xml")],
            format!(
                "{XML_DECL}<PostResponse>{}{}{}{}</PostResponse>",
                tag("Location", &location),
                tag("Bucket", bucket),
                tag("Key", &key),
                tag("ETag", &wr.etag)
            ),
        )
            .into_response(),
        _ => empty(StatusCode::NO_CONTENT),
    };
    Ok(with_headers(resp, &h))
}

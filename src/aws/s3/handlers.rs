//! S3 REST protocol layer: routing, sub-resource dispatch and shared helpers.
//!
//! S3 multiplexes many operations onto `/:bucket` and `/:bucket/*key`; the
//! operation is selected by HTTP method plus query-string sub-resources
//! (`?versioning`, `?uploads`, `?uploadId=`, ...).

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, Request, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, get},
    Router,
};

use super::cors;
use super::errors::{S3Error, S3Result};
use super::models::{ObjMeta, Tags};
use super::notify::host_id;
use super::service::S3Service;
use super::xmlutil::{esc, iso, tag, XMLNS, XML_DECL};
use super::{bucket, object, post_object};
use crate::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_buckets))
        .route("/:bucket", any(bucket_entry))
        .route("/:bucket/", any(bucket_entry))
        .route("/:bucket/*key", any(object_entry))
}

/// Query parameters that carry presigned-URL auth and never select an operation.
const AUTH_PARAMS: &[&str] = &[
    "x-amz-algorithm",
    "x-amz-credential",
    "x-amz-signature",
    "x-amz-date",
    "x-amz-expires",
    "x-amz-signedheaders",
    "x-amz-security-token",
    "x-amz-user-agent",
    "x-id",
    "awsaccesskeyid",
    "signature",
    "expires",
];

pub fn parse_query(raw: Option<&str>) -> Vec<(String, String)> {
    let raw = raw.unwrap_or("");
    let pairs: Vec<(String, String)> = raw
        .split('&')
        .filter(|s| !s.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            let dec = |s: &str| {
                percent_encoding::percent_decode_str(&s.replace('+', " "))
                    .decode_utf8_lossy()
                    .to_string()
            };
            (dec(k), dec(v))
        })
        .collect();
    pairs
        .into_iter()
        .filter(|(k, _)| !AUTH_PARAMS.contains(&k.to_ascii_lowercase().as_str()))
        .collect()
}

/// A decoded S3 request.
pub struct S3Req {
    pub method: Method,
    pub bucket: String,
    pub key: Option<String>,
    pub query: Vec<(String, String)>,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub request_id: String,
}

impl S3Req {
    pub fn q(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn has(&self, name: &str) -> bool {
        self.query.iter().any(|(k, _)| k == name)
    }

    pub fn h(&self, name: &str) -> Option<String> {
        header_str(&self.headers, name)
    }

    pub fn key(&self) -> &str {
        self.key.as_deref().unwrap_or("")
    }

    pub fn svc(&self, state: &Arc<AppState>) -> S3Service {
        S3Service::new(state.db.clone())
    }
}

pub fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .map(|v| String::from_utf8_lossy(v.as_bytes()).to_string())
}

/// All headers as lowercase (name, value) pairs.
pub fn header_pairs(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_ascii_lowercase(),
                String::from_utf8_lossy(v.as_bytes()).to_string(),
            )
        })
        .collect()
}

fn new_request_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..16].to_ascii_uppercase()
}

// ─────────────────────────────────────────────────────────────────────────────
// Response helpers
// ─────────────────────────────────────────────────────────────────────────────

pub fn xml_ok(body: String) -> Response {
    xml_status(StatusCode::OK, body)
}

pub fn xml_status(status: StatusCode, body: String) -> Response {
    (status, [(header::CONTENT_TYPE, "application/xml")], body).into_response()
}

pub fn empty(status: StatusCode) -> Response {
    status.into_response()
}

/// Append headers, skipping invalid names/values.
pub fn add_headers(resp: &mut Response, pairs: &[(String, String)]) {
    for (k, v) in pairs {
        if let (Ok(n), Ok(val)) = (
            HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            resp.headers_mut().insert(n, val);
        }
    }
}

pub fn with_headers(mut resp: Response, pairs: &[(String, String)]) -> Response {
    add_headers(&mut resp, pairs);
    resp
}

fn error_response(err: &S3Error, method: &Method, request_id: &str, resource: &str) -> Response {
    let status = StatusCode::from_u16(err.status()).unwrap_or(StatusCode::BAD_REQUEST);
    let mut resp = if *method == Method::HEAD {
        status.into_response()
    } else {
        let mut extra = String::new();
        for (k, v) in &err.fields {
            extra.push_str(&tag(k, v));
        }
        if !err.fields.iter().any(|(k, _)| *k == "Resource") {
            extra.push_str(&tag("Resource", resource));
        }
        xml_status(
            status,
            format!(
                "{XML_DECL}<Error><Code>{}</Code><Message>{}</Message>{extra}<RequestId>{request_id}</RequestId><HostId>{}</HostId></Error>",
                esc(err.code),
                esc(&err.message),
                host_id(request_id)
            ),
        )
    };
    add_headers(&mut resp, &err.headers);
    resp
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry points
// ─────────────────────────────────────────────────────────────────────────────

async fn list_buckets(State(state): State<Arc<AppState>>) -> Response {
    let request_id = new_request_id();
    let result = S3Service::new(state.db.clone()).list_buckets().await.map(|buckets| {
        let items: String = buckets
            .iter()
            .map(|b| {
                format!(
                    "<Bucket><Name>{}</Name><CreationDate>{}</CreationDate><BucketRegion>{}</BucketRegion></Bucket>",
                    esc(&b.name),
                    iso(&b.created_at),
                    crate::aws::REGION
                )
            })
            .collect();
        xml_ok(format!(
            "{XML_DECL}<ListAllMyBucketsResult xmlns=\"{XMLNS}\"><Owner><ID>cloudtwin</ID><DisplayName>cloudtwin</DisplayName></Owner><Buckets>{items}</Buckets></ListAllMyBucketsResult>"
        ))
    });
    finish(&state, "", &Method::GET, None, &request_id, "/", result).await
}

async fn bucket_entry(
    State(state): State<Arc<AppState>>,
    Path(bucket): Path<String>,
    req: Request,
) -> Response {
    entry(state, bucket, None, req).await
}

async fn object_entry(
    State(state): State<Arc<AppState>>,
    Path((bucket, key)): Path<(String, String)>,
    req: Request,
) -> Response {
    entry(state, bucket, Some(key), req).await
}

async fn entry(
    state: Arc<AppState>,
    bucket: String,
    key: Option<String>,
    req: Request,
) -> Response {
    let request_id = new_request_id();
    let method = req.method().clone();
    let origin = header_str(req.headers(), "origin");
    let resource = match &key {
        Some(k) => format!("/{bucket}/{k}"),
        None => format!("/{bucket}"),
    };
    let is_form = key.is_none()
        && method == Method::POST
        && header_str(req.headers(), "content-type")
            .is_some_and(|c| c.to_ascii_lowercase().starts_with("multipart/form-data"));

    let result = if method == Method::OPTIONS {
        preflight(&state, &bucket, req.headers()).await
    } else if is_form {
        post_object::handle(&state, &bucket, req, &request_id).await
    } else {
        let (parts, body) = req.into_parts();
        match axum::body::to_bytes(body, usize::MAX).await {
            Err(e) => Err(S3Error::new("IncompleteBody", e.to_string())),
            Ok(body) => {
                let r = S3Req {
                    query: parse_query(parts.uri.query()),
                    method: parts.method,
                    bucket: bucket.clone(),
                    key,
                    headers: parts.headers,
                    body,
                    request_id: request_id.clone(),
                };
                if r.key.is_some() {
                    object::dispatch(&state, &r).await
                } else {
                    bucket::dispatch(&state, &r).await
                }
            }
        }
    };
    finish(
        &state,
        &bucket,
        &method,
        origin,
        &request_id,
        &resource,
        result,
    )
    .await
}

async fn finish(
    state: &Arc<AppState>,
    bucket: &str,
    method: &Method,
    origin: Option<String>,
    request_id: &str,
    resource: &str,
    result: S3Result<Response>,
) -> Response {
    let mut resp = match result {
        Ok(r) => r,
        Err(e) => {
            if e.code == "InternalError" {
                tracing::warn!("S3 internal error on {method} {resource}: {}", e.message);
            }
            error_response(&e, method, request_id, resource)
        }
    };
    let h = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(request_id) {
        h.insert("x-amz-request-id", v);
    }
    if let Ok(v) = HeaderValue::from_str(&host_id(request_id)) {
        h.insert("x-amz-id-2", v);
    }
    if let Some(origin) = origin {
        if *method != Method::OPTIONS && !bucket.is_empty() {
            if let Ok(Some(xml)) = S3Service::new(state.db.clone())
                .get_config(bucket, "cors")
                .await
            {
                if let Ok(rules) = cors::parse_rules(xml.as_bytes()) {
                    if let Some(rule) = cors::find_rule(&rules, &origin, method.as_str(), &[]) {
                        cors::apply(resp.headers_mut(), rule, &origin, false, &[]);
                    }
                }
            }
        }
    }
    resp
}

/// CORS preflight (`OPTIONS /:bucket[/key]`).
async fn preflight(state: &Arc<AppState>, bucket: &str, headers: &HeaderMap) -> S3Result<Response> {
    let origin = header_str(headers, "origin");
    let req_method = header_str(headers, "access-control-request-method");
    let (Some(origin), Some(req_method)) = (origin, req_method) else {
        return Err(S3Error::new(
            "BadRequest",
            "Insufficient information. Origin request header needed.",
        ));
    };
    let req_headers = header_str(headers, "access-control-request-headers")
        .map(|v| cors::split_header_list(&v))
        .unwrap_or_default();
    let xml = S3Service::new(state.db.clone())
        .get_config(bucket, "cors")
        .await
        .ok()
        .flatten()
        .ok_or_else(|| S3Error::new("CORSResponse", "CORS is not enabled for this bucket."))?;
    let rules = cors::parse_rules(xml.as_bytes())?;
    let rule = cors::find_rule(&rules, &origin, req_method.trim(), &req_headers).ok_or_else(|| {
        S3Error::new(
            "CORSResponse",
            "This CORS request is not allowed. This is usually because the evalution of Origin, request method / Access-Control-Request-Method or Access-Control-Request-Headers are not whitelisted by the resource's CORS spec.",
        )
        .with("Method", req_method.clone())
        .with("ResourceType", "BUCKET")
    })?;
    let mut resp = StatusCode::OK.into_response();
    cors::apply(resp.headers_mut(), rule, &origin, true, &req_headers);
    Ok(resp)
}

// ─────────────────────────────────────────────────────────────────────────────
// Object attribute parsing (shared by PutObject, CopyObject, multipart, POST)
// ─────────────────────────────────────────────────────────────────────────────

pub const STD_HEADERS: &[&str] = &[
    "cache-control",
    "content-disposition",
    "content-encoding",
    "content-language",
    "expires",
];

const SSE_HEADERS: &[&str] = &[
    "x-amz-server-side-encryption",
    "x-amz-server-side-encryption-aws-kms-key-id",
    "x-amz-server-side-encryption-bucket-key-enabled",
    "x-amz-server-side-encryption-context",
    "x-amz-server-side-encryption-customer-algorithm",
    "x-amz-server-side-encryption-customer-key-md5",
];

pub const CHECKSUM_HEADERS: &[&str] = &[
    "x-amz-checksum-crc32",
    "x-amz-checksum-crc32c",
    "x-amz-checksum-crc64nvme",
    "x-amz-checksum-sha1",
    "x-amz-checksum-sha256",
];

pub struct ObjectAttrs {
    pub content_type: Option<String>,
    pub meta: ObjMeta,
    pub tags: Option<Tags>,
}

fn pair<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

/// Parse a URL-encoded tag set (`x-amz-tagging: k1=v1&k2=v2`).
pub fn parse_tag_query(s: &str) -> S3Result<Tags> {
    let tags: Tags = serde_urlencoded::from_str(s)
        .map_err(|_| S3Error::invalid_argument("The header 'x-amz-tagging' shall be encoded as UTF-8 then URLEncoded URL query parameters without tag name duplicates."))?;
    super::xmlutil::validate_tags(&tags, 10)?;
    Ok(tags)
}

/// Extract stored object attributes from lowercase header/form pairs.
pub fn object_attrs(pairs: &[(String, String)]) -> S3Result<ObjectAttrs> {
    let mut meta = ObjMeta::default();
    for (k, v) in pairs {
        if let Some(name) = k.strip_prefix("x-amz-meta-") {
            meta.user.push((name.to_string(), v.clone()));
        }
    }
    for h in STD_HEADERS {
        if let Some(v) = pair(pairs, h) {
            let v = if *h == "content-encoding" {
                v.split(',')
                    .map(str::trim)
                    .filter(|e| !e.eq_ignore_ascii_case("aws-chunked") && !e.is_empty())
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                v.to_string()
            };
            if !v.is_empty() {
                meta.headers.push((h.to_string(), v));
            }
        }
    }
    for h in SSE_HEADERS {
        if let Some(v) = pair(pairs, h) {
            meta.sse.push((h.to_string(), v.to_string()));
        }
    }
    if let Some(algo) = pair(pairs, "x-amz-server-side-encryption") {
        if !["AES256", "aws:kms", "aws:kms:dsse"].contains(&algo) {
            return Err(S3Error::invalid_argument(format!(
                "Server Side Encryption with unsupported algorithm {algo}"
            ))
            .with("ArgumentName", "x-amz-server-side-encryption")
            .with("ArgumentValue", algo));
        }
    }
    for h in CHECKSUM_HEADERS {
        if let Some(v) = pair(pairs, h) {
            meta.checksums.push((h.to_string(), v.to_string()));
        }
    }
    meta.storage_class = pair(pairs, "x-amz-storage-class").map(String::from);
    meta.website_redirect = pair(pairs, "x-amz-website-redirect-location").map(String::from);
    let tags = match pair(pairs, "x-amz-tagging") {
        Some(t) => Some(parse_tag_query(t)?),
        None => None,
    };
    Ok(ObjectAttrs {
        content_type: pair(pairs, "content-type").map(String::from),
        meta,
        tags,
    })
}

/// SSE response headers for a stored object (customer-key objects omit the
/// plain `x-amz-server-side-encryption` header like AWS).
pub fn sse_headers(meta: &ObjMeta) -> Vec<(String, String)> {
    meta.sse
        .iter()
        .filter(|(k, _)| k != "x-amz-server-side-encryption-context")
        .cloned()
        .collect()
}

/// Validate supplied checksums (CRC32 and SHA256) against `content`.
pub fn verify_checksums(meta: &ObjMeta, content: &[u8]) -> S3Result<()> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    for (k, v) in &meta.checksums {
        let (name, expected) = match k.as_str() {
            "x-amz-checksum-crc32" => ("CRC32", b64.encode(crc32fast::hash(content).to_be_bytes())),
            "x-amz-checksum-sha256" => {
                use sha2::Digest;
                ("SHA256", b64.encode(sha2::Sha256::digest(content)))
            }
            _ => continue,
        };
        if *v != expected {
            return Err(S3Error::new(
                "BadDigest",
                format!("The {name} you specified did not match the calculated checksum."),
            ));
        }
    }
    Ok(())
}

pub fn emit_json(
    state: &Arc<AppState>,
    action: &'static str,
    payload: serde_json::Value,
) -> impl std::future::Future<Output = ()> + Send + 'static {
    let db = state.db.clone();
    async move { crate::telemetry::emit(&db, "aws", "s3", action, &payload.to_string()).await }
}

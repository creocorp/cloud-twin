//! GCP Cloud Storage HTTP handlers (Google Cloud Storage JSON API v1).
//!
//! Routes (mounted at `/gcp`; `app.rs` maps the root `/storage/v1`,
//! `/upload/storage/v1` and `/download/storage/v1` prefixes onto them):
//!
//! ```text
//! POST   /storage/v1/b                          create bucket
//! GET    /storage/v1/b                          list buckets
//! GET    /storage/v1/b/{bucket}                 get bucket
//! DELETE /storage/v1/b/{bucket}                 delete (empty) bucket
//! POST   /upload/storage/v1/b/{bucket}/o        upload (uploadType=media|multipart|resumable)
//! PUT    /upload/storage/v1/b/{bucket}/o        resumable upload chunks (?upload_id=)
//! DELETE /upload/storage/v1/b/{bucket}/o        cancel resumable upload
//! GET    /storage/v1/b/{bucket}/o               list objects (prefix, delimiter, paging)
//! GET    /storage/v1/b/{bucket}/o/{object}      object metadata (or data with alt=media)
//! PATCH  /storage/v1/b/{bucket}/o/{object}      update object metadata
//! DELETE /storage/v1/b/{bucket}/o/{object}      delete object
//! GET    /download/storage/v1/b/{bucket}/o/{object}?alt=media   download
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde_json::{json, Value};

use super::models::{rfc3339_z, GcsBucket, GcsObject, OBJECT_PROPS};
use super::service::{merge_props, GcsError, ListQuery, StorageService};
use crate::telemetry;
use crate::AppState;

type Params = HashMap<String, String>;

fn svc(state: &Arc<AppState>) -> StorageService {
    StorageService::new(state.db.clone(), state.cfg.gcp_project.clone())
}

/// Characters left unescaped in object names inside generated links.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

fn encode_name(name: &str) -> String {
    utf8_percent_encode(name, SEGMENT).to_string()
}

/// Externally visible base URL (scheme + host) the client used.
fn base_url(headers: &HeaderMap) -> String {
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost:4793");
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    format!("{proto}://{host}")
}

fn bucket_to_json(b: &GcsBucket, base: &str) -> Value {
    let ts = rfc3339_z(&b.created_at);
    json!({
        "kind":           "storage#bucket",
        "id":             b.name,
        "selfLink":       format!("{base}/storage/v1/b/{}", b.name),
        "name":           b.name,
        "projectNumber":  "000000000000",
        "metageneration": "1",
        "location":       b.location,
        "locationType":   "multi-region",
        "storageClass":   "STANDARD",
        "etag":           "CAE=",
        "timeCreated":    ts,
        "updated":        ts,
        "iamConfiguration": {
            "uniformBucketLevelAccess": { "enabled": false },
            "publicAccessPrevention":   "inherited",
        },
    })
}

fn object_to_json(o: &GcsObject, base: &str) -> Value {
    let generation = o.generation();
    let enc = encode_name(&o.name);
    let ts = rfc3339_z(&o.created_at);
    let mut v = json!({
        "kind":           "storage#object",
        "id":             format!("{}/{}/{generation}", o.bucket, o.name),
        "selfLink":       format!("{base}/storage/v1/b/{}/o/{enc}", o.bucket),
        "mediaLink":      format!("{base}/download/storage/v1/b/{}/o/{enc}?generation={generation}&alt=media", o.bucket),
        "name":           o.name,
        "bucket":         o.bucket,
        "generation":     generation.to_string(),
        "metageneration": "1",
        "contentType":    o.content_type,
        "storageClass":   "STANDARD",
        "size":           o.size.to_string(),
        "md5Hash":        o.md5_b64(),
        "crc32c":         o.crc32c_b64(),
        "etag":           o.etag,
        "timeCreated":    ts,
        "updated":        ts,
        "timeStorageClassUpdated": ts,
    });
    if let (Some(out), Some(props)) = (v.as_object_mut(), o.props.as_object()) {
        for key in OBJECT_PROPS {
            if let Some(p) = props.get(*key) {
                out.insert(key.to_string(), p.clone());
            }
        }
    }
    v
}

/// Google JSON API error envelope.
fn gcs_error(status: StatusCode, reason: &str, message: impl Into<String>) -> Response {
    let message = message.into();
    (
        status,
        Json(json!({
            "error": {
                "code": status.as_u16(),
                "message": message,
                "errors": [{ "message": message, "domain": "global", "reason": reason }],
            }
        })),
    )
        .into_response()
}

fn error_response(e: GcsError) -> Response {
    match e {
        GcsError::BucketNotFound(b) => {
            gcs_error(StatusCode::NOT_FOUND, "notFound", format!("The specified bucket does not exist: {b}"))
        }
        GcsError::ObjectNotFound(b, o) => {
            gcs_error(StatusCode::NOT_FOUND, "notFound", format!("No such object: {b}/{o}"))
        }
        GcsError::BucketExists(b) => gcs_error(
            StatusCode::CONFLICT,
            "conflict",
            format!("Your previous request to create the named bucket succeeded and you already own it: {b}"),
        ),
        GcsError::BucketNotEmpty(b) => gcs_error(
            StatusCode::CONFLICT,
            "conflict",
            format!("The bucket you tried to delete is not empty: {b}"),
        ),
        GcsError::PreconditionFailed(o) => gcs_error(
            StatusCode::PRECONDITION_FAILED,
            "conditionNotMet",
            format!("At least one of the pre-conditions you specified did not hold: {o}"),
        ),
        GcsError::Internal(m) => gcs_error(StatusCode::INTERNAL_SERVER_ERROR, "backendError", m),
    }
}

fn param_i64(params: &Params, key: &str) -> Option<i64> {
    params.get(key).and_then(|v| v.parse().ok())
}

/// Axum wildcard captures may include the leading `/`.
fn object_name(raw: String) -> String {
    raw.strip_prefix('/').map(str::to_string).unwrap_or(raw)
}

/// Router is mounted at `/gcp` in main.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // Buckets
        .route("/storage/v1/b", post(create_bucket).get(list_buckets))
        .route(
            "/storage/v1/b/:bucket",
            get(get_bucket).delete(delete_bucket),
        )
        // Uploads
        .route(
            "/upload/storage/v1/b/:bucket/o",
            post(upload_object)
                .put(resumable_put)
                .delete(resumable_cancel),
        )
        // Objects
        .route("/storage/v1/b/:bucket/o", get(list_objects))
        .route(
            "/storage/v1/b/:bucket/o/*object",
            get(get_object).patch(patch_object).delete(delete_object),
        )
        .route(
            "/download/storage/v1/b/:bucket/o/*object",
            get(download_object),
        )
}

// ── Bucket handlers ───────────────────────────────────────────────────────────

async fn create_bucket(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let body: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("");
    if name.is_empty() {
        return gcs_error(StatusCode::BAD_REQUEST, "required", "Required: bucket name");
    }
    let location = body.get("location").and_then(|v| v.as_str());
    match svc(&state).create_bucket(name, location).await {
        Ok(b) => {
            telemetry::emit(
                &state.db,
                "gcp",
                "storage",
                "create_bucket",
                &json!({"bucket": name}).to_string(),
            )
            .await;
            Json(bucket_to_json(&b, &base_url(&headers))).into_response()
        }
        Err(e) => error_response(e),
    }
}

async fn list_buckets(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let base = base_url(&headers);
    match svc(&state).list_buckets().await {
        Ok(bs) => {
            let items: Vec<_> = bs.iter().map(|b| bucket_to_json(b, &base)).collect();
            Json(json!({ "kind": "storage#buckets", "items": items })).into_response()
        }
        Err(e) => error_response(e),
    }
}

async fn get_bucket(
    State(state): State<Arc<AppState>>,
    Path(bucket): Path<String>,
    headers: HeaderMap,
) -> Response {
    match svc(&state).get_bucket(&bucket).await {
        Ok(b) => Json(bucket_to_json(&b, &base_url(&headers))).into_response(),
        Err(e) => error_response(e),
    }
}

async fn delete_bucket(State(state): State<Arc<AppState>>, Path(bucket): Path<String>) -> Response {
    match svc(&state).delete_bucket(&bucket).await {
        Ok(()) => {
            telemetry::emit(
                &state.db,
                "gcp",
                "storage",
                "delete_bucket",
                &json!({"bucket": bucket}).to_string(),
            )
            .await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => error_response(e),
    }
}

// ── Uploads ───────────────────────────────────────────────────────────────────

/// Extract the `boundary` parameter of a `multipart/related` content type.
fn multipart_boundary(content_type: &str) -> Option<String> {
    content_type.split(';').find_map(|p| {
        let p = p.trim();
        let (k, v) = p.split_once('=')?;
        k.trim()
            .eq_ignore_ascii_case("boundary")
            .then(|| v.trim().trim_matches('"').to_string())
    })
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > hay.len() || needle.is_empty() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// Parse a `multipart/related` body into `(content-type, body)` parts.
/// Part bodies are taken verbatim (binary-safe) up to the next delimiter.
fn parse_multipart(body: &[u8], boundary: &str) -> Vec<(String, Vec<u8>)> {
    let delim = format!("--{boundary}").into_bytes();
    let next_delim = [b"\r\n".as_slice(), &delim].concat();
    let mut parts = Vec::new();
    let Some(mut pos) = find(body, &delim, 0) else {
        return parts;
    };
    loop {
        pos += delim.len();
        if body[pos..].starts_with(b"--") {
            break;
        }
        // Skip transport padding up to and including the CRLF after the delimiter.
        let Some(line_end) = find(body, b"\n", pos) else {
            break;
        };
        let start = line_end + 1;
        let end = find(body, &next_delim, start).unwrap_or(body.len());
        let part = &body[start..end];
        let (head, data) = if part.starts_with(b"\r\n") {
            (&part[..0], &part[2..])
        } else if let Some(h) = find(part, b"\r\n\r\n", 0) {
            (&part[..h], &part[h + 4..])
        } else {
            (part, &part[part.len()..])
        };
        let content_type = String::from_utf8_lossy(head)
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim()
                    .eq_ignore_ascii_case("content-type")
                    .then(|| v.trim().to_string())
            })
            .unwrap_or_default();
        parts.push((content_type, data.to_vec()));
        if end >= body.len() {
            break;
        }
        pos = end + 2;
    }
    parts
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
}

/// Persist an uploaded object and return its JSON resource.
#[allow(clippy::too_many_arguments)]
async fn store_object(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    bucket: &str,
    name: &str,
    data: Vec<u8>,
    content_type: &str,
    meta: &Value,
    if_generation_match: Option<i64>,
) -> Response {
    if name.is_empty() {
        return gcs_error(StatusCode::BAD_REQUEST, "required", "Required: object name");
    }
    let mut props = json!({});
    merge_props(&mut props, meta);
    match svc(state)
        .upload_object(bucket, name, data, content_type, props, if_generation_match)
        .await
    {
        Ok(o) => {
            telemetry::emit(
                &state.db,
                "gcp",
                "storage",
                "put_object",
                &json!({"bucket": bucket, "object": name, "size": o.size}).to_string(),
            )
            .await;
            Json(object_to_json(&o, &base_url(headers))).into_response()
        }
        Err(e) => error_response(e),
    }
}

async fn upload_object(
    State(state): State<Arc<AppState>>,
    Path(bucket): Path<String>,
    Query(params): Query<Params>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let upload_type = params
        .get("uploadType")
        .map(String::as_str)
        .unwrap_or("media");
    let if_gen = param_i64(&params, "ifGenerationMatch");
    let query_name = params.get("name").cloned().unwrap_or_default();
    let header_ct = header_str(&headers, "content-type").unwrap_or("application/octet-stream");

    match upload_type {
        "media" => {
            store_object(
                &state,
                &headers,
                &bucket,
                &query_name,
                body.to_vec(),
                header_ct,
                &json!({}),
                if_gen,
            )
            .await
        }
        "multipart" => {
            let Some(boundary) = multipart_boundary(header_ct) else {
                return gcs_error(
                    StatusCode::BAD_REQUEST,
                    "invalid",
                    "Missing multipart boundary",
                );
            };
            let mut parts = parse_multipart(&body, &boundary).into_iter();
            let (Some((_, meta_raw)), Some((part_ct, data))) = (parts.next(), parts.next()) else {
                return gcs_error(
                    StatusCode::BAD_REQUEST,
                    "invalid",
                    "Multipart upload requires metadata and media parts",
                );
            };
            let meta: Value = serde_json::from_slice(&meta_raw).unwrap_or_else(|_| json!({}));
            let name = meta
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or(query_name);
            let ct = meta
                .get("contentType")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .or(Some(part_ct).filter(|c| !c.is_empty()))
                .unwrap_or_else(|| "application/octet-stream".to_string());
            store_object(&state, &headers, &bucket, &name, data, &ct, &meta, if_gen).await
        }
        "resumable" => start_resumable(&state, &bucket, &params, &headers, &body).await,
        other => gcs_error(
            StatusCode::BAD_REQUEST,
            "invalid",
            format!("Unknown uploadType: {other}"),
        ),
    }
}

/// An in-progress resumable upload session (kept in memory, like the
/// session URIs of real GCS they are short-lived).
struct ResumableSession {
    bucket: String,
    name: String,
    content_type: String,
    meta: Value,
    if_generation_match: Option<i64>,
    data: Vec<u8>,
}

fn sessions() -> &'static Mutex<HashMap<String, ResumableSession>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, ResumableSession>>> = OnceLock::new();
    SESSIONS.get_or_init(Default::default)
}

async fn start_resumable(
    state: &Arc<AppState>,
    bucket: &str,
    params: &Params,
    headers: &HeaderMap,
    body: &[u8],
) -> Response {
    let meta: Value = serde_json::from_slice(body).unwrap_or_else(|_| json!({}));
    let name = meta
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| params.get("name").cloned())
        .unwrap_or_default();
    if name.is_empty() {
        return gcs_error(StatusCode::BAD_REQUEST, "required", "Required: object name");
    }
    if let Err(e) = svc(state).get_bucket(bucket).await {
        return error_response(e);
    }
    let content_type = meta
        .get("contentType")
        .and_then(|v| v.as_str())
        .or_else(|| header_str(headers, "x-upload-content-type"))
        .unwrap_or("application/octet-stream")
        .to_string();
    let id = uuid::Uuid::new_v4().simple().to_string();
    sessions().lock().expect("sessions").insert(
        id.clone(),
        ResumableSession {
            bucket: bucket.to_string(),
            name,
            content_type,
            meta,
            if_generation_match: param_i64(params, "ifGenerationMatch"),
            data: Vec::new(),
        },
    );
    let location = format!(
        "{}/upload/storage/v1/b/{bucket}/o?uploadType=resumable&upload_id={id}",
        base_url(headers)
    );
    let mut resp = StatusCode::OK.into_response();
    if let Ok(v) = HeaderValue::from_str(&location) {
        resp.headers_mut().insert(header::LOCATION, v);
    }
    resp
}

/// Parsed `Content-Range` of a resumable upload request.
#[derive(Debug, PartialEq)]
struct ChunkRange {
    /// Offset of the first byte in this request (None for `bytes */N`).
    start: Option<usize>,
    /// Total object size, when the client knows it.
    total: Option<usize>,
}

fn parse_content_range(v: &str) -> Option<ChunkRange> {
    let rest = v.trim().strip_prefix("bytes")?.trim();
    let (range, total) = rest.split_once('/')?;
    let total = match total.trim() {
        "*" => None,
        t => Some(t.parse().ok()?),
    };
    let start = match range.trim() {
        "*" => None,
        r => Some(r.split_once('-')?.0.trim().parse().ok()?),
    };
    Some(ChunkRange { start, total })
}

enum ChunkOutcome {
    Incomplete(usize),
    Complete(ResumableSession),
}

async fn resumable_put(
    State(state): State<Arc<AppState>>,
    Query(params): Query<Params>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(id) = params.get("upload_id") else {
        return gcs_error(StatusCode::BAD_REQUEST, "required", "Required: upload_id");
    };
    let range = match header_str(&headers, "content-range") {
        None => None,
        Some(v) => match parse_content_range(v) {
            Some(r) => Some(r),
            None => return gcs_error(StatusCode::BAD_REQUEST, "invalid", "Invalid Content-Range"),
        },
    };
    let outcome = {
        let mut map = sessions().lock().expect("sessions");
        let Some(session) = map.get_mut(id) else {
            return gcs_error(
                StatusCode::NOT_FOUND,
                "notFound",
                "Upload session not found",
            );
        };
        let complete = match &range {
            // No Content-Range: the body is the whole object.
            None => {
                session.data = body.to_vec();
                true
            }
            Some(r) => {
                if let Some(start) = r.start {
                    if start > session.data.len() {
                        return gcs_error(
                            StatusCode::BAD_REQUEST,
                            "invalid",
                            "Chunk starts beyond received data",
                        );
                    }
                    session.data.truncate(start);
                    session.data.extend_from_slice(&body);
                }
                r.total.is_some_and(|t| session.data.len() >= t)
            }
        };
        if complete {
            ChunkOutcome::Complete(map.remove(id).expect("session"))
        } else {
            ChunkOutcome::Incomplete(session.data.len())
        }
    };
    match outcome {
        ChunkOutcome::Incomplete(received) => {
            let mut resp = StatusCode::PERMANENT_REDIRECT.into_response();
            if received > 0 {
                if let Ok(v) = HeaderValue::from_str(&format!("bytes=0-{}", received - 1)) {
                    resp.headers_mut().insert(header::RANGE, v);
                }
            }
            resp
        }
        ChunkOutcome::Complete(s) => {
            store_object(
                &state,
                &headers,
                &s.bucket,
                &s.name,
                s.data,
                &s.content_type,
                &s.meta,
                s.if_generation_match,
            )
            .await
        }
    }
}

async fn resumable_cancel(Query(params): Query<Params>) -> Response {
    let removed = params
        .get("upload_id")
        .and_then(|id| sessions().lock().expect("sessions").remove(id));
    match removed {
        // GCS answers a cancelled session with 499 Client Closed Request.
        Some(_) => StatusCode::from_u16(499)
            .unwrap_or(StatusCode::NO_CONTENT)
            .into_response(),
        None => gcs_error(
            StatusCode::NOT_FOUND,
            "notFound",
            "Upload session not found",
        ),
    }
}

// ── Object handlers ───────────────────────────────────────────────────────────

async fn list_objects(
    State(state): State<Arc<AppState>>,
    Path(bucket): Path<String>,
    Query(params): Query<Params>,
    headers: HeaderMap,
) -> Response {
    let q = ListQuery {
        prefix: params.get("prefix").cloned().unwrap_or_default(),
        delimiter: params.get("delimiter").cloned(),
        start_offset: params.get("startOffset").cloned(),
        end_offset: params.get("endOffset").cloned(),
        max_results: params.get("maxResults").and_then(|v| v.parse().ok()),
        page_token: params.get("pageToken").cloned(),
    };
    let base = base_url(&headers);
    match svc(&state).list_objects(&bucket, q).await {
        Ok(listing) => {
            let items: Vec<_> = listing
                .items
                .iter()
                .map(|o| object_to_json(o, &base))
                .collect();
            let mut out = json!({ "kind": "storage#objects", "items": items });
            if !listing.prefixes.is_empty() {
                out["prefixes"] = json!(listing.prefixes);
            }
            if let Some(t) = listing.next_page_token {
                out["nextPageToken"] = json!(t);
            }
            Json(out).into_response()
        }
        Err(e) => error_response(e),
    }
}

/// Fetch an object, honouring an optional `generation` selector.
async fn fetch_object(
    state: &Arc<AppState>,
    bucket: &str,
    name: &str,
    params: &Params,
) -> Result<GcsObject, Response> {
    let obj = svc(state)
        .get_object(bucket, name)
        .await
        .map_err(error_response)?;
    match param_i64(params, "generation") {
        Some(g) if g != obj.generation() => Err(error_response(GcsError::ObjectNotFound(
            bucket.to_string(),
            name.to_string(),
        ))),
        _ => Ok(obj),
    }
}

async fn get_object(
    State(state): State<Arc<AppState>>,
    Path((bucket, object)): Path<(String, String)>,
    Query(params): Query<Params>,
    headers: HeaderMap,
) -> Response {
    let name = object_name(object);
    let obj = match fetch_object(&state, &bucket, &name, &params).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    if params.get("alt").map(String::as_str) == Some("media") {
        return media_response(obj, &headers);
    }
    Json(object_to_json(&obj, &base_url(&headers))).into_response()
}

async fn download_object(
    State(state): State<Arc<AppState>>,
    Path((bucket, object)): Path<(String, String)>,
    Query(params): Query<Params>,
    headers: HeaderMap,
) -> Response {
    let name = object_name(object);
    match fetch_object(&state, &bucket, &name, &params).await {
        Ok(o) => media_response(o, &headers),
        Err(r) => r,
    }
}

/// Parse a single `Range: bytes=...` spec into an inclusive range.
/// `None` = header absent/unsupported (serve everything), `Some(Err)` = unsatisfiable.
fn parse_range(v: Option<&str>, len: usize) -> Option<Result<(usize, usize), ()>> {
    let spec = v?.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (a, b) = (a.trim(), b.trim());
    let range = if a.is_empty() {
        let n: usize = b.parse().ok()?;
        if n == 0 || len == 0 {
            return Some(Err(()));
        }
        (len.saturating_sub(n), len - 1)
    } else {
        let start: usize = a.parse().ok()?;
        let end = if b.is_empty() {
            len.saturating_sub(1)
        } else {
            b.parse::<usize>().ok()?.min(len.saturating_sub(1))
        };
        if start >= len || end < start {
            return Some(Err(()));
        }
        (start, end)
    };
    Some(Ok(range))
}

/// Build an `alt=media` response with GCS checksum/generation headers.
fn media_response(obj: GcsObject, req_headers: &HeaderMap) -> Response {
    let len = obj.content.len();
    let hash = format!("crc32c={},md5={}", obj.crc32c_b64(), obj.md5_b64());
    let common = [
        (header::CONTENT_TYPE.as_str(), obj.content_type.clone()),
        (header::ETAG.as_str(), format!("\"{}\"", obj.etag)),
        ("x-goog-hash", hash),
        ("x-goog-generation", obj.generation().to_string()),
        ("x-goog-metageneration", "1".to_string()),
        ("x-goog-stored-content-length", len.to_string()),
        ("x-goog-stored-content-encoding", "identity".to_string()),
        (header::ACCEPT_RANGES.as_str(), "bytes".to_string()),
    ];
    match parse_range(header_str(req_headers, "range"), len) {
        None => (StatusCode::OK, common, obj.content).into_response(),
        Some(Ok((start, end))) => {
            let mut resp = (
                StatusCode::PARTIAL_CONTENT,
                common,
                obj.content[start..=end].to_vec(),
            )
                .into_response();
            if let Ok(v) = HeaderValue::from_str(&format!("bytes {start}-{end}/{len}")) {
                resp.headers_mut().insert(header::CONTENT_RANGE, v);
            }
            resp
        }
        Some(Err(())) => {
            let mut resp = gcs_error(
                StatusCode::RANGE_NOT_SATISFIABLE,
                "requestedRangeNotSatisfiable",
                "Requested range not satisfiable",
            );
            if let Ok(v) = HeaderValue::from_str(&format!("bytes */{len}")) {
                resp.headers_mut().insert(header::CONTENT_RANGE, v);
            }
            resp
        }
    }
}

async fn patch_object(
    State(state): State<Arc<AppState>>,
    Path((bucket, object)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let name = object_name(object);
    let patch: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));
    match svc(&state).patch_object(&bucket, &name, patch).await {
        Ok(o) => Json(object_to_json(&o, &base_url(&headers))).into_response(),
        Err(e) => error_response(e),
    }
}

async fn delete_object(
    State(state): State<Arc<AppState>>,
    Path((bucket, object)): Path<(String, String)>,
    Query(params): Query<Params>,
) -> Response {
    let name = object_name(object);
    match svc(&state)
        .delete_object(&bucket, &name, param_i64(&params, "ifGenerationMatch"))
        .await
    {
        Ok(()) => {
            telemetry::emit(
                &state.db,
                "gcp",
                "storage",
                "delete_object",
                &json!({"bucket": bucket, "object": name}).to_string(),
            )
            .await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => error_response(e),
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn multipart_is_binary_safe() {
        let data: Vec<u8> = (0u8..=255).chain(b"\r\n".iter().copied()).collect();
        let mut body = b"--==b==\r\ncontent-type: application/json; charset=UTF-8\r\n\r\n{\"name\":\"x\"}\r\n--==b==\r\ncontent-type: application/octet-stream\r\n\r\n".to_vec();
        body.extend_from_slice(&data);
        body.extend_from_slice(b"\r\n--==b==--");
        let parts = parse_multipart(&body, "==b==");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].0, "application/json; charset=UTF-8");
        assert_eq!(parts[0].1, b"{\"name\":\"x\"}");
        assert_eq!(parts[1].0, "application/octet-stream");
        assert_eq!(parts[1].1, data);
        assert_eq!(
            multipart_boundary("multipart/related; boundary=\"==b==\""),
            Some("==b==".to_string())
        );
    }

    #[test]
    fn content_range_forms() {
        assert_eq!(
            parse_content_range("bytes 0-8/9"),
            Some(ChunkRange {
                start: Some(0),
                total: Some(9)
            })
        );
        assert_eq!(
            parse_content_range("bytes 5-9/*"),
            Some(ChunkRange {
                start: Some(5),
                total: None
            })
        );
        assert_eq!(
            parse_content_range("bytes */0"),
            Some(ChunkRange {
                start: None,
                total: Some(0)
            })
        );
        assert_eq!(parse_content_range("nonsense"), None);
    }

    #[test]
    fn range_header_forms() {
        assert_eq!(parse_range(Some("bytes=0-3"), 10), Some(Ok((0, 3))));
        assert_eq!(parse_range(Some("bytes=7-"), 10), Some(Ok((7, 9))));
        assert_eq!(parse_range(Some("bytes=-2"), 10), Some(Ok((8, 9))));
        assert_eq!(parse_range(Some("bytes=20-"), 10), Some(Err(())));
        assert_eq!(parse_range(None, 10), None);
    }
}

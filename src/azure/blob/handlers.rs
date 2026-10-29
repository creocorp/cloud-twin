//! Azure Blob Storage HTTP handlers.
//!
//! Implements the REST subset the `azure-storage-blob` SDK uses against a
//! custom endpoint (Azurite-style `http://host/{account}`). Operations are
//! selected by HTTP verb plus the `restype`/`comp` query parameters, exactly as
//! in the real service. Shared-key signatures are never validated.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, put},
    Router,
};
use base64::Engine;
use md5::{Digest, Md5};

use super::models::{AzureBlob, AzureContainer};
use super::service::{BlobService, PutOutcome};
use crate::proto::xml_escape;
use crate::telemetry;
use crate::AppState;

/// API version reported when the client does not send `x-ms-version`.
const DEFAULT_API_VERSION: &str = "2021-08-06";

type Params = HashMap<String, String>;

fn svc(state: &Arc<AppState>) -> BlobService {
    BlobService::new(state.db.clone(), state.cfg.azure_account.clone())
}

/// Convert a stored RFC 3339 timestamp to the RFC 1123 form Azure uses.
pub(super) fn rfc1123(ts: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now())
        .format("%a, %d %b %Y %H:%M:%S GMT")
        .to_string()
}

fn container_etag(c: &AzureContainer) -> String {
    let mut h = Md5::new();
    h.update(c.name.as_bytes());
    h.update(c.created_at.as_bytes());
    format!("\"0x{}\"", hex::encode_upper(&h.finalize()[..8]))
}

fn content_md5(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(Md5::digest(data))
}

/// Response headers every Blob response carries.
fn base_headers(req: &HeaderMap) -> HeaderMap {
    let mut h = HeaderMap::new();
    let version = req
        .get("x-ms-version")
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static(DEFAULT_API_VERSION));
    h.insert("x-ms-version", version);
    if let Ok(v) = HeaderValue::from_str(&uuid::Uuid::new_v4().to_string()) {
        h.insert("x-ms-request-id", v);
    }
    if let Some(v) = req.get("x-ms-client-request-id") {
        h.insert("x-ms-client-request-id", v.clone());
    }
    if let Ok(v) = HeaderValue::from_str(&rfc1123(&chrono::Utc::now().to_rfc3339())) {
        h.insert(header::DATE, v);
    }
    h
}

fn set(h: &mut HeaderMap, name: &'static str, value: &str) {
    if let Ok(v) = HeaderValue::from_str(value) {
        h.insert(HeaderName::from_static(name), v);
    }
}

fn xml_ok(req: &HeaderMap, body: String) -> Response {
    let mut h = base_headers(req);
    set(&mut h, "content-type", "application/xml");
    (StatusCode::OK, h, body).into_response()
}

fn empty(req: &HeaderMap, status: StatusCode, extra: HeaderMap) -> Response {
    let mut h = base_headers(req);
    h.extend(extra);
    (status, h).into_response()
}

fn xml_err(req: &HeaderMap, status: StatusCode, code: &str, msg: &str) -> Response {
    let mut h = base_headers(req);
    set(&mut h, "content-type", "application/xml");
    set(&mut h, "x-ms-error-code", code);
    (
        status,
        h,
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?><Error><Code>{code}</Code><Message>{}</Message></Error>",
            xml_escape(msg)
        ),
    )
        .into_response()
}

fn internal(req: &HeaderMap, e: anyhow::Error) -> Response {
    xml_err(
        req,
        StatusCode::INTERNAL_SERVER_ERROR,
        "InternalError",
        &e.to_string(),
    )
}

fn account_not_found(req: &HeaderMap) -> Response {
    xml_err(
        req,
        StatusCode::NOT_FOUND,
        "ResourceNotFound",
        "The specified account does not exist.",
    )
}

fn container_not_found(req: &HeaderMap) -> Response {
    xml_err(
        req,
        StatusCode::NOT_FOUND,
        "ContainerNotFound",
        "The specified container does not exist.",
    )
}

fn blob_not_found(req: &HeaderMap) -> Response {
    xml_err(
        req,
        StatusCode::NOT_FOUND,
        "BlobNotFound",
        "The specified blob does not exist.",
    )
}

fn unsupported(req: &HeaderMap) -> Response {
    xml_err(
        req,
        StatusCode::BAD_REQUEST,
        "UnsupportedQueryParameter",
        "This operation is not supported by CloudTwin.",
    )
}

fn param<'a>(p: &'a Params, k: &str) -> &'a str {
    p.get(k).map(|s| s.as_str()).unwrap_or("")
}

fn header_str<'a>(h: &'a HeaderMap, k: &str) -> Option<&'a str> {
    h.get(k).and_then(|v| v.to_str().ok())
}

/// Router is mounted at `/azure` in main.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // Account operations (the SDK uses `/{account}/?comp=list`)
        .route("/:account", get(account_get))
        .route("/:account/", get(account_get))
        // Container operations (`?restype=container`)
        .route(
            "/:account/:container",
            put(container_put)
                .delete(container_delete)
                .get(container_get)
                .head(container_head),
        )
        // Blob operations
        .route(
            "/:account/:container/*blob",
            put(put_blob)
                .get(get_blob)
                .head(head_blob)
                .delete(delete_blob),
        )
}

// ── Pagination ────────────────────────────────────────────────────────────────

/// Apply `marker`/`maxresults` to a name-sorted list, returning the page and
/// the `NextMarker` (empty when this is the last page).
fn paginate<T>(items: Vec<T>, name: impl Fn(&T) -> &str, params: &Params) -> (Vec<T>, String) {
    let marker = param(params, "marker");
    let max = params
        .get("maxresults")
        .and_then(|m| m.parse::<usize>().ok())
        .filter(|m| *m > 0)
        .unwrap_or(5000);
    let mut page: Vec<T> = items
        .into_iter()
        .filter(|i| marker.is_empty() || name(i) >= marker)
        .collect();
    let next = if page.len() > max {
        let n = name(&page[max]).to_string();
        page.truncate(max);
        n
    } else {
        String::new()
    };
    (page, next)
}

fn endpoint(state: &AppState) -> String {
    format!(
        "{}/{}/",
        state.cfg.base_url.trim_end_matches('/'),
        state.cfg.azure_account
    )
}

// ── Account handlers ──────────────────────────────────────────────────────────

async fn account_get(
    State(state): State<Arc<AppState>>,
    Path(account): Path<String>,
    Query(params): Query<Params>,
    headers: HeaderMap,
) -> Response {
    if account != state.cfg.azure_account {
        return account_not_found(&headers);
    }
    if param(&params, "comp") != "list" {
        return unsupported(&headers);
    }
    let prefix = param(&params, "prefix").to_string();
    let containers = match svc(&state).list_containers().await {
        Ok(c) => c,
        Err(e) => return internal(&headers, e),
    };
    let containers: Vec<_> = containers
        .into_iter()
        .filter(|c| c.name.starts_with(&prefix))
        .collect();
    let (page, next) = paginate(containers, |c| c.name.as_str(), &params);
    let items: String = page
        .iter()
        .map(|c| {
            format!(
                "<Container><Name>{}</Name><Properties>\
<Last-Modified>{}</Last-Modified><Etag>{}</Etag>\
<LeaseStatus>unlocked</LeaseStatus><LeaseState>available</LeaseState>\
<DefaultEncryptionScope>$account-encryption-key</DefaultEncryptionScope>\
<DenyEncryptionScopeOverride>false</DenyEncryptionScopeOverride>\
<HasImmutabilityPolicy>false</HasImmutabilityPolicy><HasLegalHold>false</HasLegalHold>\
</Properties></Container>",
                xml_escape(&c.name),
                rfc1123(&c.created_at),
                container_etag(c),
            )
        })
        .collect();
    xml_ok(
        &headers,
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<EnumerationResults ServiceEndpoint=\"{}\">\
<Prefix>{}</Prefix><Marker>{}</Marker><MaxResults>{}</MaxResults>\
<Containers>{items}</Containers><NextMarker>{}</NextMarker></EnumerationResults>",
            xml_escape(&endpoint(&state)),
            xml_escape(&prefix),
            xml_escape(param(&params, "marker")),
            xml_escape(
                params
                    .get("maxresults")
                    .map(|s| s.as_str())
                    .unwrap_or("5000")
            ),
            xml_escape(&next),
        ),
    )
}

// ── Container handlers ────────────────────────────────────────────────────────

fn container_headers(c: &AzureContainer) -> HeaderMap {
    let mut h = HeaderMap::new();
    set(&mut h, "etag", &container_etag(c));
    set(&mut h, "last-modified", &rfc1123(&c.created_at));
    set(&mut h, "x-ms-lease-status", "unlocked");
    set(&mut h, "x-ms-lease-state", "available");
    set(&mut h, "x-ms-has-immutability-policy", "false");
    set(&mut h, "x-ms-has-legal-hold", "false");
    set(
        &mut h,
        "x-ms-default-encryption-scope",
        "$account-encryption-key",
    );
    set(&mut h, "x-ms-deny-encryption-scope-override", "false");
    h
}

async fn container_put(
    State(state): State<Arc<AppState>>,
    Path((account, container)): Path<(String, String)>,
    Query(params): Query<Params>,
    headers: HeaderMap,
) -> Response {
    if account != state.cfg.azure_account {
        return account_not_found(&headers);
    }
    if param(&params, "restype") != "container" || params.contains_key("comp") {
        return unsupported(&headers);
    }
    let svc = svc(&state);
    match svc.create_container(&container).await {
        Ok(true) => {
            telemetry::emit(
                &state.db,
                "azure",
                "blob",
                "create_container",
                &serde_json::json!({"container": container}).to_string(),
            )
            .await;
            match svc.get_container(&container).await {
                Ok(Some(c)) => empty(&headers, StatusCode::CREATED, container_headers(&c)),
                Ok(None) => container_not_found(&headers),
                Err(e) => internal(&headers, e),
            }
        }
        Ok(false) => xml_err(
            &headers,
            StatusCode::CONFLICT,
            "ContainerAlreadyExists",
            "The specified container already exists.",
        ),
        Err(e) => internal(&headers, e),
    }
}

async fn container_delete(
    State(state): State<Arc<AppState>>,
    Path((account, container)): Path<(String, String)>,
    Query(params): Query<Params>,
    headers: HeaderMap,
) -> Response {
    if account != state.cfg.azure_account {
        return account_not_found(&headers);
    }
    if param(&params, "restype") != "container" || params.contains_key("comp") {
        return unsupported(&headers);
    }
    match svc(&state).delete_container(&container).await {
        Ok(true) => {
            telemetry::emit(
                &state.db,
                "azure",
                "blob",
                "delete_container",
                &serde_json::json!({"container": container}).to_string(),
            )
            .await;
            empty(&headers, StatusCode::ACCEPTED, HeaderMap::new())
        }
        Ok(false) => container_not_found(&headers),
        Err(e) => internal(&headers, e),
    }
}

async fn container_head(
    State(state): State<Arc<AppState>>,
    Path((account, container)): Path<(String, String)>,
    Query(params): Query<Params>,
    headers: HeaderMap,
) -> Response {
    if account != state.cfg.azure_account {
        return empty(&headers, StatusCode::NOT_FOUND, HeaderMap::new());
    }
    if param(&params, "restype") != "container" {
        return empty(&headers, StatusCode::BAD_REQUEST, HeaderMap::new());
    }
    match svc(&state).get_container(&container).await {
        Ok(Some(c)) => empty(&headers, StatusCode::OK, container_headers(&c)),
        Ok(None) => {
            let mut h = HeaderMap::new();
            set(&mut h, "x-ms-error-code", "ContainerNotFound");
            empty(&headers, StatusCode::NOT_FOUND, h)
        }
        Err(_) => empty(
            &headers,
            StatusCode::INTERNAL_SERVER_ERROR,
            HeaderMap::new(),
        ),
    }
}

async fn container_get(
    State(state): State<Arc<AppState>>,
    Path((account, container)): Path<(String, String)>,
    Query(params): Query<Params>,
    headers: HeaderMap,
) -> Response {
    if account != state.cfg.azure_account {
        return account_not_found(&headers);
    }
    if param(&params, "restype") != "container" {
        return unsupported(&headers);
    }
    let svc = svc(&state);
    let c = match svc.get_container(&container).await {
        Ok(Some(c)) => c,
        Ok(None) => return container_not_found(&headers),
        Err(e) => return internal(&headers, e),
    };
    match param(&params, "comp") {
        "" => empty(&headers, StatusCode::OK, container_headers(&c)),
        "list" => list_blobs(&state, &svc, &container, &params, &headers).await,
        _ => unsupported(&headers),
    }
}

/// One entry of a blob listing: a blob, or a virtual directory when a
/// `delimiter` is given.
enum Listed {
    Blob(AzureBlob),
    Prefix(String),
}

impl Listed {
    fn name(&self) -> &str {
        match self {
            Listed::Blob(b) => &b.name,
            Listed::Prefix(p) => p,
        }
    }
}

fn blob_item_xml(b: &AzureBlob) -> String {
    format!(
        "<Blob><Name>{}</Name><Properties>\
<Creation-Time>{lm}</Creation-Time><Last-Modified>{lm}</Last-Modified><Etag>{}</Etag>\
<Content-Length>{}</Content-Length><Content-Type>{}</Content-Type>\
<BlobType>BlockBlob</BlobType><AccessTier>Hot</AccessTier><AccessTierInferred>true</AccessTierInferred>\
<LeaseStatus>unlocked</LeaseStatus><LeaseState>available</LeaseState>\
<ServerEncrypted>false</ServerEncrypted>\
</Properties></Blob>",
        xml_escape(&b.name),
        xml_escape(&b.etag),
        b.size,
        xml_escape(&b.content_type),
        lm = rfc1123(&b.created_at),
    )
}

async fn list_blobs(
    state: &AppState,
    svc: &BlobService,
    container: &str,
    params: &Params,
    headers: &HeaderMap,
) -> Response {
    let prefix = param(params, "prefix");
    let delimiter = param(params, "delimiter");
    let blobs = match svc.list_blobs(container, prefix).await {
        Ok(b) => b,
        Err(e) => return internal(headers, e),
    };
    let mut items: Vec<Listed> = Vec::new();
    for b in blobs {
        let rest = &b.name[prefix.len()..];
        match (!delimiter.is_empty())
            .then(|| rest.find(delimiter))
            .flatten()
        {
            Some(i) => {
                let p = format!("{prefix}{}", &rest[..i + delimiter.len()]);
                if !matches!(items.last(), Some(Listed::Prefix(last)) if *last == p) {
                    items.push(Listed::Prefix(p));
                }
            }
            None => items.push(Listed::Blob(b)),
        }
    }
    let (page, next) = paginate(items, |i| i.name(), params);
    let body: String = page
        .iter()
        .map(|i| match i {
            Listed::Blob(b) => blob_item_xml(b),
            Listed::Prefix(p) => format!("<BlobPrefix><Name>{}</Name></BlobPrefix>", xml_escape(p)),
        })
        .collect();
    let delim_xml = if delimiter.is_empty() {
        String::new()
    } else {
        format!("<Delimiter>{}</Delimiter>", xml_escape(delimiter))
    };
    xml_ok(
        headers,
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<EnumerationResults ServiceEndpoint=\"{}\" ContainerName=\"{}\">\
<Prefix>{}</Prefix><Marker>{}</Marker><MaxResults>{}</MaxResults>{delim_xml}\
<Blobs>{body}</Blobs><NextMarker>{}</NextMarker></EnumerationResults>",
            xml_escape(&endpoint(state)),
            xml_escape(container),
            xml_escape(prefix),
            xml_escape(param(params, "marker")),
            xml_escape(
                params
                    .get("maxresults")
                    .map(|s| s.as_str())
                    .unwrap_or("5000")
            ),
            xml_escape(&next),
        ),
    )
}

// ── Blob handlers ─────────────────────────────────────────────────────────────

fn blob_headers(b: &AzureBlob) -> HeaderMap {
    let mut h = HeaderMap::new();
    set(&mut h, "etag", &b.etag);
    set(&mut h, "last-modified", &rfc1123(&b.created_at));
    set(&mut h, "x-ms-creation-time", &rfc1123(&b.created_at));
    set(&mut h, "x-ms-blob-type", "BlockBlob");
    set(&mut h, "x-ms-lease-status", "unlocked");
    set(&mut h, "x-ms-lease-state", "available");
    set(&mut h, "x-ms-server-encrypted", "false");
    set(&mut h, "x-ms-access-tier", "Hot");
    set(&mut h, "x-ms-access-tier-inferred", "true");
    set(&mut h, "accept-ranges", "bytes");
    set(&mut h, "content-type", &b.content_type);
    h
}

/// Evaluate `If-Match` / `If-None-Match` for reads; `Some(response)` aborts.
fn read_conditions(req: &HeaderMap, b: &AzureBlob) -> Option<Response> {
    if let Some(m) = header_str(req, "if-match") {
        if m != "*" && m != b.etag {
            return Some(xml_err(
                req,
                StatusCode::PRECONDITION_FAILED,
                "ConditionNotMet",
                "The condition specified using HTTP conditional header(s) is not met.",
            ));
        }
    }
    if let Some(m) = header_str(req, "if-none-match") {
        if m == "*" || m == b.etag {
            let mut h = HeaderMap::new();
            set(&mut h, "etag", &b.etag);
            return Some(empty(req, StatusCode::NOT_MODIFIED, h));
        }
    }
    None
}

async fn put_blob(
    State(state): State<Arc<AppState>>,
    Path((account, container, blob)): Path<(String, String, String)>,
    Query(params): Query<Params>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if account != state.cfg.azure_account {
        return account_not_found(&headers);
    }
    // Only single-shot Put Blob of block blobs is supported (no staged blocks,
    // page/append blobs, metadata or property updates).
    if params.contains_key("comp") {
        return unsupported(&headers);
    }
    match header_str(&headers, "x-ms-blob-type") {
        None | Some("BlockBlob") => {}
        Some(_) => {
            return xml_err(
                &headers,
                StatusCode::BAD_REQUEST,
                "InvalidBlobType",
                "Only BlockBlob is supported by CloudTwin.",
            )
        }
    }
    let ct = header_str(&headers, "x-ms-blob-content-type")
        .or_else(|| header_str(&headers, "content-type"))
        .unwrap_or("application/octet-stream")
        .to_string();
    let if_none_match_any = header_str(&headers, "if-none-match") == Some("*");
    let md5 = content_md5(&body);
    match svc(&state)
        .put_blob(&container, &blob, body.to_vec(), &ct, if_none_match_any)
        .await
    {
        Ok(PutOutcome::Stored(b)) => {
            telemetry::emit(
                &state.db,
                "azure",
                "blob",
                "put_blob",
                &serde_json::json!({"container": container, "blob": blob}).to_string(),
            )
            .await;
            let mut h = HeaderMap::new();
            set(&mut h, "etag", &b.etag);
            set(&mut h, "last-modified", &rfc1123(&b.created_at));
            set(&mut h, "content-md5", &md5);
            set(&mut h, "x-ms-request-server-encrypted", "false");
            empty(&headers, StatusCode::CREATED, h)
        }
        Ok(PutOutcome::ContainerNotFound) => container_not_found(&headers),
        Ok(PutOutcome::AlreadyExists) => xml_err(
            &headers,
            StatusCode::CONFLICT,
            "BlobAlreadyExists",
            "The specified blob already exists.",
        ),
        Err(e) => internal(&headers, e),
    }
}

/// Parse `bytes=start-[end]` into an inclusive range clamped to `total`.
/// `Err(())` means the range is unsatisfiable.
fn parse_range(spec: &str, total: usize) -> Option<Result<(usize, usize), ()>> {
    let r = spec.trim().strip_prefix("bytes=")?;
    let (s, e) = r.split_once('-')?;
    let start: usize = s.trim().parse().ok()?;
    if start >= total {
        return Some(Err(()));
    }
    let end = match e.trim() {
        "" => total - 1,
        e => e.parse::<usize>().ok()?.min(total - 1),
    };
    if end < start {
        return Some(Err(()));
    }
    Some(Ok((start, end)))
}

async fn get_blob(
    State(state): State<Arc<AppState>>,
    Path((account, container, blob)): Path<(String, String, String)>,
    Query(params): Query<Params>,
    headers: HeaderMap,
) -> Response {
    if account != state.cfg.azure_account {
        return account_not_found(&headers);
    }
    if params.contains_key("comp") {
        return unsupported(&headers);
    }
    let b = match svc(&state).get_blob(&container, &blob).await {
        Ok(Some(b)) => b,
        Ok(None) => return blob_not_found(&headers),
        Err(e) => return internal(&headers, e),
    };
    if let Some(r) = read_conditions(&headers, &b) {
        return r;
    }
    let total = b.content.len();
    let mut h = blob_headers(&b);
    let range = header_str(&headers, "x-ms-range").or_else(|| header_str(&headers, "range"));
    match range.and_then(|r| parse_range(r, total)) {
        Some(Ok((start, end))) => {
            let chunk = b.content[start..=end].to_vec();
            set(
                &mut h,
                "content-range",
                &format!("bytes {start}-{end}/{total}"),
            );
            if header_str(&headers, "x-ms-range-get-content-md5") == Some("true") {
                set(&mut h, "content-md5", &content_md5(&chunk));
            }
            set(&mut h, "x-ms-blob-content-md5", &content_md5(&b.content));
            empty(&headers, StatusCode::PARTIAL_CONTENT, h).map(|_| axum::body::Body::from(chunk))
        }
        Some(Err(())) => {
            let mut r = xml_err(
                &headers,
                StatusCode::RANGE_NOT_SATISFIABLE,
                "InvalidRange",
                "The range specified is invalid for the current size of the resource.",
            );
            set(
                r.headers_mut(),
                "content-range",
                &format!("bytes */{total}"),
            );
            r
        }
        None => {
            set(&mut h, "content-md5", &content_md5(&b.content));
            empty(&headers, StatusCode::OK, h).map(|_| axum::body::Body::from(b.content))
        }
    }
}

async fn head_blob(
    State(state): State<Arc<AppState>>,
    Path((account, container, blob)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if account != state.cfg.azure_account {
        return empty(&headers, StatusCode::NOT_FOUND, HeaderMap::new());
    }
    match svc(&state).get_blob(&container, &blob).await {
        Ok(Some(b)) => {
            if let Some(r) = read_conditions(&headers, &b) {
                return r;
            }
            let mut h = blob_headers(&b);
            set(&mut h, "content-length", &b.content.len().to_string());
            set(&mut h, "content-md5", &content_md5(&b.content));
            empty(&headers, StatusCode::OK, h)
        }
        Ok(None) => {
            let mut h = HeaderMap::new();
            set(&mut h, "x-ms-error-code", "BlobNotFound");
            empty(&headers, StatusCode::NOT_FOUND, h)
        }
        Err(_) => empty(
            &headers,
            StatusCode::INTERNAL_SERVER_ERROR,
            HeaderMap::new(),
        ),
    }
}

async fn delete_blob(
    State(state): State<Arc<AppState>>,
    Path((account, container, blob)): Path<(String, String, String)>,
    Query(params): Query<Params>,
    headers: HeaderMap,
) -> Response {
    if account != state.cfg.azure_account {
        return account_not_found(&headers);
    }
    if params.contains_key("comp") {
        return unsupported(&headers);
    }
    match svc(&state).delete_blob(&container, &blob).await {
        Ok(true) => {
            telemetry::emit(
                &state.db,
                "azure",
                "blob",
                "delete_blob",
                &serde_json::json!({"container": container, "blob": blob}).to_string(),
            )
            .await;
            let mut h = HeaderMap::new();
            set(&mut h, "x-ms-delete-type-permanent", "true");
            empty(&headers, StatusCode::ACCEPTED, h)
        }
        Ok(false) => blob_not_found(&headers),
        Err(e) => internal(&headers, e),
    }
}

//! Object-level S3 operations (`/:bucket/*key`).

use std::sync::Arc;

use axum::{
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
};
use base64::Engine;
use serde_json::json;

use super::chunked;
use super::errors::{S3Error, S3Result};
use super::handlers::{
    emit_json, empty, header_pairs, object_attrs, sse_headers, verify_checksums, with_headers,
    xml_ok, S3Req,
};
use super::models::{DeleteOutcome, ObjMeta, ObjectInfo, Versioning, WriteResult};
use super::multipart;
use super::notify::{self, Event};
use super::xmlutil::{
    http_date, iso, parse_http_date, parse_tagging, tag, tagging_xml, url_decode, XMLNS, XML_DECL,
};
use crate::AppState;

pub async fn dispatch(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    match r.method.clone() {
        Method::GET if r.has("tagging") => get_tagging(state, r).await,
        Method::GET if r.has("acl") => {
            r.svc(state)
                .object_info(&r.bucket, r.key(), r.q("versionId"))
                .await?;
            Ok(xml_ok(super::bucket::acl_xml()))
        }
        Method::GET if r.has("uploadId") => multipart::list_parts(state, r).await,
        Method::GET if r.has("attributes") => get_attributes(state, r).await,
        Method::GET => get_object(state, r, false).await,
        Method::HEAD => get_object(state, r, true).await,
        Method::PUT if r.has("tagging") => put_tagging(state, r).await,
        Method::PUT if r.has("acl") => {
            r.svc(state)
                .object_info(&r.bucket, r.key(), r.q("versionId"))
                .await?;
            Ok(empty(StatusCode::OK))
        }
        Method::PUT if r.has("uploadId") || r.has("partNumber") => {
            multipart::upload_part(state, r).await
        }
        Method::PUT if r.h("x-amz-copy-source").is_some() => copy_object(state, r).await,
        Method::PUT => put_object(state, r).await,
        Method::DELETE if r.has("tagging") => delete_tagging(state, r).await,
        Method::DELETE if r.has("uploadId") => multipart::abort(state, r).await,
        Method::DELETE => delete_object(state, r).await,
        Method::POST if r.has("uploads") => multipart::create(state, r).await,
        Method::POST if r.has("uploadId") => multipart::complete(state, r).await,
        _ => Err(S3Error::new(
            "NotImplemented",
            "A header or query you provided implies functionality that is not implemented.",
        )),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Request body with `aws-chunked` framing removed, plus any trailers.
pub type DecodedBody = (Vec<u8>, Vec<(String, String)>);

pub fn decode_body(r: &S3Req) -> S3Result<DecodedBody> {
    if chunked::is_aws_chunked(&r.headers) {
        let d = chunked::decode(&r.body).map_err(|e| {
            S3Error::new(
                "IncompleteBody",
                format!("Invalid aws-chunked request body: {e}"),
            )
        })?;
        Ok((d.data, d.trailers))
    } else {
        Ok((r.body.to_vec(), Vec::new()))
    }
}

pub fn check_content_md5(headers: &HeaderMap, content: &[u8]) -> S3Result<()> {
    let Some(v) = headers.get("content-md5").and_then(|v| v.to_str().ok()) else {
        return Ok(());
    };
    let given = base64::engine::general_purpose::STANDARD
        .decode(v.trim())
        .map_err(|_| {
            S3Error::new(
                "InvalidDigest",
                "The Content-MD5 you specified was invalid.",
            )
        })?;
    use md5::Digest;
    if given != md5::Md5::digest(content).as_slice() {
        return Err(S3Error::new(
            "BadDigest",
            "The Content-MD5 you specified did not match what we received.",
        ));
    }
    Ok(())
}

fn etag_list_matches(list: &str, etag: &str) -> bool {
    let etag = etag.trim_matches('"');
    list.split(',').any(|e| {
        let e = e.trim().trim_start_matches("W/").trim_matches('"');
        e == "*" || e == etag
    })
}

/// Evaluate If-Match / If-None-Match / If-(Un)Modified-Since (optionally with
/// the `x-amz-copy-source-` prefix). Returns `Some(304)` for not-modified.
pub fn check_conditionals(
    headers: &HeaderMap,
    prefix: &str,
    etag: &str,
    last_modified: &str,
) -> S3Result<Option<StatusCode>> {
    let h = |n: &str| {
        headers
            .get(format!("{prefix}{n}"))
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let lm = super::xmlutil::parse_ts(last_modified).timestamp();
    let failed = |cond: &str| {
        S3Error::new(
            "PreconditionFailed",
            "At least one of the pre-conditions you specified did not hold",
        )
        .with("Condition", cond.to_string())
    };
    let copy = !prefix.is_empty();
    let if_match = h("if-match");
    if let Some(m) = &if_match {
        if !etag_list_matches(m, etag) {
            return Err(failed("If-Match"));
        }
    } else if let Some(d) = h("if-unmodified-since").and_then(|d| parse_http_date(&d)) {
        if lm > d.timestamp() {
            return Err(failed("If-Unmodified-Since"));
        }
    }
    let not_modified = if let Some(m) = h("if-none-match") {
        etag_list_matches(&m, etag).then_some("If-None-Match")
    } else if let Some(d) = h("if-modified-since").and_then(|d| parse_http_date(&d)) {
        (lm <= d.timestamp()).then_some("If-Modified-Since")
    } else {
        None
    };
    match not_modified {
        Some(c) if copy => Err(failed(c)),
        Some(_) => Ok(Some(StatusCode::NOT_MODIFIED)),
        None => Ok(None),
    }
}

/// Parse a single `bytes=` range. None = ignore; Some(None) = unsatisfiable.
pub fn parse_range(v: &str, size: u64) -> Option<Option<(u64, u64)>> {
    let spec = v.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (a, b) = (a.trim(), b.trim());
    if a.is_empty() {
        let n: u64 = b.parse().ok()?;
        if n == 0 || size == 0 {
            return Some(None);
        }
        return Some(Some((size.saturating_sub(n), size - 1)));
    }
    let start: u64 = a.parse().ok()?;
    let end: Option<u64> = if b.is_empty() {
        None
    } else {
        Some(b.parse().ok()?)
    };
    if end.is_some_and(|e| e < start) {
        return None;
    }
    if start >= size {
        return Some(None);
    }
    Some(Some((start, end.map_or(size - 1, |e| e.min(size - 1)))))
}

fn version_header(info_vid: &str, versioning: Versioning) -> Vec<(String, String)> {
    if versioning.is_configured() || info_vid != "null" {
        vec![("x-amz-version-id".into(), info_vid.to_string())]
    } else {
        Vec::new()
    }
}

pub fn write_headers(wr: &WriteResult) -> Vec<(String, String)> {
    let mut h = vec![("etag".to_string(), wr.etag.clone())];
    h.extend(version_header(&wr.version_id, wr.versioning));
    h.extend(sse_headers(&wr.meta));
    h
}

pub fn event_version(wr_vid: &str, versioning: Versioning) -> Option<&str> {
    versioning.is_configured().then_some(wr_vid)
}

pub async fn fire_created(
    state: &Arc<AppState>,
    r_bucket: &str,
    key: &str,
    name: &str,
    wr: &WriteResult,
    request_id: &str,
) {
    notify::fire(
        state,
        Event {
            bucket: r_bucket,
            key,
            name,
            size: Some(wr.size),
            etag: Some(&wr.etag),
            version_id: event_version(&wr.version_id, wr.versioning),
            seq: wr.seq,
            request_id,
        },
    )
    .await;
}

/// Parse `x-amz-copy-source` (`[/]bucket/key[?versionId=..]`, URL-encoded).
pub fn parse_copy_source(src: &str) -> S3Result<(String, String, Option<String>)> {
    let (path, vid) = match src.split_once("?versionId=") {
        Some((p, v)) => (p, Some(url_decode(v))),
        None => (src.split('?').next().unwrap_or(src), None),
    };
    let path = url_decode(path);
    let path = path.trim_start_matches('/');
    match path.split_once('/') {
        Some((b, k)) if !b.is_empty() && !k.is_empty() => Ok((b.to_string(), k.to_string(), vid)),
        _ => Err(S3Error::invalid_argument("Invalid copy source object key")
            .with("ArgumentName", "x-amz-copy-source")
            .with("ArgumentValue", src.to_string())),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PutObject / GetObject / HeadObject / DeleteObject
// ─────────────────────────────────────────────────────────────────────────────

async fn put_object(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let (content, trailers) = decode_body(r)?;
    let mut pairs = header_pairs(&r.headers);
    pairs.extend(trailers);
    let attrs = object_attrs(&pairs)?;
    check_content_md5(&r.headers, &content)?;
    verify_checksums(&attrs.meta, &content)?;
    let svc = r.svc(state);
    if let Some(m) = r.h("if-none-match") {
        if m.trim() == "*" && svc.object_info(&r.bucket, r.key(), None).await.is_ok() {
            return Err(S3Error::new(
                "PreconditionFailed",
                "At least one of the pre-conditions you specified did not hold",
            )
            .with("Condition", "If-None-Match"));
        }
    }
    if let Some(m) = r.h("if-match") {
        let cur = svc.object_info(&r.bucket, r.key(), None).await?;
        if !etag_list_matches(&m, &cur.etag) {
            return Err(S3Error::new(
                "PreconditionFailed",
                "At least one of the pre-conditions you specified did not hold",
            )
            .with("Condition", "If-Match"));
        }
    }
    let checksums = attrs.meta.checksums.clone();
    let wr = svc
        .put_object(
            &r.bucket,
            r.key(),
            content,
            attrs
                .content_type
                .as_deref()
                .unwrap_or("binary/octet-stream"),
            attrs.meta,
            attrs.tags.unwrap_or_default(),
        )
        .await?;
    emit_json(
        state,
        "put_object",
        json!({"bucket": r.bucket, "key": r.key(), "size": wr.size, "version_id": wr.version_id}),
    )
    .await;
    fire_created(
        state,
        &r.bucket,
        r.key(),
        "ObjectCreated:Put",
        &wr,
        &r.request_id,
    )
    .await;
    let mut h = write_headers(&wr);
    h.extend(checksums);
    Ok(with_headers(empty(StatusCode::OK), &h))
}

const RESPONSE_OVERRIDES: &[(&str, &str)] = &[
    ("response-cache-control", "cache-control"),
    ("response-content-disposition", "content-disposition"),
    ("response-content-encoding", "content-encoding"),
    ("response-content-language", "content-language"),
    ("response-expires", "expires"),
];

fn object_headers(info: &ObjectInfo, versioning: Versioning, r: &S3Req) -> Vec<(String, String)> {
    let mut h: Vec<(String, String)> = vec![
        (
            "content-type".into(),
            r.q("response-content-type")
                .unwrap_or(&info.content_type)
                .to_string(),
        ),
        ("etag".into(), info.etag.clone()),
        ("last-modified".into(), http_date(&info.created_at)),
        ("accept-ranges".into(), "bytes".into()),
    ];
    h.extend(version_header(&info.version_id, versioning));
    for (k, v) in &info.meta.headers {
        if !RESPONSE_OVERRIDES.iter().any(|(q, hn)| hn == k && r.has(q)) {
            h.push((k.clone(), v.clone()));
        }
    }
    for (q, hn) in RESPONSE_OVERRIDES {
        if let Some(v) = r.q(q) {
            h.push((hn.to_string(), v.to_string()));
        }
    }
    for (k, v) in &info.meta.user {
        h.push((format!("x-amz-meta-{k}"), v.clone()));
    }
    h.extend(sse_headers(&info.meta));
    if info.meta.storage_class() != "STANDARD" {
        h.push((
            "x-amz-storage-class".into(),
            info.meta.storage_class().to_string(),
        ));
    }
    if let Some(w) = &info.meta.website_redirect {
        h.push(("x-amz-website-redirect-location".into(), w.clone()));
    }
    if !info.tags.is_empty() {
        h.push(("x-amz-tagging-count".into(), info.tags.len().to_string()));
    }
    h
}

async fn get_object(state: &Arc<AppState>, r: &S3Req, head: bool) -> S3Result<Response> {
    let svc = r.svc(state);
    for _ in 0..3 {
        let info = svc
            .object_info(&r.bucket, r.key(), r.q("versionId"))
            .await?;
        let versioning = if info.version_id == "null" {
            svc.versioning(&r.bucket).await?
        } else {
            Versioning::Enabled
        };
        if let Some(status) = check_conditionals(&r.headers, "", &info.etag, &info.created_at)? {
            let mut resp = empty(status);
            super::handlers::add_headers(
                &mut resp,
                &[
                    ("etag".into(), info.etag.clone()),
                    ("last-modified".into(), http_date(&info.created_at)),
                ],
            );
            return Ok(resp);
        }
        let size = info.size as u64;
        let mut headers = object_headers(&info, versioning, r);
        let mut range: Option<(u64, u64)> = None;
        if let Some(pn) = r.q("partNumber") {
            let pn: usize = pn.parse().ok().filter(|n| *n >= 1).ok_or_else(|| {
                S3Error::invalid_argument(
                    "Part number must be an integer between 1 and 10000, inclusive",
                )
            })?;
            let parts = if info.meta.parts.is_empty() {
                vec![info.size]
            } else {
                info.meta.parts.clone()
            };
            if pn > parts.len() {
                return Err(S3Error::new(
                    "InvalidPartNumber",
                    "The requested partnumber is not satisfiable",
                )
                .with("PartNumberRequested", pn.to_string())
                .with("ActualPartCount", parts.len().to_string()));
            }
            let start: i64 = parts[..pn - 1].iter().sum();
            let len = parts[pn - 1];
            if len > 0 {
                range = Some((start as u64, (start + len - 1) as u64));
            }
            headers.push(("x-amz-mp-parts-count".into(), parts.len().to_string()));
        } else if let Some(rh) = r.h("range") {
            match parse_range(&rh, size) {
                None => {}
                Some(None) => {
                    return Err(S3Error::new(
                        "InvalidRange",
                        "The requested range is not satisfiable",
                    )
                    .with("RangeRequested", rh.clone())
                    .with("ActualObjectSize", size.to_string())
                    .header("content-range", format!("bytes */{size}")));
                }
                Some(Some(rg)) => range = Some(rg),
            }
        }
        let checksum_mode = r
            .h("x-amz-checksum-mode")
            .is_some_and(|v| v.eq_ignore_ascii_case("ENABLED"));
        if checksum_mode && range.is_none() && info.meta.parts.is_empty() {
            headers.extend(info.meta.checksums.iter().cloned());
        }
        let status = match range {
            Some((s, e)) => {
                headers.push(("content-range".into(), format!("bytes {s}-{e}/{size}")));
                StatusCode::PARTIAL_CONTENT
            }
            None => StatusCode::OK,
        };
        let length = range.map_or(size, |(s, e)| e - s + 1);
        if head {
            let mut resp = with_headers(empty(status), &headers);
            resp.headers_mut()
                .insert(header::CONTENT_LENGTH, HeaderValue::from(length));
            return Ok(resp);
        }
        let Some(content) = svc
            .read_content(&info, range.map(|(s, e)| (s, e - s + 1)))
            .await?
        else {
            continue;
        };
        return Ok(with_headers((status, content).into_response(), &headers));
    }
    Err(S3Error::internal("object changed while reading"))
}

/// GetObjectAttributes (`GET ?attributes`).
async fn get_attributes(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let svc = r.svc(state);
    let info = svc
        .object_info(&r.bucket, r.key(), r.q("versionId"))
        .await?;
    let versioning = if info.version_id == "null" {
        svc.versioning(&r.bucket).await?
    } else {
        Versioning::Enabled
    };
    let mut x = format!("{XML_DECL}<GetObjectAttributesResponse xmlns=\"{XMLNS}\">");
    x.push_str(&tag("ETag", info.etag.trim_matches('"')));
    let checksums: String = info
        .meta
        .checksums
        .iter()
        .filter_map(|(k, v)| checksum_element(k).map(|n| tag(n, v)))
        .collect();
    if !checksums.is_empty() && info.meta.parts.is_empty() {
        x.push_str(&format!("<Checksum>{checksums}</Checksum>"));
    }
    if !info.meta.parts.is_empty() {
        x.push_str(&format!(
            "<ObjectParts><PartsCount>{}</PartsCount></ObjectParts>",
            info.meta.parts.len()
        ));
    }
    x.push_str(&tag("StorageClass", info.meta.storage_class()));
    x.push_str(&format!("<ObjectSize>{}</ObjectSize>", info.size));
    x.push_str("</GetObjectAttributesResponse>");
    let mut h = vec![("last-modified".to_string(), http_date(&info.created_at))];
    h.extend(version_header(&info.version_id, versioning));
    Ok(with_headers(xml_ok(x), &h))
}

pub fn checksum_element(header: &str) -> Option<&'static str> {
    Some(match header {
        "x-amz-checksum-crc32" => "ChecksumCRC32",
        "x-amz-checksum-crc32c" => "ChecksumCRC32C",
        "x-amz-checksum-crc64nvme" => "ChecksumCRC64NVME",
        "x-amz-checksum-sha1" => "ChecksumSHA1",
        "x-amz-checksum-sha256" => "ChecksumSHA256",
        _ => return None,
    })
}

pub async fn after_delete(
    state: &Arc<AppState>,
    r: &S3Req,
    key: &str,
    requested_vid: Option<&str>,
    out: &DeleteOutcome,
) {
    emit_json(state, "delete_object", json!({"bucket": r.bucket, "key": key, "version_id": out.version_id, "delete_marker": out.delete_marker})).await;
    let name = if requested_vid.is_none() && out.delete_marker {
        "ObjectRemoved:DeleteMarkerCreated"
    } else {
        "ObjectRemoved:Delete"
    };
    notify::fire(
        state,
        Event {
            bucket: &r.bucket,
            key,
            name,
            size: None,
            etag: None,
            version_id: if out.versioning.is_configured() {
                out.version_id.as_deref()
            } else {
                None
            },
            seq: out.seq,
            request_id: &r.request_id,
        },
    )
    .await;
}

async fn delete_object(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let vid = r.q("versionId");
    let out = r.svc(state).delete_object(&r.bucket, r.key(), vid).await?;
    after_delete(state, r, r.key(), vid, &out).await;
    let mut h = Vec::new();
    if out.delete_marker {
        h.push(("x-amz-delete-marker".to_string(), "true".to_string()));
    }
    if let Some(v) = &out.version_id {
        h.push(("x-amz-version-id".to_string(), v.clone()));
    }
    Ok(with_headers(empty(StatusCode::NO_CONTENT), &h))
}

// ─────────────────────────────────────────────────────────────────────────────
// CopyObject
// ─────────────────────────────────────────────────────────────────────────────

async fn copy_object(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let src = r.h("x-amz-copy-source").unwrap_or_default();
    let (sb, sk, svid) = parse_copy_source(&src)?;
    let svc = r.svc(state);
    if !svc.bucket_exists(&r.bucket).await? {
        return Err(S3Error::no_such_bucket(&r.bucket));
    }
    let (sinfo, content) = svc.get_object_full(&sb, &sk, svid.as_deref()).await?;
    check_conditionals(
        &r.headers,
        "x-amz-copy-source-",
        &sinfo.etag,
        &sinfo.created_at,
    )?;
    let pairs = header_pairs(&r.headers);
    let attrs = object_attrs(&pairs)?;
    let directive = r
        .h("x-amz-metadata-directive")
        .unwrap_or_else(|| "COPY".into())
        .to_ascii_uppercase();
    if directive != "COPY" && directive != "REPLACE" {
        return Err(S3Error::invalid_argument("Unknown metadata directive.")
            .with("ArgumentName", "x-amz-metadata-directive")
            .with("ArgumentValue", directive));
    }
    let tag_directive = r
        .h("x-amz-tagging-directive")
        .unwrap_or_else(|| "COPY".into())
        .to_ascii_uppercase();
    let changes_encryption = attrs
        .meta
        .sse
        .iter()
        .any(|(k, _)| k == "x-amz-server-side-encryption");
    if sb == r.bucket
        && sk == r.key()
        && svid.is_none()
        && directive == "COPY"
        && !changes_encryption
        && attrs.meta.storage_class.is_none()
        && attrs.meta.website_redirect.is_none()
    {
        return Err(S3Error::new(
            "InvalidRequest",
            "This copy request is illegal because it is trying to copy an object to itself without changing the object's metadata, storage class, website redirect location or encryption attributes.",
        ));
    }
    let (content_type, mut meta) = if directive == "REPLACE" {
        (
            attrs
                .content_type
                .clone()
                .unwrap_or_else(|| "binary/octet-stream".into()),
            attrs.meta.clone(),
        )
    } else {
        (
            sinfo.content_type.clone(),
            ObjMeta {
                user: sinfo.meta.user.clone(),
                headers: sinfo.meta.headers.clone(),
                sse: attrs.meta.sse.clone(),
                storage_class: attrs.meta.storage_class.clone(),
                website_redirect: attrs.meta.website_redirect.clone(),
                ..Default::default()
            },
        )
    };
    meta.checksums = if sinfo.meta.parts.is_empty() {
        sinfo.meta.checksums.clone()
    } else {
        Vec::new()
    };
    let tags = if tag_directive == "REPLACE" {
        attrs.tags.unwrap_or_default()
    } else {
        sinfo.tags.clone()
    };
    let wr = svc
        .put_object(&r.bucket, r.key(), content, &content_type, meta, tags)
        .await?;
    emit_json(
        state,
        "copy_object",
        json!({"bucket": r.bucket, "key": r.key(), "source": format!("{sb}/{sk}")}),
    )
    .await;
    fire_created(
        state,
        &r.bucket,
        r.key(),
        "ObjectCreated:Copy",
        &wr,
        &r.request_id,
    )
    .await;
    let mut h = write_headers(&wr);
    h.retain(|(k, _)| k != "etag");
    if svid.is_some() || sinfo.version_id != "null" {
        h.push((
            "x-amz-copy-source-version-id".into(),
            sinfo.version_id.clone(),
        ));
    }
    let checksums: String = wr
        .meta
        .checksums
        .iter()
        .filter_map(|(k, v)| checksum_element(k).map(|n| tag(n, v)))
        .collect();
    Ok(with_headers(
        xml_ok(format!(
            "{XML_DECL}<CopyObjectResult xmlns=\"{XMLNS}\"><LastModified>{}</LastModified>{}{checksums}</CopyObjectResult>",
            iso(&wr.created_at),
            tag("ETag", &wr.etag)
        )),
        &h,
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// Object tagging
// ─────────────────────────────────────────────────────────────────────────────

fn tagging_version_header(info: &ObjectInfo) -> Vec<(String, String)> {
    if info.version_id != "null" {
        vec![("x-amz-version-id".into(), info.version_id.clone())]
    } else {
        Vec::new()
    }
}

async fn get_tagging(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let info = r
        .svc(state)
        .object_info(&r.bucket, r.key(), r.q("versionId"))
        .await?;
    Ok(with_headers(
        xml_ok(tagging_xml(&info.tags)),
        &tagging_version_header(&info),
    ))
}

async fn put_tagging(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let tags = parse_tagging(&r.body)?;
    super::xmlutil::validate_tags(&tags, 10)?;
    let info = r
        .svc(state)
        .set_object_tags(&r.bucket, r.key(), r.q("versionId"), tags)
        .await?;
    emit_json(
        state,
        "put_object_tagging",
        json!({"bucket": r.bucket, "key": r.key()}),
    )
    .await;
    Ok(with_headers(
        empty(StatusCode::OK),
        &tagging_version_header(&info),
    ))
}

async fn delete_tagging(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let info = r
        .svc(state)
        .set_object_tags(&r.bucket, r.key(), r.q("versionId"), Vec::new())
        .await?;
    emit_json(
        state,
        "delete_object_tagging",
        json!({"bucket": r.bucket, "key": r.key()}),
    )
    .await;
    Ok(with_headers(
        empty(StatusCode::NO_CONTENT),
        &tagging_version_header(&info),
    ))
}

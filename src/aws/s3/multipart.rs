//! Multipart upload operations.

use std::sync::Arc;

use axum::{http::StatusCode, response::Response};
use serde_json::json;

use super::errors::{S3Error, S3Result};
use super::handlers::{
    emit_json, empty, header_pairs, object_attrs, sse_headers, verify_checksums, with_headers,
    xml_ok, S3Req,
};
use super::models::ObjMeta;
use super::object::{
    check_conditionals, check_content_md5, decode_body, fire_created, parse_copy_source,
    write_headers,
};
use super::xmlutil::{iso, tag, url_encode, XMLNS, XML_DECL};
use crate::AppState;

fn upload_id(r: &S3Req) -> S3Result<&str> {
    r.q("uploadId")
        .filter(|u| !u.is_empty())
        .ok_or_else(|| S3Error::no_such_upload(""))
}

pub async fn create(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let attrs = object_attrs(&header_pairs(&r.headers))?;
    let mut meta = attrs.meta;
    meta.checksums.clear();
    let up = r
        .svc(state)
        .create_upload(
            &r.bucket,
            r.key(),
            attrs
                .content_type
                .as_deref()
                .unwrap_or("binary/octet-stream"),
            meta,
            attrs.tags.unwrap_or_default(),
        )
        .await?;
    emit_json(
        state,
        "create_multipart_upload",
        json!({"bucket": r.bucket, "key": r.key(), "upload_id": up.upload_id}),
    )
    .await;
    let mut h = sse_headers(&up.meta);
    if let Some(a) = r.h("x-amz-checksum-algorithm") {
        h.push(("x-amz-checksum-algorithm".into(), a));
    }
    Ok(with_headers(
        xml_ok(format!(
            "{XML_DECL}<InitiateMultipartUploadResult xmlns=\"{XMLNS}\">{}{}{}</InitiateMultipartUploadResult>",
            tag("Bucket", &r.bucket),
            tag("Key", r.key()),
            tag("UploadId", &up.upload_id)
        )),
        &h,
    ))
}

fn part_number(r: &S3Req) -> S3Result<i64> {
    r.q("partNumber")
        .and_then(|p| p.parse().ok())
        .ok_or_else(|| {
            S3Error::invalid_argument(
                "Part number must be an integer between 1 and 10000, inclusive",
            )
            .with("ArgumentName", "partNumber")
        })
}

pub async fn upload_part(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let id = upload_id(r)?;
    let num = part_number(r)?;
    let svc = r.svc(state);
    let up = svc.get_upload(&r.bucket, r.key(), id).await?;
    if r.h("x-amz-copy-source").is_some() {
        return upload_part_copy(state, r, id, num, &up.meta).await;
    }
    let (content, trailers) = decode_body(r)?;
    let mut pairs = header_pairs(&r.headers);
    pairs.extend(trailers);
    let meta = ObjMeta {
        checksums: object_attrs(&pairs)?.meta.checksums,
        ..Default::default()
    };
    check_content_md5(&r.headers, &content)?;
    verify_checksums(&meta, &content)?;
    let etag = svc
        .upload_part(&r.bucket, r.key(), id, num, content, meta.clone())
        .await?;
    emit_json(
        state,
        "upload_part",
        json!({"bucket": r.bucket, "key": r.key(), "upload_id": id, "part_number": num}),
    )
    .await;
    let mut h = vec![("etag".to_string(), etag)];
    h.extend(sse_headers(&up.meta));
    h.extend(meta.checksums);
    Ok(with_headers(empty(StatusCode::OK), &h))
}

async fn upload_part_copy(
    state: &Arc<AppState>,
    r: &S3Req,
    id: &str,
    num: i64,
    up_meta: &ObjMeta,
) -> S3Result<Response> {
    let src = r.h("x-amz-copy-source").unwrap_or_default();
    let (sb, sk, svid) = parse_copy_source(&src)?;
    let svc = r.svc(state);
    let (sinfo, content) = svc.get_object_full(&sb, &sk, svid.as_deref()).await?;
    check_conditionals(
        &r.headers,
        "x-amz-copy-source-",
        &sinfo.etag,
        &sinfo.created_at,
    )?;
    let size = content.len() as u64;
    let data = match r.h("x-amz-copy-source-range") {
        None => content,
        Some(rg) => {
            let bad = || {
                S3Error::invalid_argument(format!(
                    "Range specified is not valid for source object of size: {size}"
                ))
                .with("ArgumentName", "x-amz-copy-source-range")
                .with("ArgumentValue", rg.clone())
            };
            let spec = rg.trim().strip_prefix("bytes=").ok_or_else(bad)?;
            let (a, b) = spec.split_once('-').ok_or_else(bad)?;
            let (a, b): (u64, u64) = (
                a.trim().parse().map_err(|_| bad())?,
                b.trim().parse().map_err(|_| bad())?,
            );
            if a > b || b >= size {
                return Err(bad());
            }
            content[a as usize..=b as usize].to_vec()
        }
    };
    let etag = svc
        .upload_part(&r.bucket, r.key(), id, num, data, ObjMeta::default())
        .await?;
    emit_json(
        state,
        "upload_part_copy",
        json!({"bucket": r.bucket, "key": r.key(), "upload_id": id, "part_number": num}),
    )
    .await;
    let mut h = sse_headers(up_meta);
    if svid.is_some() || sinfo.version_id != "null" {
        h.push((
            "x-amz-copy-source-version-id".into(),
            sinfo.version_id.clone(),
        ));
    }
    Ok(with_headers(
        xml_ok(format!(
            "{XML_DECL}<CopyPartResult xmlns=\"{XMLNS}\"><LastModified>{}</LastModified>{}</CopyPartResult>",
            iso(&super::service::now_iso()),
            tag("ETag", &etag)
        )),
        &h,
    ))
}

pub async fn complete(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let id = upload_id(r)?;
    let doc = crate::xml::parse(&r.body).map_err(|_| S3Error::malformed_xml())?;
    if doc.name != "CompleteMultipartUpload" {
        return Err(S3Error::malformed_xml());
    }
    let mut parts = Vec::new();
    for p in doc.children_named("Part") {
        let num: i64 = p
            .child_text("PartNumber")
            .and_then(|n| n.parse().ok())
            .ok_or_else(S3Error::malformed_xml)?;
        let etag = p.child_text("ETag").ok_or_else(S3Error::malformed_xml)?;
        parts.push((num, etag));
    }
    let wr = r
        .svc(state)
        .complete_upload(&r.bucket, r.key(), id, parts)
        .await?;
    emit_json(
        state,
        "complete_multipart_upload",
        json!({"bucket": r.bucket, "key": r.key(), "upload_id": id, "size": wr.size}),
    )
    .await;
    fire_created(
        state,
        &r.bucket,
        r.key(),
        "ObjectCreated:CompleteMultipartUpload",
        &wr,
        &r.request_id,
    )
    .await;
    let mut h = write_headers(&wr);
    h.retain(|(k, _)| k != "etag");
    let location = format!(
        "{}/{}/{}",
        state.cfg.base_url.trim_end_matches('/'),
        r.bucket,
        url_encode(r.key())
    );
    Ok(with_headers(
        xml_ok(format!(
            "{XML_DECL}<CompleteMultipartUploadResult xmlns=\"{XMLNS}\">{}{}{}{}</CompleteMultipartUploadResult>",
            tag("Location", &location),
            tag("Bucket", &r.bucket),
            tag("Key", r.key()),
            tag("ETag", &wr.etag)
        )),
        &h,
    ))
}

pub async fn abort(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let id = upload_id(r)?;
    r.svc(state).abort_upload(&r.bucket, r.key(), id).await?;
    emit_json(
        state,
        "abort_multipart_upload",
        json!({"bucket": r.bucket, "key": r.key(), "upload_id": id}),
    )
    .await;
    Ok(empty(StatusCode::NO_CONTENT))
}

pub async fn list_parts(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let id = upload_id(r)?;
    let (up, parts) = r.svc(state).list_parts(&r.bucket, r.key(), id).await?;
    let marker: i64 = r
        .q("part-number-marker")
        .and_then(|m| m.parse().ok())
        .unwrap_or(0);
    let max: usize = r
        .q("max-parts")
        .and_then(|m| m.parse().ok())
        .unwrap_or(1000)
        .min(1000);
    let mut parts: Vec<_> = parts
        .into_iter()
        .filter(|p| p.part_number > marker)
        .collect();
    let truncated = parts.len() > max;
    parts.truncate(max);
    let mut x = format!("{XML_DECL}<ListPartsResult xmlns=\"{XMLNS}\">");
    x.push_str(&tag("Bucket", &r.bucket));
    x.push_str(&tag("Key", r.key()));
    x.push_str(&tag("UploadId", &up.upload_id));
    x.push_str("<Initiator><ID>cloudtwin</ID><DisplayName>cloudtwin</DisplayName></Initiator>");
    x.push_str("<Owner><ID>cloudtwin</ID><DisplayName>cloudtwin</DisplayName></Owner>");
    x.push_str(&tag("StorageClass", up.meta.storage_class()));
    x.push_str(&format!("<PartNumberMarker>{marker}</PartNumberMarker>"));
    if let Some(last) = parts.last() {
        x.push_str(&format!(
            "<NextPartNumberMarker>{}</NextPartNumberMarker>",
            last.part_number
        ));
    }
    x.push_str(&format!(
        "<MaxParts>{max}</MaxParts><IsTruncated>{truncated}</IsTruncated>"
    ));
    for p in &parts {
        x.push_str("<Part>");
        x.push_str(&format!("<PartNumber>{}</PartNumber>", p.part_number));
        x.push_str(&tag("LastModified", &iso(&p.created_at)));
        x.push_str(&tag("ETag", &p.etag));
        x.push_str(&format!("<Size>{}</Size>", p.size));
        for (k, v) in &p.meta.checksums {
            let Some(name) = super::object::checksum_element(k) else {
                continue;
            };
            x.push_str(&tag(name, v));
        }
        x.push_str("</Part>");
    }
    x.push_str("</ListPartsResult>");
    Ok(xml_ok(x))
}

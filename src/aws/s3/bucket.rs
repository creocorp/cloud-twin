//! Bucket-level S3 operations (`/:bucket` with optional sub-resources).

use std::sync::Arc;

use axum::{
    http::{header, Method, StatusCode},
    response::{IntoResponse, Response},
};
use base64::Engine;
use serde_json::json;

use super::cors;
use super::errors::{S3Error, S3Result};
use super::handlers::{add_headers, emit_json, empty, xml_ok, S3Req};
use super::notify;
use super::service::ListParams;
use super::xmlutil::{esc, iso, parse_tagging, tag, url_encode, XMLNS, XML_DECL};
use crate::AppState;

/// A bucket sub-resource stored verbatim in `s3_bucket_configs`.
struct ConfigKind {
    param: &'static str,
    /// Expected XML root element; empty for JSON (bucket policy).
    root: &'static str,
    not_found: &'static str,
    message: &'static str,
}

const CONFIGS: &[ConfigKind] = &[
    ConfigKind {
        param: "tagging",
        root: "Tagging",
        not_found: "NoSuchTagSet",
        message: "The TagSet does not exist",
    },
    ConfigKind {
        param: "cors",
        root: "CORSConfiguration",
        not_found: "NoSuchCORSConfiguration",
        message: "The CORS configuration does not exist",
    },
    ConfigKind {
        param: "lifecycle",
        root: "LifecycleConfiguration",
        not_found: "NoSuchLifecycleConfiguration",
        message: "The lifecycle configuration does not exist",
    },
    ConfigKind {
        param: "encryption",
        root: "ServerSideEncryptionConfiguration",
        not_found: "ServerSideEncryptionConfigurationNotFoundError",
        message: "The server side encryption configuration was not found",
    },
    ConfigKind {
        param: "publicAccessBlock",
        root: "PublicAccessBlockConfiguration",
        not_found: "NoSuchPublicAccessBlockConfiguration",
        message: "The public access block configuration was not found",
    },
    ConfigKind {
        param: "ownershipControls",
        root: "OwnershipControls",
        not_found: "OwnershipControlsNotFoundError",
        message: "The bucket ownership controls were not found",
    },
    ConfigKind {
        param: "website",
        root: "WebsiteConfiguration",
        not_found: "NoSuchWebsiteConfiguration",
        message: "The specified bucket does not have a website configuration",
    },
    ConfigKind {
        param: "replication",
        root: "ReplicationConfiguration",
        not_found: "ReplicationConfigurationNotFoundError",
        message: "The replication configuration was not found",
    },
    ConfigKind {
        param: "object-lock",
        root: "ObjectLockConfiguration",
        not_found: "ObjectLockConfigurationNotFoundError",
        message: "Object Lock configuration does not exist for this bucket",
    },
    ConfigKind {
        param: "policy",
        root: "",
        not_found: "NoSuchBucketPolicy",
        message: "The bucket policy does not exist",
    },
];

fn method_not_allowed() -> S3Error {
    S3Error::new(
        "MethodNotAllowed",
        "The specified method is not allowed against this resource.",
    )
}

pub async fn dispatch(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let m = &r.method;
    if let Some(c) = CONFIGS.iter().find(|c| r.has(c.param)) {
        return config_op(state, r, c).await;
    }
    match (m.clone(), r) {
        (Method::GET, r) if r.has("versioning") => get_versioning(state, r).await,
        (Method::PUT, r) if r.has("versioning") => put_versioning(state, r).await,
        (Method::GET, r) if r.has("notification") => {
            let cfg = r.svc(state).get_config(&r.bucket, "notification").await?;
            Ok(xml_ok(cfg.unwrap_or_else(notify::empty_xml)))
        }
        (Method::PUT, r) if r.has("notification") => {
            let skip = r
                .h("x-amz-skip-destination-validation")
                .is_some_and(|v| v == "true");
            notify::put(state, &r.bucket, &r.body, &r.request_id, skip).await?;
            emit_json(
                state,
                "put_bucket_notification",
                json!({"bucket": r.bucket}),
            )
            .await;
            Ok(empty(StatusCode::OK))
        }
        (Method::GET, r) if r.has("acl") => {
            r.svc(state).get_config(&r.bucket, "acl").await?;
            Ok(xml_ok(acl_xml()))
        }
        (Method::PUT, r) if r.has("acl") => {
            r.svc(state).get_config(&r.bucket, "acl").await?;
            Ok(empty(StatusCode::OK))
        }
        (Method::GET, r) if r.has("location") => {
            let loc = r.svc(state).get_config(&r.bucket, "location").await?;
            Ok(xml_ok(
                match loc.filter(|l| !l.is_empty() && l != "us-east-1") {
                    Some(l) => format!(
                        "{XML_DECL}<LocationConstraint xmlns=\"{XMLNS}\">{}</LocationConstraint>",
                        esc(&l)
                    ),
                    None => format!("{XML_DECL}<LocationConstraint xmlns=\"{XMLNS}\"/>"),
                },
            ))
        }
        (Method::GET, r) if r.has("versions") => list_versions(state, r).await,
        (Method::GET, r) if r.has("uploads") => list_uploads(state, r).await,
        (Method::POST, r) if r.has("delete") => delete_objects(state, r).await,
        (Method::GET, r) if r.has("logging") => {
            r.svc(state).get_config(&r.bucket, "logging").await?;
            Ok(xml_ok(format!(
                "{XML_DECL}<BucketLoggingStatus xmlns=\"{XMLNS}\"/>"
            )))
        }
        (Method::GET, r) if r.has("requestPayment") => {
            r.svc(state).get_config(&r.bucket, "requestPayment").await?;
            Ok(xml_ok(format!(
                "{XML_DECL}<RequestPaymentConfiguration xmlns=\"{XMLNS}\"><Payer>BucketOwner</Payer></RequestPaymentConfiguration>"
            )))
        }
        (Method::GET, r) if r.has("accelerate") => {
            r.svc(state).get_config(&r.bucket, "accelerate").await?;
            Ok(xml_ok(format!(
                "{XML_DECL}<AccelerateConfiguration xmlns=\"{XMLNS}\"/>"
            )))
        }
        (Method::GET, r) if r.has("policyStatus") => {
            r.svc(state).get_config(&r.bucket, "policyStatus").await?;
            Ok(xml_ok(format!(
                "{XML_DECL}<PolicyStatus xmlns=\"{XMLNS}\"><IsPublic>false</IsPublic></PolicyStatus>"
            )))
        }
        (Method::PUT, r) if r.has("logging") || r.has("requestPayment") || r.has("accelerate") => {
            r.svc(state).get_config(&r.bucket, "logging").await?;
            Ok(empty(StatusCode::OK))
        }
        (Method::GET, r) => list_objects(state, r).await,
        (Method::PUT, r) => create_bucket(state, r).await,
        (Method::DELETE, r) => {
            r.svc(state).delete_bucket(&r.bucket).await?;
            emit_json(state, "delete_bucket", json!({"bucket": r.bucket})).await;
            Ok(empty(StatusCode::NO_CONTENT))
        }
        (Method::HEAD, r) => {
            if !r.svc(state).bucket_exists(&r.bucket).await? {
                return Err(S3Error::no_such_bucket(&r.bucket));
            }
            let mut resp = empty(StatusCode::OK);
            add_headers(
                &mut resp,
                &[
                    ("x-amz-bucket-region".into(), crate::aws::REGION.into()),
                    ("x-amz-access-point-alias".into(), "false".into()),
                ],
            );
            Ok(resp)
        }
        _ => Err(method_not_allowed()),
    }
}

pub fn acl_xml() -> String {
    format!(
        "{XML_DECL}<AccessControlPolicy xmlns=\"{XMLNS}\"><Owner><ID>cloudtwin</ID><DisplayName>cloudtwin</DisplayName></Owner>\
<AccessControlList><Grant><Grantee xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"CanonicalUser\">\
<ID>cloudtwin</ID><DisplayName>cloudtwin</DisplayName></Grantee><Permission>FULL_CONTROL</Permission></Grant></AccessControlList></AccessControlPolicy>"
    )
}

async fn create_bucket(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let mut location = None;
    if !r.body.iter().all(|b| b.is_ascii_whitespace()) {
        let doc = crate::xml::parse(&r.body).map_err(|_| S3Error::malformed_xml())?;
        location = doc
            .child_text("LocationConstraint")
            .filter(|l| !l.is_empty());
    }
    let svc = r.svc(state);
    if svc.create_bucket(&r.bucket).await? {
        if let Some(l) = location {
            svc.put_config(&r.bucket, "location", &l).await?;
        }
        emit_json(state, "create_bucket", json!({"bucket": r.bucket})).await;
    }
    Ok((
        StatusCode::OK,
        [(header::LOCATION, format!("/{}", r.bucket))],
    )
        .into_response())
}

/// Validate a sub-resource document before storing it.
fn validate_config(c: &ConfigKind, body: &[u8]) -> S3Result<()> {
    if c.root.is_empty() {
        return serde_json::from_slice::<serde_json::Value>(body)
            .map(|_| ())
            .map_err(|_| {
                S3Error::new(
                    "MalformedPolicy",
                    "Policies must be valid JSON and the first byte must be '{'",
                )
            });
    }
    let doc = crate::xml::parse(body).map_err(|_| S3Error::malformed_xml())?;
    if doc.name != c.root {
        return Err(S3Error::malformed_xml());
    }
    match c.param {
        "cors" => {
            cors::parse_rules(body)?;
        }
        "tagging" => {
            parse_tagging(body)?;
        }
        "encryption" => {
            let rule = doc.child("Rule").ok_or_else(S3Error::malformed_xml)?;
            if let Some(def) = rule.child("ApplyServerSideEncryptionByDefault") {
                let algo = def.child_text("SSEAlgorithm").unwrap_or_default();
                if !["AES256", "aws:kms", "aws:kms:dsse"].contains(&algo.as_str()) {
                    return Err(S3Error::malformed_xml());
                }
            }
        }
        "lifecycle" => {
            if doc.children_named("Rule").next().is_none() {
                return Err(S3Error::malformed_xml());
            }
            for rule in doc.children_named("Rule") {
                let status = rule.child_text("Status").unwrap_or_default();
                if status != "Enabled" && status != "Disabled" {
                    return Err(S3Error::malformed_xml());
                }
            }
        }
        _ => {}
    }
    Ok(())
}

async fn config_op(state: &Arc<AppState>, r: &S3Req, c: &ConfigKind) -> S3Result<Response> {
    let svc = r.svc(state);
    match r.method {
        Method::GET => {
            let body = svc.get_config(&r.bucket, c.param).await?.ok_or_else(|| {
                S3Error::new(c.not_found, c.message).with("BucketName", r.bucket.clone())
            })?;
            if c.root.is_empty() {
                Ok((
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/json")],
                    body,
                )
                    .into_response())
            } else {
                Ok(xml_ok(body))
            }
        }
        Method::PUT => {
            svc.get_config(&r.bucket, c.param).await?;
            validate_config(c, &r.body)?;
            svc.put_config(&r.bucket, c.param, &String::from_utf8_lossy(&r.body))
                .await?;
            emit_json(
                state,
                "put_bucket_config",
                json!({"bucket": r.bucket, "kind": c.param}),
            )
            .await;
            Ok(empty(if c.param == "policy" || c.param == "tagging" {
                StatusCode::NO_CONTENT
            } else {
                StatusCode::OK
            }))
        }
        Method::DELETE => {
            svc.delete_config(&r.bucket, c.param).await?;
            emit_json(
                state,
                "delete_bucket_config",
                json!({"bucket": r.bucket, "kind": c.param}),
            )
            .await;
            Ok(empty(StatusCode::NO_CONTENT))
        }
        _ => Err(method_not_allowed()),
    }
}

async fn get_versioning(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let status = r.svc(state).get_config(&r.bucket, "versioning").await?;
    Ok(xml_ok(match status {
        Some(s) => format!(
            "{XML_DECL}<VersioningConfiguration xmlns=\"{XMLNS}\"><Status>{}</Status></VersioningConfiguration>",
            esc(&s)
        ),
        None => format!("{XML_DECL}<VersioningConfiguration xmlns=\"{XMLNS}\"/>"),
    }))
}

async fn put_versioning(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let doc = crate::xml::parse(&r.body).map_err(|_| S3Error::malformed_xml())?;
    if doc.name != "VersioningConfiguration" {
        return Err(S3Error::malformed_xml());
    }
    let status = doc.child_text("Status").unwrap_or_default();
    if status != "Enabled" && status != "Suspended" {
        return Err(S3Error::malformed_xml());
    }
    r.svc(state)
        .put_config(&r.bucket, "versioning", &status)
        .await?;
    emit_json(
        state,
        "put_bucket_versioning",
        json!({"bucket": r.bucket, "status": status}),
    )
    .await;
    Ok(empty(StatusCode::OK))
}

fn max_keys(r: &S3Req, name: &str) -> S3Result<usize> {
    match r.q(name) {
        None => Ok(1000),
        Some(v) => v
            .parse::<i64>()
            .ok()
            .filter(|n| *n >= 0)
            .map(|n| n.min(1000) as usize)
            .ok_or_else(|| {
                S3Error::invalid_argument(
                    "Provided max-keys not an integer or within integer range",
                )
                .with("ArgumentName", name)
                .with("ArgumentValue", v)
            }),
    }
}

fn encoding(r: &S3Req) -> S3Result<bool> {
    match r.q("encoding-type") {
        None => Ok(false),
        Some(v) if v.eq_ignore_ascii_case("url") => Ok(true),
        Some(v) => Err(
            S3Error::invalid_argument("Invalid Encoding Method specified in Request")
                .with("ArgumentName", "encoding-type")
                .with("ArgumentValue", v),
        ),
    }
}

fn owner_xml() -> &'static str {
    "<Owner><ID>cloudtwin</ID><DisplayName>cloudtwin</DisplayName></Owner>"
}

async fn list_objects(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let v2 = r.q("list-type") == Some("2");
    let enc = encoding(r)?;
    let e = |s: &str| if enc { url_encode(s) } else { s.to_string() };
    let prefix = r.q("prefix").unwrap_or("").to_string();
    let delimiter = r.q("delimiter").filter(|d| !d.is_empty()).map(String::from);
    let max = max_keys(r, "max-keys")?;
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let token = r.q("continuation-token");
    let after = if v2 {
        match token {
            Some(t) => Some(
                b64.decode(t)
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok())
                    .ok_or_else(|| {
                        S3Error::invalid_argument("The continuation token provided is incorrect")
                            .with("ArgumentName", "continuation-token")
                    })?,
            ),
            None => r
                .q("start-after")
                .filter(|s| !s.is_empty())
                .map(String::from),
        }
    } else {
        r.q("marker").filter(|s| !s.is_empty()).map(String::from)
    };
    let page = r
        .svc(state)
        .list_objects(
            &r.bucket,
            ListParams {
                prefix: prefix.clone(),
                delimiter: delimiter.clone(),
                after,
                max_keys: max,
            },
        )
        .await?;
    let with_owner = !v2 || r.q("fetch-owner") == Some("true");
    let mut x = format!("{XML_DECL}<ListBucketResult xmlns=\"{XMLNS}\">");
    x.push_str(&tag("Name", &r.bucket));
    x.push_str(&tag("Prefix", &e(&prefix)));
    if v2 {
        if let Some(t) = token {
            x.push_str(&tag("ContinuationToken", t));
        }
        if let Some(sa) = r.q("start-after") {
            x.push_str(&tag("StartAfter", &e(sa)));
        }
        x.push_str(&format!(
            "<KeyCount>{}</KeyCount>",
            page.contents.len() + page.common_prefixes.len()
        ));
    } else {
        x.push_str(&tag("Marker", &e(r.q("marker").unwrap_or(""))));
    }
    x.push_str(&format!("<MaxKeys>{max}</MaxKeys>"));
    if let Some(d) = &delimiter {
        x.push_str(&tag("Delimiter", &e(d)));
    }
    if enc {
        x.push_str("<EncodingType>url</EncodingType>");
    }
    x.push_str(&format!("<IsTruncated>{}</IsTruncated>", page.truncated));
    if page.truncated {
        if let Some(last) = &page.last {
            if v2 {
                x.push_str(&tag("NextContinuationToken", &b64.encode(last)));
            } else if delimiter.is_some() {
                x.push_str(&tag("NextMarker", &e(last)));
            }
        }
    }
    for o in &page.contents {
        x.push_str("<Contents>");
        x.push_str(&tag("Key", &e(&o.key)));
        x.push_str(&tag("LastModified", &iso(&o.created_at)));
        x.push_str(&tag("ETag", &o.etag));
        x.push_str(&format!("<Size>{}</Size>", o.size));
        if with_owner {
            x.push_str(owner_xml());
        }
        x.push_str(&tag("StorageClass", &o.storage_class));
        x.push_str("</Contents>");
    }
    for cp in &page.common_prefixes {
        x.push_str(&format!(
            "<CommonPrefixes>{}</CommonPrefixes>",
            tag("Prefix", &e(cp))
        ));
    }
    x.push_str("</ListBucketResult>");
    Ok(xml_ok(x))
}

async fn list_versions(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let enc = encoding(r)?;
    let e = |s: &str| if enc { url_encode(s) } else { s.to_string() };
    let prefix = r.q("prefix").unwrap_or("").to_string();
    let delimiter = r.q("delimiter").filter(|d| !d.is_empty()).map(String::from);
    let max = max_keys(r, "max-keys")?;
    let key_marker = r.q("key-marker").unwrap_or("").to_string();
    let vid_marker = r.q("version-id-marker").unwrap_or("").to_string();
    let page = r
        .svc(state)
        .list_versions(
            &r.bucket,
            ListParams {
                prefix: prefix.clone(),
                delimiter: delimiter.clone(),
                after: Some(key_marker.clone()),
                max_keys: max,
            },
            Some(vid_marker.clone()),
        )
        .await?;
    let mut x = format!("{XML_DECL}<ListVersionsResult xmlns=\"{XMLNS}\">");
    x.push_str(&tag("Name", &r.bucket));
    x.push_str(&tag("Prefix", &e(&prefix)));
    x.push_str(&tag("KeyMarker", &e(&key_marker)));
    x.push_str(&tag("VersionIdMarker", &vid_marker));
    if let Some(k) = &page.next_key_marker {
        x.push_str(&tag("NextKeyMarker", &e(k)));
    }
    if let Some(v) = &page.next_version_marker {
        x.push_str(&tag("NextVersionIdMarker", v));
    }
    x.push_str(&format!("<MaxKeys>{max}</MaxKeys>"));
    if let Some(d) = &delimiter {
        x.push_str(&tag("Delimiter", &e(d)));
    }
    if enc {
        x.push_str("<EncodingType>url</EncodingType>");
    }
    x.push_str(&format!("<IsTruncated>{}</IsTruncated>", page.truncated));
    for v in &page.entries {
        let el = if v.is_delete_marker {
            "DeleteMarker"
        } else {
            "Version"
        };
        x.push_str(&format!("<{el}>"));
        x.push_str(&tag("Key", &e(&v.key)));
        x.push_str(&tag("VersionId", &v.version_id));
        x.push_str(&format!("<IsLatest>{}</IsLatest>", v.is_latest));
        x.push_str(&tag("LastModified", &iso(&v.created_at)));
        if !v.is_delete_marker {
            x.push_str(&tag("ETag", &v.etag));
            x.push_str(&format!("<Size>{}</Size>", v.size));
            x.push_str(&tag("StorageClass", &v.storage_class));
        }
        x.push_str(owner_xml());
        x.push_str(&format!("</{el}>"));
    }
    for cp in &page.common_prefixes {
        x.push_str(&format!(
            "<CommonPrefixes>{}</CommonPrefixes>",
            tag("Prefix", &e(cp))
        ));
    }
    x.push_str("</ListVersionsResult>");
    Ok(xml_ok(x))
}

async fn list_uploads(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let enc = encoding(r)?;
    let e = |s: &str| if enc { url_encode(s) } else { s.to_string() };
    let prefix = r.q("prefix").unwrap_or("");
    let max = max_keys(r, "max-uploads")?;
    let key_marker = r.q("key-marker").unwrap_or("");
    let id_marker = r.q("upload-id-marker").unwrap_or("");
    let mut uploads: Vec<_> = r
        .svc(state)
        .list_uploads(&r.bucket, prefix)
        .await?
        .into_iter()
        .filter(|u| {
            key_marker.is_empty()
                || u.key.as_str() > key_marker
                || (!id_marker.is_empty()
                    && u.key == key_marker
                    && u.upload_id.as_str() > id_marker)
        })
        .collect();
    let truncated = uploads.len() > max;
    uploads.truncate(max);
    let mut x = format!("{XML_DECL}<ListMultipartUploadsResult xmlns=\"{XMLNS}\">");
    x.push_str(&tag("Bucket", &r.bucket));
    x.push_str(&tag("KeyMarker", &e(key_marker)));
    x.push_str(&tag("UploadIdMarker", id_marker));
    if truncated {
        if let Some(last) = uploads.last() {
            x.push_str(&tag("NextKeyMarker", &e(&last.key)));
            x.push_str(&tag("NextUploadIdMarker", &last.upload_id));
        }
    }
    x.push_str(&tag("Prefix", &e(prefix)));
    x.push_str(&format!(
        "<MaxUploads>{max}</MaxUploads><IsTruncated>{truncated}</IsTruncated>"
    ));
    if enc {
        x.push_str("<EncodingType>url</EncodingType>");
    }
    for u in &uploads {
        x.push_str("<Upload>");
        x.push_str(&tag("Key", &e(&u.key)));
        x.push_str(&tag("UploadId", &u.upload_id));
        x.push_str("<Initiator><ID>cloudtwin</ID><DisplayName>cloudtwin</DisplayName></Initiator>");
        x.push_str(owner_xml());
        x.push_str(&tag("StorageClass", u.meta.storage_class()));
        x.push_str(&tag("Initiated", &iso(&u.created_at)));
        x.push_str("</Upload>");
    }
    x.push_str("</ListMultipartUploadsResult>");
    Ok(xml_ok(x))
}

async fn delete_objects(state: &Arc<AppState>, r: &S3Req) -> S3Result<Response> {
    let doc = crate::xml::parse(&r.body).map_err(|_| S3Error::malformed_xml())?;
    if doc.name != "Delete" {
        return Err(S3Error::malformed_xml());
    }
    let quiet = doc
        .child_text("Quiet")
        .is_some_and(|q| q.eq_ignore_ascii_case("true"));
    let objects: Vec<(String, Option<String>)> = doc
        .children_named("Object")
        .map(|o| {
            (
                o.child("Key").map(|k| k.text.clone()).unwrap_or_default(),
                o.child_text("VersionId").filter(|v| !v.is_empty()),
            )
        })
        .collect();
    if objects.is_empty() || objects.len() > 1000 {
        return Err(S3Error::malformed_xml());
    }
    let svc = r.svc(state);
    if !svc.bucket_exists(&r.bucket).await? {
        return Err(S3Error::no_such_bucket(&r.bucket));
    }
    let mut x = format!("{XML_DECL}<DeleteResult xmlns=\"{XMLNS}\">");
    for (key, vid) in &objects {
        match svc.delete_object(&r.bucket, key, vid.as_deref()).await {
            Ok(out) => {
                super::object::after_delete(state, r, key, vid.as_deref(), &out).await;
                if quiet {
                    continue;
                }
                x.push_str("<Deleted>");
                x.push_str(&tag("Key", key));
                if let Some(v) = vid {
                    x.push_str(&tag("VersionId", v));
                }
                if out.delete_marker {
                    x.push_str("<DeleteMarker>true</DeleteMarker>");
                    if let Some(v) = &out.version_id {
                        x.push_str(&tag("DeleteMarkerVersionId", v));
                    }
                }
                x.push_str("</Deleted>");
            }
            Err(err) => {
                x.push_str("<Error>");
                x.push_str(&tag("Key", key));
                if let Some(v) = vid {
                    x.push_str(&tag("VersionId", v));
                }
                x.push_str(&tag("Code", err.code));
                x.push_str(&tag("Message", &err.message));
                x.push_str("</Error>");
            }
        }
    }
    x.push_str("</DeleteResult>");
    Ok(xml_ok(x))
}

//! S3 REST handlers.
//!
//! S3 is the AWS service here that behaves most like a normal REST API, so this
//! file looks more like an Axum router than the AWS Query/JSON handler files.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, put},
    Router,
};

use super::service::S3Service;
use crate::telemetry;
use crate::AppState;

// ─────────────────────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────────────────────

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_buckets))
        .route(
            "/:bucket",
            put(create_bucket)
                .delete(delete_bucket)
                .get(list_objects)
                .head(bucket_head),
        )
        .route(
            "/:bucket/*key",
            put(put_object)
                .get(get_object)
                .head(head_object)
                .delete(delete_object),
        )
}

// ─────────────────────────────────────────────────────────────────────────────
// XML helpers
// ─────────────────────────────────────────────────────────────────────────────

fn xml_response(status: StatusCode, body: String) -> Response {
    (status, [(header::CONTENT_TYPE, "application/xml")], body).into_response()
}

fn xml_ok(body: String) -> Response {
    xml_response(StatusCode::OK, body)
}

fn xml_error(status: StatusCode, code: &str, message: &str, extra: &str) -> Response {
    xml_response(
        status,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Error>
  <Code>{code}</Code>
  <Message>{message}</Message>
  {extra}
  <RequestId>cloudtwin-lite</RequestId>
</Error>"#,
            code = xml_escape(code),
            message = xml_escape(message),
            extra = extra,
        ),
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn svc(state: &Arc<AppState>) -> S3Service {
    S3Service::new(state.db.clone())
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers — Buckets
// ─────────────────────────────────────────────────────────────────────────────

async fn list_buckets(State(state): State<Arc<AppState>>) -> Response {
    match svc(&state).list_buckets().await {
        Ok(buckets) => {
            let bucket_xml: String = buckets
                .iter()
                .map(|b| {
                    format!(
                        "    <Bucket><Name>{}</Name><CreationDate>{}</CreationDate></Bucket>\n",
                        xml_escape(&b.name),
                        xml_escape(&b.created_at),
                    )
                })
                .collect();

            xml_ok(format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Owner><ID>cloudtwin</ID><DisplayName>cloudtwin</DisplayName></Owner>
  <Buckets>
{bucket_xml}  </Buckets>
</ListAllMyBucketsResult>"#
            ))
        }
        Err(e) => xml_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "InternalError",
            &e.to_string(),
            "",
        ),
    }
}

async fn create_bucket(State(state): State<Arc<AppState>>, Path(bucket): Path<String>) -> Response {
    match svc(&state).create_bucket(&bucket).await {
        Ok(_) => {
            telemetry::emit(&state.db, "aws", "s3", "create_bucket", &format!("{{\"bucket\":\"{bucket}\"}}")).await;
            (StatusCode::OK, [("Location", format!("/{bucket}"))]).into_response()
        }
        Err(e) => xml_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "InternalError",
            &e.to_string(),
            "",
        ),
    }
}

async fn delete_bucket(State(state): State<Arc<AppState>>, Path(bucket): Path<String>) -> Response {
    match svc(&state).delete_bucket(&bucket).await {
        Ok(_) => {
            telemetry::emit(&state.db, "aws", "s3", "delete_bucket", &format!("{{\"bucket\":\"{bucket}\"}}")).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) if e.to_string().contains("NoSuchBucket") => xml_error(
            StatusCode::NOT_FOUND,
            "NoSuchBucket",
            "The specified bucket does not exist.",
            &format!("<BucketName>{}</BucketName>", xml_escape(&bucket)),
        ),
        Err(e) => xml_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "InternalError",
            &e.to_string(),
            "",
        ),
    }
}

/// HEAD /:bucket — used by some SDKs to check bucket existence.
async fn bucket_head(State(state): State<Arc<AppState>>, Path(bucket): Path<String>) -> StatusCode {
    match svc(&state).bucket_exists(&bucket).await {
        Ok(true) => StatusCode::OK,
        Ok(false) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers — Objects
// ─────────────────────────────────────────────────────────────────────────────

async fn list_objects(
    State(state): State<Arc<AppState>>,
    Path(bucket): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let prefix = params.get("prefix").map(|s| s.as_str()).unwrap_or("");
    let max_keys: i64 = params
        .get("max-keys")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);

    match svc(&state)
        .list_objects(&bucket, Some(prefix), max_keys)
        .await
    {
        Ok(objects) => {
            let key_count = objects.len();
            let contents: String = objects
                .iter()
                .map(|o| {
                    format!(
                        "  <Contents>\
<Key>{}</Key>\
<LastModified>{}</LastModified>\
<ETag>{}</ETag>\
<Size>{}</Size>\
<StorageClass>STANDARD</StorageClass></Contents>\n",
                        xml_escape(&o.key),
                        xml_escape(&o.created_at),
                        xml_escape(&o.etag),
                        o.size,
                    )
                })
                .collect();

            xml_ok(format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Name>{bucket}</Name>
  <Prefix>{prefix}</Prefix>
  <KeyCount>{key_count}</KeyCount>
  <MaxKeys>{max_keys}</MaxKeys>
  <IsTruncated>false</IsTruncated>
{contents}</ListBucketResult>"#,
                bucket = xml_escape(&bucket),
                prefix = xml_escape(prefix),
            ))
        }
        Err(e) if e.to_string().contains("NoSuchBucket") => xml_error(
            StatusCode::NOT_FOUND,
            "NoSuchBucket",
            "The specified bucket does not exist.",
            &format!("<BucketName>{}</BucketName>", xml_escape(&bucket)),
        ),
        Err(e) => xml_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "InternalError",
            &e.to_string(),
            "",
        ),
    }
}

async fn put_object(
    State(state): State<Arc<AppState>>,
    Path((bucket, key)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    match svc(&state)
        .put_object(&bucket, &key, body.to_vec(), &content_type)
        .await
    {
        Ok(etag) => {
            telemetry::emit(&state.db, "aws", "s3", "put_object", &format!("{{\"bucket\":\"{bucket}\",\"key\":\"{key}\"}}")).await;
            (StatusCode::OK, [(header::ETAG, etag)]).into_response()
        }
        Err(e) if e.to_string().contains("NoSuchBucket") => xml_error(
            StatusCode::NOT_FOUND,
            "NoSuchBucket",
            "The specified bucket does not exist.",
            &format!("<BucketName>{}</BucketName>", xml_escape(&bucket)),
        ),
        Err(e) => xml_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "InternalError",
            &e.to_string(),
            "",
        ),
    }
}

async fn get_object(
    State(state): State<Arc<AppState>>,
    Path((bucket, key)): Path<(String, String)>,
) -> Response {
    match svc(&state).get_object(&bucket, &key).await {
        Ok(obj) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE.as_str(), obj.content_type.as_str()),
                (header::ETAG.as_str(), obj.etag.as_str()),
                ("x-amz-request-id", "cloudtwin-lite"),
            ],
            obj.content,
        )
            .into_response(),
        Err(e) if e.to_string().contains("NoSuchKey") => xml_error(
            StatusCode::NOT_FOUND,
            "NoSuchKey",
            "The specified key does not exist.",
            &format!("<Key>{}</Key>", xml_escape(&key)),
        ),
        Err(e) if e.to_string().contains("NoSuchBucket") => xml_error(
            StatusCode::NOT_FOUND,
            "NoSuchBucket",
            "The specified bucket does not exist.",
            &format!("<BucketName>{}</BucketName>", xml_escape(&bucket)),
        ),
        Err(e) => xml_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "InternalError",
            &e.to_string(),
            "",
        ),
    }
}

async fn head_object(
    State(state): State<Arc<AppState>>,
    Path((bucket, key)): Path<(String, String)>,
) -> Response {
    match svc(&state).get_object(&bucket, &key).await {
        Ok(obj) => {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                obj.content_type
                    .parse()
                    .unwrap_or_else(|_| "application/octet-stream".parse().unwrap()),
            );
            headers.insert(
                header::CONTENT_LENGTH,
                obj.size.to_string().parse().unwrap(),
            );
            headers.insert(
                header::ETAG,
                obj.etag.parse().unwrap_or_else(|_| "".parse().unwrap()),
            );
            (StatusCode::OK, headers).into_response()
        }
        Err(e) if e.to_string().contains("NoSuchKey") => StatusCode::NOT_FOUND.into_response(),
        Err(e) if e.to_string().contains("NoSuchBucket") => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn delete_object(
    State(state): State<Arc<AppState>>,
    Path((bucket, key)): Path<(String, String)>,
) -> StatusCode {
    match svc(&state).delete_object(&bucket, &key).await {
        Ok(_) => {
            telemetry::emit(&state.db, "aws", "s3", "delete_object", &format!("{{\"bucket\":\"{bucket}\",\"key\":\"{key}\"}}")).await;
            StatusCode::NO_CONTENT
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

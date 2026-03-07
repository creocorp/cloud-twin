//! GCP Cloud Storage HTTP handlers.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};

use crate::AppState;
use super::service::StorageService;

fn svc(state: &Arc<AppState>) -> StorageService {
    StorageService::new(state.db.clone(), state.cfg.gcp_project.clone())
}

fn bucket_to_json(b: &super::models::GcsBucket) -> serde_json::Value {
    serde_json::json!({
        "kind":        "storage#bucket",
        "id":          b.name,
        "name":        b.name,
        "location":    b.location,
        "timeCreated": b.created_at,
    })
}

fn object_to_json(o: &super::models::GcsObject) -> serde_json::Value {
    serde_json::json!({
        "kind":        "storage#object",
        "bucket":      o.bucket,
        "name":        o.name,
        "contentType": o.content_type,
        "size":        o.size.to_string(),
        "etag":        o.etag,
        "timeCreated": o.created_at,
    })
}

/// Router is mounted at `/gcp` in main.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // Buckets
        .route("/storage/v1/b",              post(create_bucket).get(list_buckets))
        .route("/storage/v1/b/:bucket",      get(get_bucket).delete(delete_bucket))
        // Objects upload
        .route("/upload/storage/v1/b/:bucket/o",          post(upload_object).put(upload_object_put))
        // Objects
        .route("/storage/v1/b/:bucket/o",                 get(list_objects))
        .route("/storage/v1/b/:bucket/o/*object",         get(get_object_meta).delete(delete_object))
        .route("/download/storage/v1/b/:bucket/o/*object", get(download_object))
}

// ── Bucket handlers ───────────────────────────────────────────────────────────

async fn create_bucket(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let name     = body.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let location = body.get("location").and_then(|v| v.as_str());
    match svc(&state).create_bucket(name, location).await {
        Ok(b)  => (StatusCode::OK, Json(bucket_to_json(&b))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn list_buckets(State(state): State<Arc<AppState>>) -> Response {
    match svc(&state).list_buckets().await {
        Ok(bs) => {
            let items: Vec<_> = bs.iter().map(bucket_to_json).collect();
            (StatusCode::OK, Json(serde_json::json!({ "kind": "storage#buckets", "items": items }))).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn get_bucket(
    State(state): State<Arc<AppState>>,
    Path(bucket): Path<String>,
) -> Response {
    match svc(&state).get_bucket(&bucket).await {
        Ok(Some(b)) => (StatusCode::OK, Json(bucket_to_json(&b))).into_response(),
        Ok(None)    => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "Bucket not found" }))).into_response(),
        Err(e)      => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn delete_bucket(
    State(state): State<Arc<AppState>>,
    Path(bucket): Path<String>,
) -> StatusCode {
    match svc(&state).delete_bucket(&bucket).await {
        Ok(_)                                        => StatusCode::NO_CONTENT,
        Err(e) if e.to_string().contains("BucketNotFound") => StatusCode::NOT_FOUND,
        Err(_)                                       => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// ── Object handlers ───────────────────────────────────────────────────────────

async fn upload_object(
    State(state): State<Arc<AppState>>,
    Path(bucket): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let name = params.get("name").map(|s| s.as_str()).unwrap_or("unnamed");
    let ct   = headers.get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");
    match svc(&state).upload_object(&bucket, name, body.to_vec(), ct).await {
        Ok(o)  => (StatusCode::OK, Json(object_to_json(&o))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn upload_object_put(
    State(state): State<Arc<AppState>>,
    Path(bucket): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    upload_object(State(state), Path(bucket), Query(params), headers, body).await
}

async fn list_objects(
    State(state): State<Arc<AppState>>,
    Path(bucket): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let prefix = params.get("prefix").map(|s| s.as_str());
    match svc(&state).list_objects(&bucket, prefix).await {
        Ok(objs) => {
            let items: Vec<_> = objs.iter().map(object_to_json).collect();
            (StatusCode::OK, Json(serde_json::json!({ "kind": "storage#objects", "items": items }))).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn get_object_meta(
    State(state): State<Arc<AppState>>,
    Path((bucket, object)): Path<(String, String)>,
) -> Response {
    match svc(&state).get_object(&bucket, &object).await {
        Ok(o)  => (StatusCode::OK, Json(object_to_json(&o))).into_response(),
        Err(e) if e.to_string().contains("ObjectNotFound") =>
            (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "Object not found" }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn download_object(
    State(state): State<Arc<AppState>>,
    Path((bucket, object)): Path<(String, String)>,
) -> Response {
    match svc(&state).get_object(&bucket, &object).await {
        Ok(o) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE.as_str(),  o.content_type.as_str()),
                (header::ETAG.as_str(),           o.etag.as_str()),
            ],
            o.content,
        ).into_response(),
        Err(e) if e.to_string().contains("ObjectNotFound") =>
            (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "Object not found" }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn delete_object(
    State(state): State<Arc<AppState>>,
    Path((bucket, object)): Path<(String, String)>,
) -> StatusCode {
    match svc(&state).delete_object(&bucket, &object).await {
        Ok(_)  => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

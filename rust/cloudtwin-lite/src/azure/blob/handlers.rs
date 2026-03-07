//! Azure Blob Storage HTTP handlers.
//!
//! Compared with the AWS modules, these routes look more like straightforward
//! REST endpoints. The service still does the real work; handlers just map HTTP
//! verbs, paths, headers, and XML responses.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, head, put},
    Router,
};

use crate::AppState;
use super::service::BlobService;

fn svc(state: &Arc<AppState>) -> BlobService {
    BlobService::new(state.db.clone(), state.cfg.azure_account.clone())
}

fn xml_ok(body: String) -> Response {
    (StatusCode::OK, [(header::CONTENT_TYPE, "application/xml")], body).into_response()
}

fn xml_err(status: StatusCode, code: &str, msg: &str) -> Response {
    (status, [(header::CONTENT_TYPE, "application/xml")],
     format!("<?xml version=\"1.0\"?><Error><Code>{code}</Code><Message>{msg}</Message></Error>"),
    ).into_response()
}

/// Router is mounted at `/azure` in main.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // List containers for an account
        .route("/:account",              get(list_containers))
        // Container operations
        .route("/:account/:container",   put(create_container)
                                             .delete(delete_container)
                                             .get(list_blobs_or_container)
                                             .head(head_container))
        // Blob operations
        .route("/:account/:container/*blob", put(put_blob)
                                                 .get(get_blob)
                                                 .head(head_blob)
                                                 .delete(delete_blob))
}

// ── Container handlers ────────────────────────────────────────────────────────

async fn list_containers(
    State(state): State<Arc<AppState>>,
    Path(account): Path<String>,
) -> Response {
    // Only serve the configured account
    if account != state.cfg.azure_account {
        return xml_err(StatusCode::NOT_FOUND, "AccountNotFound", "Account not found");
    }
    match svc(&state).list_containers().await {
        Ok(containers) => {
            let items: String = containers.iter().map(|c| {
                format!(
                    "<Container><Name>{}</Name><Properties><LastModified>{}</LastModified></Properties></Container>",
                    c.name, c.created_at,
                )
            }).collect();
            xml_ok(format!(
                "<?xml version=\"1.0\"?>\
<EnumerationResults><Containers>{items}</Containers></EnumerationResults>"
            ))
        }
        Err(e) => xml_err(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
    }
}

async fn create_container(
    State(state): State<Arc<AppState>>,
    Path((account, container)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    // Azure SDK sends ?restype=container for container ops
    if params.get("restype").map(|s| s.as_str()) != Some("container") {
        // Might be a blob PUT at container level — handled by blob route
    }
    match svc(&state).create_container(&container).await {
        Ok(_)  => StatusCode::CREATED.into_response(),
        Err(e) => xml_err(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
    }
}

async fn delete_container(
    State(state): State<Arc<AppState>>,
    Path((_, container)): Path<(String, String)>,
) -> Response {
    match svc(&state).delete_container(&container).await {
        Ok(_)                                         => StatusCode::ACCEPTED.into_response(),
        Err(e) if e.to_string().contains("ContainerNotFound") =>
            xml_err(StatusCode::NOT_FOUND, "ContainerNotFound", "Container not found"),
        Err(e) => xml_err(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
    }
}

async fn head_container(
    State(state): State<Arc<AppState>>,
    Path((_, container)): Path<(String, String)>,
) -> StatusCode {
    match svc(&state).container_exists(&container).await {
        Ok(true)  => StatusCode::OK,
        Ok(false) => StatusCode::NOT_FOUND,
        Err(_)    => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn list_blobs_or_container(
    State(state): State<Arc<AppState>>,
    Path((_, container)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let prefix = params.get("prefix").map(|s| s.as_str());
    match svc(&state).list_blobs(&container, prefix).await {
        Ok(blobs) => {
            let items: String = blobs.iter().map(|b| format!(
                "<Blob><Name>{}</Name><Properties>\
<Content-Length>{}</Content-Length>\
<Content-Type>{}</Content-Type>\
<Etag>{}</Etag>\
<Last-Modified>{}</Last-Modified>\
</Properties></Blob>",
                b.name, b.size, b.content_type, b.etag, b.created_at,
            )).collect();
            xml_ok(format!(
                "<?xml version=\"1.0\"?>\
<EnumerationResults>\
<Container>{container}</Container>\
<Blobs>{items}</Blobs>\
</EnumerationResults>"
            ))
        }
        Err(e) => xml_err(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
    }
}

// ── Blob handlers ─────────────────────────────────────────────────────────────

async fn put_blob(
    State(state): State<Arc<AppState>>,
    Path((_, container, blob)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let ct = headers.get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    match svc(&state).put_blob(&container, &blob, body.to_vec(), &ct).await {
        Ok(etag) => (StatusCode::CREATED, [(header::ETAG, etag)]).into_response(),
        Err(e) if e.to_string().contains("ContainerNotFound") =>
            xml_err(StatusCode::NOT_FOUND, "ContainerNotFound", "Container not found"),
        Err(e) => xml_err(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
    }
}

async fn get_blob(
    State(state): State<Arc<AppState>>,
    Path((_, container, blob)): Path<(String, String, String)>,
) -> Response {
    match svc(&state).get_blob(&container, &blob).await {
        Ok(b) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE.as_str(),  b.content_type.as_str()),
                (header::ETAG.as_str(),           b.etag.as_str()),
                ("x-ms-request-id",               "cloudtwin-lite"),
            ],
            b.content,
        ).into_response(),
        Err(e) if e.to_string().contains("BlobNotFound") =>
            xml_err(StatusCode::NOT_FOUND, "BlobNotFound", "Blob not found"),
        Err(e) => xml_err(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
    }
}

async fn head_blob(
    State(state): State<Arc<AppState>>,
    Path((_, container, blob)): Path<(String, String, String)>,
) -> Response {
    match svc(&state).get_blob(&container, &blob).await {
        Ok(b) => {
            let mut h = HeaderMap::new();
            h.insert(header::CONTENT_TYPE, b.content_type.parse().unwrap_or_else(|_| "application/octet-stream".parse().unwrap()));
            h.insert(header::CONTENT_LENGTH, b.size.to_string().parse().unwrap());
            h.insert(header::ETAG, b.etag.parse().unwrap_or_else(|_| "".parse().unwrap()));
            (StatusCode::OK, h).into_response()
        }
        Err(e) if e.to_string().contains("BlobNotFound") => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn delete_blob(
    State(state): State<Arc<AppState>>,
    Path((_, container, blob)): Path<(String, String, String)>,
) -> StatusCode {
    match svc(&state).delete_blob(&container, &blob).await {
        Ok(_)  => StatusCode::ACCEPTED,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

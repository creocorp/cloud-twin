//! DynamoDB AWS JSON protocol handlers.
//!
//! Handlers are the transport adapter layer: they decode protocol fields,
//! invoke the service, and shape the response back into AWS-compatible JSON.

use std::sync::Arc;

use axum::{http::StatusCode, response::{IntoResponse, Response}};

use crate::proto::json_error_response;
use crate::AppState;
use super::service::DynamoDBService;

fn svc(state: &Arc<AppState>) -> DynamoDBService { DynamoDBService::new(state.db.clone()) }

/// AWS JSON target names this module knows how to serve.
pub const JSON_TARGETS: &[&str] = &[
    "DynamoDB_20120810.CreateTable",
    "DynamoDB_20120810.DescribeTable",
    "DynamoDB_20120810.ListTables",
    "DynamoDB_20120810.DeleteTable",
    "DynamoDB_20120810.PutItem",
    "DynamoDB_20120810.GetItem",
    "DynamoDB_20120810.DeleteItem",
    "DynamoDB_20120810.Scan",
    "DynamoDB_20120810.UpdateItem",
    "DynamoDB_20120810.BatchWriteItem",
    "DynamoDB_20120810.BatchGetItem",
];

pub async fn handle_json(state: &Arc<AppState>, target: &str, body: serde_json::Value) -> Response {
    match target {
        "DynamoDB_20120810.CreateTable" => {
            let name = body.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
            let ks   = body.get("KeySchema").map(|v| v.to_string()).unwrap_or_else(|| "[]".into());
            let ad   = body.get("AttributeDefinitions").map(|v| v.to_string()).unwrap_or_else(|| "[]".into());
            match svc(state).create_table(name, &ks, &ad).await {
                Ok(t)  => (StatusCode::OK, axum::Json(serde_json::json!({ "TableDescription": svc(state).table_to_json(&t) }))).into_response(),
                Err(e) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
            }
        }
        "DynamoDB_20120810.DescribeTable" => {
            let name = body.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
            match svc(state).get_table(name).await {
                Ok(Some(t)) => (StatusCode::OK, axum::Json(serde_json::json!({ "Table": svc(state).table_to_json(&t) }))).into_response(),
                Ok(None)    => json_error_response(StatusCode::BAD_REQUEST, "ResourceNotFoundException", &format!("Table not found: {name}")),
                Err(e)      => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
            }
        }
        "DynamoDB_20120810.ListTables" => {
            match svc(state).list_tables().await {
                Ok(names) => (StatusCode::OK, axum::Json(serde_json::json!({ "TableNames": names }))).into_response(),
                Err(e)    => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
            }
        }
        "DynamoDB_20120810.DeleteTable" => {
            let name = body.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
            match svc(state).delete_table(name).await {
                Ok(_)  => (StatusCode::OK, axum::Json(serde_json::json!({ "TableDescription": { "TableName": name, "TableStatus": "DELETING" } }))).into_response(),
                Err(e) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
            }
        }
        "DynamoDB_20120810.PutItem" => {
            let name = body.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
            let item = body.get("Item").cloned().unwrap_or(serde_json::json!({}));
            match svc(state).put_item(name, item).await {
                Ok(_)  => (StatusCode::OK, axum::Json(serde_json::json!({}))).into_response(),
                Err(e) if e.to_string().contains("ResourceNotFoundException") =>
                    json_error_response(StatusCode::BAD_REQUEST, "ResourceNotFoundException", &e.to_string()),
                Err(e) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
            }
        }
        "DynamoDB_20120810.GetItem" => {
            let name = body.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
            let key  = body.get("Key").cloned().unwrap_or(serde_json::json!({}));
            match svc(state).get_item(name, &key).await {
                Ok(Some(item)) => (StatusCode::OK, axum::Json(serde_json::json!({ "Item": item }))).into_response(),
                Ok(None)       => (StatusCode::OK, axum::Json(serde_json::json!({}))).into_response(),
                Err(e) if e.to_string().contains("ResourceNotFoundException") =>
                    json_error_response(StatusCode::BAD_REQUEST, "ResourceNotFoundException", &e.to_string()),
                Err(e) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
            }
        }
        "DynamoDB_20120810.DeleteItem" => {
            let name = body.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
            let key  = body.get("Key").cloned().unwrap_or(serde_json::json!({}));
            match svc(state).delete_item(name, &key).await {
                Ok(_)  => (StatusCode::OK, axum::Json(serde_json::json!({}))).into_response(),
                Err(e) if e.to_string().contains("ResourceNotFoundException") =>
                    json_error_response(StatusCode::BAD_REQUEST, "ResourceNotFoundException", &e.to_string()),
                Err(e) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
            }
        }
        "DynamoDB_20120810.Scan" => {
            let name  = body.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
            let limit = body.get("Limit").and_then(|v| v.as_i64());
            match svc(state).scan(name, limit).await {
                Ok(items) => {
                    let count = items.len();
                    (StatusCode::OK, axum::Json(serde_json::json!({ "Items": items, "Count": count, "ScannedCount": count }))).into_response()
                }
                Err(e) if e.to_string().contains("ResourceNotFoundException") =>
                    json_error_response(StatusCode::BAD_REQUEST, "ResourceNotFoundException", &e.to_string()),
                Err(e) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
            }
        }
        "DynamoDB_20120810.UpdateItem" => {
            let name    = body.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
            let key     = body.get("Key").cloned().unwrap_or(serde_json::json!({}));
            let expr    = body.get("UpdateExpression").and_then(|v| v.as_str());
            let ea_vals = body.get("ExpressionAttributeValues");
            match svc(state).update_item(name, &key, expr, ea_vals).await {
                Ok(item) => (StatusCode::OK, axum::Json(serde_json::json!({ "Attributes": item }))).into_response(),
                Err(e) if e.to_string().contains("ResourceNotFoundException") =>
                    json_error_response(StatusCode::BAD_REQUEST, "ResourceNotFoundException", &e.to_string()),
                Err(e) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
            }
        }
        "DynamoDB_20120810.BatchWriteItem" => {
            let items = body.get("RequestItems").cloned().unwrap_or(serde_json::json!({}));
            match svc(state).batch_write(&items).await {
                Ok(_)  => (StatusCode::OK, axum::Json(serde_json::json!({ "UnprocessedItems": {} }))).into_response(),
                Err(e) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
            }
        }
        "DynamoDB_20120810.BatchGetItem" => {
            let items = body.get("RequestItems").cloned().unwrap_or(serde_json::json!({}));
            match svc(state).batch_get(&items).await {
                Ok(responses) => (StatusCode::OK, axum::Json(serde_json::json!({ "Responses": responses, "UnprocessedKeys": {} }))).into_response(),
                Err(e)        => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
            }
        }
        _ => json_error_response(StatusCode::BAD_REQUEST, "InvalidAction", &format!("Unknown DynamoDB target: {target}")),
    }
}

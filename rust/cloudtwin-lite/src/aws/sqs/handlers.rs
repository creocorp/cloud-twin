//! SQS AWS JSON protocol handlers.
//!
//! This file is the thin edge between the AWS `X-Amz-Target` protocol and the
//! queue service methods.

use std::sync::Arc;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::service::SqsService;
use crate::proto::json_error_response;
use crate::AppState;

fn svc(state: &Arc<AppState>) -> SqsService {
    SqsService::new(state.db.clone(), state.cfg.base_url.clone())
}

pub async fn handle_json(state: &Arc<AppState>, target: &str, body: serde_json::Value) -> Response {
    match target {
        "AmazonSQS.CreateQueue" => {
            let name = body.get("QueueName").and_then(|v| v.as_str()).unwrap_or("");
            if name.is_empty() {
                return json_error_response(
                    StatusCode::BAD_REQUEST,
                    "MissingParameter",
                    "QueueName required",
                );
            }
            match svc(state).create_queue(name).await {
                Ok(url) => (
                    StatusCode::OK,
                    axum::Json(serde_json::json!({ "QueueUrl": url })),
                )
                    .into_response(),
                Err(e) => json_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "InternalError",
                    &e.to_string(),
                ),
            }
        }
        "AmazonSQS.ListQueues" => {
            let prefix = body
                .get("QueueNamePrefix")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match svc(state).list_queues(prefix).await {
                Ok(urls) => (
                    StatusCode::OK,
                    axum::Json(serde_json::json!({ "QueueUrls": urls })),
                )
                    .into_response(),
                Err(e) => json_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "InternalError",
                    &e.to_string(),
                ),
            }
        }
        "AmazonSQS.GetQueueUrl" => {
            let name = body.get("QueueName").and_then(|v| v.as_str()).unwrap_or("");
            match svc(state).get_queue_url(name).await {
                Ok(url) => (
                    StatusCode::OK,
                    axum::Json(serde_json::json!({ "QueueUrl": url })),
                )
                    .into_response(),
                Err(e) if e.to_string().contains("QueueDoesNotExist") => json_error_response(
                    StatusCode::BAD_REQUEST,
                    "QueueDoesNotExist",
                    &e.to_string(),
                ),
                Err(e) => json_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "InternalError",
                    &e.to_string(),
                ),
            }
        }
        "AmazonSQS.SendMessage" => {
            let url = body.get("QueueUrl").and_then(|v| v.as_str()).unwrap_or("");
            let body_str = body
                .get("MessageBody")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match svc(state).send_message(url, body_str).await {
                Ok((mid, _)) => (
                    StatusCode::OK,
                    axum::Json(serde_json::json!({
                        "MessageId":        mid,
                        "MD5OfMessageBody": SqsService::md5_body(body_str),
                    })),
                )
                    .into_response(),
                Err(e) if e.to_string().contains("QueueDoesNotExist") => json_error_response(
                    StatusCode::BAD_REQUEST,
                    "QueueDoesNotExist",
                    &e.to_string(),
                ),
                Err(e) => json_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "InternalError",
                    &e.to_string(),
                ),
            }
        }
        "AmazonSQS.ReceiveMessage" => {
            let url = body.get("QueueUrl").and_then(|v| v.as_str()).unwrap_or("");
            let max = body
                .get("MaxNumberOfMessages")
                .and_then(|v| v.as_i64())
                .unwrap_or(1);
            match svc(state).receive_messages(url, max).await {
                Ok(msgs) => {
                    let items: Vec<_> = msgs
                        .iter()
                        .map(|m| {
                            serde_json::json!({
                                "MessageId":     m.message_id,
                                "ReceiptHandle": m.receipt_handle,
                                "Body":          m.body,
                                "MD5OfBody":     SqsService::md5_body(&m.body),
                            })
                        })
                        .collect();
                    (
                        StatusCode::OK,
                        axum::Json(serde_json::json!({ "Messages": items })),
                    )
                        .into_response()
                }
                Err(e) if e.to_string().contains("QueueDoesNotExist") => json_error_response(
                    StatusCode::BAD_REQUEST,
                    "QueueDoesNotExist",
                    &e.to_string(),
                ),
                Err(e) => json_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "InternalError",
                    &e.to_string(),
                ),
            }
        }
        "AmazonSQS.DeleteMessage" => {
            let rh = body
                .get("ReceiptHandle")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match svc(state).delete_message(rh).await {
                Ok(_) => (StatusCode::OK, axum::Json(serde_json::json!({}))).into_response(),
                Err(e) => json_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "InternalError",
                    &e.to_string(),
                ),
            }
        }
        "AmazonSQS.DeleteQueue" => {
            let url = body.get("QueueUrl").and_then(|v| v.as_str()).unwrap_or("");
            match svc(state).delete_queue(url).await {
                Ok(_) => (StatusCode::OK, axum::Json(serde_json::json!({}))).into_response(),
                Err(e) => json_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "InternalError",
                    &e.to_string(),
                ),
            }
        }
        "AmazonSQS.ChangeMessageVisibility" => {
            let _url = body.get("QueueUrl").and_then(|v| v.as_str()).unwrap_or("");
            let rh = body
                .get("ReceiptHandle")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let to = body
                .get("VisibilityTimeout")
                .and_then(|v| v.as_i64())
                .unwrap_or(30);
            match svc(state).change_visibility(rh, to).await {
                Ok(_) => (StatusCode::OK, axum::Json(serde_json::json!({}))).into_response(),
                Err(e) => json_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "InternalError",
                    &e.to_string(),
                ),
            }
        }
        "AmazonSQS.GetQueueAttributes" => {
            let url = body.get("QueueUrl").and_then(|v| v.as_str()).unwrap_or("");
            match svc(state).get_queue_attributes(url).await {
                Ok(attrs) => (StatusCode::OK, axum::Json(attrs)).into_response(),
                Err(e) if e.to_string().contains("QueueDoesNotExist") => json_error_response(
                    StatusCode::BAD_REQUEST,
                    "QueueDoesNotExist",
                    &e.to_string(),
                ),
                Err(e) => json_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "InternalError",
                    &e.to_string(),
                ),
            }
        }
        _ => json_error_response(
            StatusCode::BAD_REQUEST,
            "InvalidAction",
            &format!("Unknown SQS target: {target}"),
        ),
    }
}

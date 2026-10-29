//! Secrets Manager AWS JSON protocol handlers.

use std::sync::Arc;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::service::SecretsManagerService;
use crate::proto::json_error_response;
use crate::AppState;

fn svc(state: &Arc<AppState>) -> SecretsManagerService {
    SecretsManagerService::new(state.db.clone())
}

#[allow(dead_code)]
pub const JSON_TARGETS: &[&str] = &[
    "secretsmanager.CreateSecret",
    "secretsmanager.GetSecretValue",
    "secretsmanager.PutSecretValue",
    "secretsmanager.ListSecrets",
    "secretsmanager.DeleteSecret",
    "secretsmanager.DescribeSecret",
];

pub async fn handle_json(state: &Arc<AppState>, target: &str, body: serde_json::Value) -> Response {
    match target {
        "secretsmanager.CreateSecret" => {
            let name = body.get("Name").and_then(|v| v.as_str()).unwrap_or("");
            let ss = body.get("SecretString").and_then(|v| v.as_str());
            match svc(state).create_secret(name, ss).await {
                Ok(s) => (
                    StatusCode::OK,
                    axum::Json(serde_json::json!({
                        "ARN":  s.arn,
                        "Name": s.name,
                    })),
                )
                    .into_response(),
                Err(e) if e.to_string().contains("ResourceExistsException") => json_error_response(
                    StatusCode::BAD_REQUEST,
                    "ResourceExistsException",
                    &e.to_string(),
                ),
                Err(e) => json_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "InternalError",
                    &e.to_string(),
                ),
            }
        }
        "secretsmanager.GetSecretValue" | "secretsmanager.DescribeSecret" => {
            let id = body.get("SecretId").and_then(|v| v.as_str()).unwrap_or("");
            match svc(state).get_secret_value(id).await {
                Ok(s) => (
                    StatusCode::OK,
                    axum::Json(serde_json::json!({
                        "ARN":          s.arn,
                        "Name":         s.name,
                        "SecretString": s.secret_string,
                        "CreatedDate":  s.created_at,
                    })),
                )
                    .into_response(),
                Err(e) if e.to_string().contains("ResourceNotFoundException") => {
                    json_error_response(
                        StatusCode::BAD_REQUEST,
                        "ResourceNotFoundException",
                        &e.to_string(),
                    )
                }
                Err(e) => json_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "InternalError",
                    &e.to_string(),
                ),
            }
        }
        "secretsmanager.PutSecretValue" => {
            let id = body.get("SecretId").and_then(|v| v.as_str()).unwrap_or("");
            let ss = body
                .get("SecretString")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match svc(state).put_secret_value(id, ss).await {
                Ok(_) => (
                    StatusCode::OK,
                    axum::Json(serde_json::json!({ "ARN": id, "Name": id })),
                )
                    .into_response(),
                Err(e) if e.to_string().contains("ResourceNotFoundException") => {
                    json_error_response(
                        StatusCode::BAD_REQUEST,
                        "ResourceNotFoundException",
                        &e.to_string(),
                    )
                }
                Err(e) => json_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "InternalError",
                    &e.to_string(),
                ),
            }
        }
        "secretsmanager.ListSecrets" => match svc(state).list_secrets().await {
            Ok(secrets) => {
                let list: Vec<_> = secrets
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "ARN":  s.arn,
                            "Name": s.name,
                        })
                    })
                    .collect();
                (
                    StatusCode::OK,
                    axum::Json(serde_json::json!({ "SecretList": list })),
                )
                    .into_response()
            }
            Err(e) => json_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalError",
                &e.to_string(),
            ),
        },
        "secretsmanager.DeleteSecret" => {
            let id = body.get("SecretId").and_then(|v| v.as_str()).unwrap_or("");
            match svc(state).delete_secret(id).await {
                Ok(_) => (
                    StatusCode::OK,
                    axum::Json(serde_json::json!({ "ARN": id, "Name": id })),
                )
                    .into_response(),
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
            &format!("Unknown SecretsManager target: {target}"),
        ),
    }
}

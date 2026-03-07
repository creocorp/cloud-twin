//! AWS protocol helpers shared by all AWS service dispatchers.

use std::collections::HashMap;

use axum::{
    body::Bytes,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

pub enum AwsPayload {
    /// Form-urlencoded body (SES v1, SNS).
    Query(HashMap<String, String>),
    /// JSON body with X-Amz-Target header (SQS, DynamoDB, SecretsManager).
    Json { target: String, body: serde_json::Value },
}

impl AwsPayload {
    pub fn parse(headers: &HeaderMap, raw: &Bytes) -> Self {
        let ct = headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if ct.contains("application/x-amz-json-1.0") {
            let target = headers
                .get("x-amz-target")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let body = serde_json::from_slice(raw)
                .unwrap_or(serde_json::Value::Object(Default::default()));
            AwsPayload::Json { target, body }
        } else {
            let params: HashMap<String, String> =
                serde_urlencoded::from_bytes(raw).unwrap_or_default();
            AwsPayload::Query(params)
        }
    }
}

pub fn xml_error_response(status: StatusCode, code: &str, message: &str) -> Response {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ErrorResponse>\
<Error><Type>Sender</Type><Code>{code}</Code><Message>{msg}</Message></Error>\
<RequestId>cloudtwin-lite</RequestId>\
</ErrorResponse>",
        code = xml_escape(code),
        msg  = xml_escape(message),
    );
    (status, [(header::CONTENT_TYPE, "text/xml")], body).into_response()
}

pub fn json_error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/x-amz-json-1.0")],
        serde_json::json!({ "__type": code, "message": message }).to_string(),
    )
        .into_response()
}

pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\'', "&apos;")
}

/// Wrap inner XML in the standard AWS query-protocol response envelope.
pub fn wrap_xml(action: &str, xmlns: &str, inner: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<{action}Response xmlns=\"{xmlns}\">\
{inner}\
<ResponseMetadata><RequestId>cloudtwin-lite</RequestId></ResponseMetadata>\
</{action}Response>",
    )
}

pub fn xml_ok(body: String) -> Response {
    (StatusCode::OK, [(header::CONTENT_TYPE, "text/xml")], body).into_response()
}

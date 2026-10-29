//! AWS wire-format helpers shared by all AWS service dispatchers.
//!
//! This module is the small translation layer between raw HTTP and the service
//! routers. In ASP.NET terms this is somewhere between custom middleware and a
//! model binder. In FastAPI terms it is a manual request parser for AWS's older
//! protocol conventions.

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
    Json {
        target: String,
        body: serde_json::Value,
    },
}

impl AwsPayload {
    /// Decode an incoming AWS request into either query-protocol parameters or
    /// a JSON payload with its `X-Amz-Target` operation name.
    pub fn parse(headers: &HeaderMap, raw: &Bytes) -> Self {
        let ct = headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        // SQS/DynamoDB use 1.0; Secrets Manager, KMS, and Cognito use 1.1.
        if ct.contains("application/x-amz-json-") {
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
    // AWS query APIs expect XML envelopes even for errors.
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ErrorResponse>\
<Error><Type>Sender</Type><Code>{code}</Code><Message>{msg}</Message></Error>\
<RequestId>cloudtwin</RequestId>\
</ErrorResponse>",
        code = xml_escape(code),
        msg = xml_escape(message),
    );
    (status, [(header::CONTENT_TYPE, "text/xml")], body).into_response()
}

pub fn json_error_response(status: StatusCode, code: &str, message: &str) -> Response {
    // AWS JSON APIs expect the error type in `__type` rather than a more RESTy
    // `{ code, message }` shape.
    (
        status,
        [(header::CONTENT_TYPE, "application/x-amz-json-1.0")],
        serde_json::json!({ "__type": code, "message": message }).to_string(),
    )
        .into_response()
}

/// Error response for AWS REST/JSON (`restJson1`) services such as SES v2.
///
/// SDKs classify typed errors from the `x-amzn-ErrorType` header first and the
/// body's `__type`/`code` second, so both are set.
pub fn rest_json_error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE.as_str(), "application/json"),
            ("x-amzn-ErrorType", code),
            ("x-amzn-RequestId", "cloudtwin"),
        ],
        serde_json::json!({ "__type": code, "code": code, "message": message, "Message": message })
            .to_string(),
    )
        .into_response()
}

/// Successful AWS REST/JSON response.
pub fn rest_json_ok(value: serde_json::Value) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE.as_str(), "application/json"),
            ("x-amzn-RequestId", "cloudtwin"),
        ],
        value.to_string(),
    )
        .into_response()
}

/// Successful AWS JSON-protocol (`X-Amz-Target`) response.
pub fn json_ok(value: serde_json::Value) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/x-amz-json-1.1")],
        value.to_string(),
    )
        .into_response()
}

/// Decode flattened AWS Query parameters under `prefix` into nested JSON.
///
/// `Rule.Actions.member.1.S3Action.BucketName=b` with prefix `Rule` becomes
/// `{"Actions": [{"S3Action": {"BucketName": "b"}}]}`. `member.N` and
/// `entry.N` lists become JSON arrays ordered by N. Leaf values stay strings.
pub fn query_to_json(params: &HashMap<String, String>, prefix: &str) -> serde_json::Value {
    use std::collections::BTreeMap;

    enum Node {
        Leaf(String),
        Map(BTreeMap<String, Node>),
    }

    fn insert(node: &mut BTreeMap<String, Node>, path: &[&str], value: &str) {
        let Some((head, rest)) = path.split_first() else {
            return;
        };
        if rest.is_empty() {
            node.insert(head.to_string(), Node::Leaf(value.to_string()));
            return;
        }
        let child = node
            .entry(head.to_string())
            .or_insert_with(|| Node::Map(BTreeMap::new()));
        if let Node::Leaf(_) = child {
            *child = Node::Map(BTreeMap::new());
        }
        if let Node::Map(m) = child {
            insert(m, rest, value);
        }
    }

    fn convert(node: Node) -> serde_json::Value {
        match node {
            Node::Leaf(s) => serde_json::Value::String(s),
            Node::Map(m) => {
                let numeric = !m.is_empty() && m.keys().all(|k| k.parse::<u64>().is_ok());
                if numeric {
                    let mut items: Vec<(u64, Node)> = m
                        .into_iter()
                        .map(|(k, v)| (k.parse::<u64>().unwrap_or(0), v))
                        .collect();
                    items.sort_by_key(|(k, _)| *k);
                    return serde_json::Value::Array(
                        items.into_iter().map(|(_, v)| convert(v)).collect(),
                    );
                }
                if m.len() == 1 {
                    if let Some(key) = m.keys().next().cloned() {
                        if key == "member" || key == "entry" {
                            let inner = m.into_iter().next().map(|(_, v)| convert(v));
                            if let Some(v @ serde_json::Value::Array(_)) = inner {
                                return v;
                            }
                            return inner.unwrap_or(serde_json::Value::Null);
                        }
                    }
                }
                serde_json::Value::Object(m.into_iter().map(|(k, v)| (k, convert(v))).collect())
            }
        }
    }

    let mut root: BTreeMap<String, Node> = BTreeMap::new();
    for (k, v) in params {
        let rest = if prefix.is_empty() {
            Some(k.as_str())
        } else {
            k.strip_prefix(prefix).and_then(|r| r.strip_prefix('.'))
        };
        if let Some(rest) = rest {
            let parts: Vec<&str> = rest.split('.').collect();
            insert(&mut root, &parts, v);
        }
    }
    convert(Node::Map(root))
}

/// Encode JSON as AWS Query-style XML: object keys become elements and arrays
/// become repeated `<member>` elements.
pub fn json_to_xml(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(m) => m
            .iter()
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| format!("<{k}>{}</{k}>", json_to_xml(v)))
            .collect(),
        serde_json::Value::Array(a) => a
            .iter()
            .map(|v| format!("<member>{}</member>", json_to_xml(v)))
            .collect(),
        serde_json::Value::String(s) => xml_escape(s),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Null => String::new(),
    }
}

pub fn xml_escape(s: &str) -> String {
    // Keep escaping local and obvious instead of pulling in a heavier XML lib.
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
<ResponseMetadata><RequestId>cloudtwin</RequestId></ResponseMetadata>\
</{action}Response>",
    )
}

pub fn xml_ok(body: String) -> Response {
    (StatusCode::OK, [(header::CONTENT_TYPE, "text/xml")], body).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn query_params_decode_to_nested_json() {
        let params: HashMap<String, String> = [
            ("Rule.Name", "r1"),
            ("Rule.Recipients.member.2", "b@x.test"),
            ("Rule.Recipients.member.1", "x.test"),
            ("Rule.Actions.member.1.S3Action.BucketName", "inbound"),
            ("Rule.Actions.member.2.StopAction.Scope", "RuleSet"),
            ("Attributes.entry.1.key", "RawMessageDelivery"),
            ("Attributes.entry.1.value", "true"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        assert_eq!(
            query_to_json(&params, "Rule"),
            json!({
                "Name": "r1",
                "Recipients": ["x.test", "b@x.test"],
                "Actions": [{"S3Action": {"BucketName": "inbound"}}, {"StopAction": {"Scope": "RuleSet"}}],
            })
        );
        assert_eq!(
            query_to_json(&params, "Attributes"),
            json!([{"key": "RawMessageDelivery", "value": "true"}])
        );
        assert_eq!(
            json_to_xml(&json!({"Name": "a&b", "List": ["x"]})),
            "<List><member>x</member></List><Name>a&amp;b</Name>"
        );
    }
}

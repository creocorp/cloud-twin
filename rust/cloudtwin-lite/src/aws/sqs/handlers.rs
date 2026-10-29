//! SQS AWS JSON protocol handlers (`X-Amz-Target: AmazonSQS.*`).
//!
//! Errors carry the modern shape name in `__type` and the legacy query error
//! code in `x-amzn-query-error`, so both current SDKs (which read `__type`)
//! and query-compatible ones (boto3 maps e.g.
//! `AWS.SimpleQueueService.NonExistentQueue`) see the codes AWS sends.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{json, Map, Value};

use super::errors::{Result, SqsError};
use super::models::{attrs_to_json, md5_hex, md5_of_attrs, parse_attrs, SendInput};
use super::service::{message_size, ReceiveOptions, SqsService};
use crate::telemetry;
use crate::AppState;

fn svc(state: &Arc<AppState>) -> SqsService {
    SqsService::new(state.db.clone(), state.cfg.base_url.clone())
}

fn ok(v: Value) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE.as_str(), "application/x-amz-json-1.0"),
            ("x-amzn-RequestId", "cloudtwin-lite"),
        ],
        v.to_string(),
    )
        .into_response()
}

pub fn error_response(e: &SqsError) -> Response {
    let status = StatusCode::from_u16(e.status()).unwrap_or(StatusCode::BAD_REQUEST);
    let query = format!(
        "{};{}",
        e.query_code,
        if e.sender_fault { "Sender" } else { "Receiver" }
    );
    (
        status,
        [
            (
                header::CONTENT_TYPE.as_str(),
                "application/x-amz-json-1.0".to_string(),
            ),
            ("x-amzn-query-error", query),
            ("x-amzn-RequestId", "cloudtwin-lite".to_string()),
        ],
        json!({"__type": format!("com.amazonaws.sqs#{}", e.code), "message": e.message})
            .to_string(),
    )
        .into_response()
}

async fn emit(state: &Arc<AppState>, action: &str, payload: Value) {
    telemetry::emit(&state.db, "aws", "sqs", action, &payload.to_string()).await;
}

// ── Request field helpers ────────────────────────────────────────────────────

fn str_field<'a>(body: &'a Value, key: &str) -> Option<&'a str> {
    body.get(key).and_then(|v| v.as_str())
}

fn required<'a>(body: &'a Value, key: &str) -> Result<&'a str> {
    str_field(body, key)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| SqsError::missing_parameter(key))
}

fn int_field(body: &Value, key: &str) -> Result<Option<i64>> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n.as_i64().map(Some).ok_or_else(|| {
            SqsError::invalid_parameter(format!("Invalid value for the parameter {key}."))
        }),
        Some(Value::String(s)) => s.trim().parse().map(Some).map_err(|_| {
            SqsError::invalid_parameter(format!("Invalid value for the parameter {key}."))
        }),
        Some(_) => Err(SqsError::invalid_parameter(format!(
            "Invalid value for the parameter {key}."
        ))),
    }
}

fn string_map(v: Option<&Value>) -> BTreeMap<String, String> {
    v.and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .map(|(k, v)| {
                    let s = match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    (k.clone(), s)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn string_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn send_input(entry: &Value) -> Result<SendInput> {
    let body = str_field(entry, "MessageBody")
        .ok_or_else(|| SqsError::missing_parameter("MessageBody"))?
        .to_string();
    Ok(SendInput {
        body,
        delay_seconds: int_field(entry, "DelaySeconds")?,
        attributes: parse_attrs(entry.get("MessageAttributes"))
            .map_err(SqsError::invalid_parameter)?,
        system_attributes: parse_attrs(entry.get("MessageSystemAttributes"))
            .map_err(SqsError::invalid_parameter)?,
        group_id: str_field(entry, "MessageGroupId").map(String::from),
        dedup_id: str_field(entry, "MessageDeduplicationId").map(String::from),
    })
}

fn valid_batch_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn batch_entries(body: &Value) -> Result<Vec<Value>> {
    let entries = body
        .get("Entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if entries.is_empty() {
        return Err(SqsError::empty_batch());
    }
    if entries.len() > 10 {
        return Err(SqsError::too_many_entries(entries.len()));
    }
    let mut seen = HashSet::new();
    for e in &entries {
        let id = str_field(e, "Id").unwrap_or("");
        if !valid_batch_id(id) {
            return Err(SqsError::invalid_batch_entry_id(id));
        }
        if !seen.insert(id.to_string()) {
            return Err(SqsError::ids_not_distinct(id));
        }
    }
    Ok(entries)
}

fn failed_entry(id: &str, e: &SqsError) -> Value {
    json!({"Id": id, "SenderFault": e.sender_fault, "Code": e.query_code, "Message": e.message})
}

fn send_result(out: &super::models::SendOutcome) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("MessageId".into(), json!(out.message_id));
    m.insert("MD5OfMessageBody".into(), json!(out.md5_body));
    if let Some(v) = &out.md5_attrs {
        m.insert("MD5OfMessageAttributes".into(), json!(v));
    }
    if let Some(v) = &out.md5_system_attrs {
        m.insert("MD5OfMessageSystemAttributes".into(), json!(v));
    }
    if let Some(v) = &out.sequence_number {
        m.insert("SequenceNumber".into(), json!(v));
    }
    m
}

// ── Dispatch ─────────────────────────────────────────────────────────────────

pub async fn handle_json(state: &Arc<AppState>, target: &str, body: Value) -> Response {
    let action = target.strip_prefix("AmazonSQS.").unwrap_or(target);
    match dispatch(state, action, &body).await {
        Ok(v) => ok(v),
        Err(e) => error_response(&e),
    }
}

async fn dispatch(state: &Arc<AppState>, action: &str, body: &Value) -> Result<Value> {
    let s = svc(state);
    match action {
        "CreateQueue" => {
            let name = required(body, "QueueName")?;
            let attrs = string_map(body.get("Attributes"));
            let tags = string_map(body.get("tags").or_else(|| body.get("Tags")));
            let url = s.create_queue(name, attrs, tags).await?;
            emit(state, "create_queue", json!({"queue": name})).await;
            Ok(json!({"QueueUrl": url}))
        }
        "GetQueueUrl" => {
            let name = required(body, "QueueName")?;
            Ok(json!({"QueueUrl": s.get_queue_url(name).await?}))
        }
        "ListQueues" => {
            let prefix = str_field(body, "QueueNamePrefix").unwrap_or("");
            let max = int_field(body, "MaxResults")?;
            let (urls, token) = s
                .list_queues(prefix, max, str_field(body, "NextToken"))
                .await?;
            let mut out = Map::new();
            if !urls.is_empty() {
                out.insert("QueueUrls".into(), json!(urls));
            }
            if let Some(t) = token {
                out.insert("NextToken".into(), json!(t));
            }
            Ok(Value::Object(out))
        }
        "ListDeadLetterSourceQueues" => {
            let url = required(body, "QueueUrl")?;
            let max = int_field(body, "MaxResults")?;
            let (urls, token) = s
                .list_dead_letter_source_queues(url, max, str_field(body, "NextToken"))
                .await?;
            let mut out = Map::new();
            out.insert("queueUrls".into(), json!(urls));
            if let Some(t) = token {
                out.insert("NextToken".into(), json!(t));
            }
            Ok(Value::Object(out))
        }
        "DeleteQueue" => {
            let url = required(body, "QueueUrl")?;
            s.delete_queue(url).await?;
            emit(state, "delete_queue", json!({"queue_url": url})).await;
            Ok(json!({}))
        }
        "PurgeQueue" => {
            let url = required(body, "QueueUrl")?;
            s.purge_queue(url).await?;
            emit(state, "purge_queue", json!({"queue_url": url})).await;
            Ok(json!({}))
        }
        "GetQueueAttributes" => {
            let url = required(body, "QueueUrl")?;
            let names = string_list(body.get("AttributeNames"));
            let attrs = s.get_queue_attributes(url, names).await?;
            if attrs.is_empty() {
                Ok(json!({}))
            } else {
                Ok(json!({"Attributes": attrs}))
            }
        }
        "SetQueueAttributes" => {
            let url = required(body, "QueueUrl")?;
            let attrs = string_map(body.get("Attributes"));
            s.set_queue_attributes(url, attrs.clone()).await?;
            emit(
                state,
                "set_queue_attributes",
                json!({"queue_url": url, "attributes": attrs}),
            )
            .await;
            Ok(json!({}))
        }
        "TagQueue" => {
            let url = required(body, "QueueUrl")?;
            s.tag_queue(url, string_map(body.get("Tags"))).await?;
            emit(state, "tag_queue", json!({"queue_url": url})).await;
            Ok(json!({}))
        }
        "UntagQueue" => {
            let url = required(body, "QueueUrl")?;
            s.untag_queue(url, string_list(body.get("TagKeys"))).await?;
            emit(state, "untag_queue", json!({"queue_url": url})).await;
            Ok(json!({}))
        }
        "ListQueueTags" => {
            let url = required(body, "QueueUrl")?;
            let q = s.get_queue(url).await?;
            if q.tags.is_empty() {
                Ok(json!({}))
            } else {
                Ok(json!({"Tags": q.tags}))
            }
        }
        "AddPermission" | "RemovePermission" => {
            let url = required(body, "QueueUrl")?;
            s.get_queue(url).await?;
            Ok(json!({}))
        }
        "SendMessage" => {
            let url = required(body, "QueueUrl")?;
            let input = send_input(body)?;
            let out = s.send(url, input).await?;
            emit(
                state,
                "send_message",
                json!({"queue_url": url, "message_id": out.message_id}),
            )
            .await;
            Ok(Value::Object(send_result(&out)))
        }
        "SendMessageBatch" => {
            let url = required(body, "QueueUrl")?;
            s.get_queue(url).await?;
            let entries = batch_entries(body)?;
            let inputs: Vec<(String, Result<SendInput>)> = entries
                .iter()
                .map(|e| (str_field(e, "Id").unwrap_or("").to_string(), send_input(e)))
                .collect();
            let total: usize = inputs
                .iter()
                .filter_map(|(_, i)| i.as_ref().ok().map(message_size))
                .sum();
            s.check_batch_size(url, total).await?;
            let (mut ok_entries, mut failed) = (Vec::new(), Vec::new());
            for (id, input) in inputs {
                let res = match input {
                    Ok(i) => s.send(url, i).await,
                    Err(e) => Err(e),
                };
                match res {
                    Ok(out) => {
                        let mut m = send_result(&out);
                        m.insert("Id".into(), json!(id));
                        ok_entries.push(Value::Object(m));
                    }
                    Err(e) => failed.push(failed_entry(&id, &e)),
                }
            }
            emit(
                state,
                "send_message_batch",
                json!({"queue_url": url, "count": ok_entries.len()}),
            )
            .await;
            Ok(json!({"Successful": ok_entries, "Failed": failed}))
        }
        "ReceiveMessage" => {
            let url = required(body, "QueueUrl")?;
            let mut sys = string_list(body.get("AttributeNames"));
            sys.extend(string_list(body.get("MessageSystemAttributeNames")));
            let opts = ReceiveOptions {
                max: int_field(body, "MaxNumberOfMessages")?.unwrap_or(1),
                visibility_timeout: int_field(body, "VisibilityTimeout")?,
                wait_seconds: int_field(body, "WaitTimeSeconds")?,
                system_attribute_names: sys,
                message_attribute_names: string_list(body.get("MessageAttributeNames")),
            };
            let msgs = s.receive(url, opts).await?;
            if msgs.is_empty() {
                return Ok(json!({}));
            }
            let items: Vec<Value> = msgs
                .iter()
                .map(|m| {
                    let mut o = Map::new();
                    o.insert("MessageId".into(), json!(m.message_id));
                    o.insert("ReceiptHandle".into(), json!(m.receipt_handle));
                    o.insert("MD5OfBody".into(), json!(md5_hex(m.body.as_bytes())));
                    o.insert("Body".into(), json!(m.body));
                    if !m.attributes.is_empty() {
                        o.insert("Attributes".into(), json!(m.attributes));
                    }
                    if !m.message_attributes.is_empty() {
                        o.insert(
                            "MD5OfMessageAttributes".into(),
                            json!(md5_of_attrs(&m.message_attributes)),
                        );
                        o.insert(
                            "MessageAttributes".into(),
                            attrs_to_json(&m.message_attributes),
                        );
                    }
                    Value::Object(o)
                })
                .collect();
            emit(
                state,
                "receive_message",
                json!({"queue_url": url, "count": items.len()}),
            )
            .await;
            Ok(json!({"Messages": items}))
        }
        "DeleteMessage" => {
            let url = required(body, "QueueUrl")?;
            let rh = required(body, "ReceiptHandle")?;
            s.delete_message(url, rh).await?;
            emit(state, "delete_message", json!({"queue_url": url})).await;
            Ok(json!({}))
        }
        "DeleteMessageBatch" => {
            let url = required(body, "QueueUrl")?;
            s.get_queue(url).await?;
            let entries = batch_entries(body)?;
            let (mut ok_entries, mut failed) = (Vec::new(), Vec::new());
            for e in &entries {
                let id = str_field(e, "Id").unwrap_or("");
                match s
                    .delete_message(url, str_field(e, "ReceiptHandle").unwrap_or(""))
                    .await
                {
                    Ok(()) => ok_entries.push(json!({"Id": id})),
                    Err(err) => failed.push(failed_entry(id, &err)),
                }
            }
            emit(
                state,
                "delete_message_batch",
                json!({"queue_url": url, "count": ok_entries.len()}),
            )
            .await;
            Ok(json!({"Successful": ok_entries, "Failed": failed}))
        }
        "ChangeMessageVisibility" => {
            let url = required(body, "QueueUrl")?;
            let rh = required(body, "ReceiptHandle")?;
            let to = int_field(body, "VisibilityTimeout")?
                .ok_or_else(|| SqsError::missing_parameter("VisibilityTimeout"))?;
            s.change_visibility(url, rh, to).await?;
            emit(
                state,
                "change_message_visibility",
                json!({"queue_url": url, "timeout": to}),
            )
            .await;
            Ok(json!({}))
        }
        "ChangeMessageVisibilityBatch" => {
            let url = required(body, "QueueUrl")?;
            s.get_queue(url).await?;
            let entries = batch_entries(body)?;
            let (mut ok_entries, mut failed) = (Vec::new(), Vec::new());
            for e in &entries {
                let id = str_field(e, "Id").unwrap_or("");
                let res = match int_field(e, "VisibilityTimeout") {
                    Ok(Some(to)) => {
                        s.change_visibility(url, str_field(e, "ReceiptHandle").unwrap_or(""), to)
                            .await
                    }
                    Ok(None) => Err(SqsError::missing_parameter("VisibilityTimeout")),
                    Err(err) => Err(err),
                };
                match res {
                    Ok(()) => ok_entries.push(json!({"Id": id})),
                    Err(err) => failed.push(failed_entry(id, &err)),
                }
            }
            emit(
                state,
                "change_message_visibility_batch",
                json!({"queue_url": url}),
            )
            .await;
            Ok(json!({"Successful": ok_entries, "Failed": failed}))
        }
        _ => Err(SqsError::unsupported(format!(
            "Unknown SQS operation: {action}"
        ))),
    }
}

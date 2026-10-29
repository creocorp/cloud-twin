//! SNS AWS Query protocol handlers (form-urlencoded in, XML out).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use axum::{http::StatusCode, response::Response};
use base64::Engine;
use serde_json::{json, Value};

use super::delivery::{self, PublishRequest};
use super::models::SnsSubscription;
use super::service::{decode_token, page, Result, SnsError, SnsService};
use crate::aws::sqs::{MessageAttributes, MsgAttr};
use crate::proto::{query_to_json, wrap_xml, xml_error_response, xml_escape, xml_ok};
use crate::telemetry;
use crate::AppState;

fn svc(state: &Arc<AppState>) -> SnsService {
    SnsService::new(state.db.clone())
}

const NS: &str = "https://sns.amazonaws.com/doc/2010-03-31/";
const PAGE_SIZE: usize = 100;

/// Query actions handled by SNS (checked after SES's list).
pub const QUERY_ACTIONS: &[&str] = &[
    "CreateTopic",
    "ListTopics",
    "DeleteTopic",
    "GetTopicAttributes",
    "SetTopicAttributes",
    "Subscribe",
    "Unsubscribe",
    "ListSubscriptions",
    "ListSubscriptionsByTopic",
    "GetSubscriptionAttributes",
    "SetSubscriptionAttributes",
    "Publish",
    "PublishBatch",
    "TagResource",
    "UntagResource",
    "ListTagsForResource",
];

// ── Parameter decoding ───────────────────────────────────────────────────────

fn param<'a>(params: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    params.get(key).map(|s| s.as_str())
}

fn required<'a>(params: &'a HashMap<String, String>, key: &str) -> Result<&'a str> {
    param(params, key).filter(|s| !s.is_empty()).ok_or_else(|| {
        SnsError::invalid(format!(
            "Invalid parameter: {key} Reason: no value for required parameter"
        ))
    })
}

fn as_array(v: Value) -> Vec<Value> {
    match v {
        Value::Array(a) => a,
        Value::Null => Vec::new(),
        Value::Object(o) if o.is_empty() => Vec::new(),
        other => vec![other],
    }
}

fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|s| s.as_str()).map(String::from)
}

/// `X.entry.N.key` / `X.entry.N.value` → map.
fn entry_map(params: &HashMap<String, String>, prefix: &str) -> BTreeMap<String, String> {
    as_array(query_to_json(params, prefix))
        .iter()
        .filter_map(|e| Some((str_of(e, "key")?, str_of(e, "value").unwrap_or_default())))
        .collect()
}

/// `Tags.member.N.Key` / `.Value` → map.
fn tag_map(params: &HashMap<String, String>, prefix: &str) -> BTreeMap<String, String> {
    as_array(query_to_json(params, prefix))
        .iter()
        .filter_map(|e| Some((str_of(e, "Key")?, str_of(e, "Value").unwrap_or_default())))
        .collect()
}

/// Message attributes from decoded `MessageAttributes.entry.N.{Name,Value.*}` items.
fn message_attributes(items: Vec<Value>) -> Result<MessageAttributes> {
    let mut out = MessageAttributes::new();
    for item in items {
        let name = str_of(&item, "Name").unwrap_or_default();
        let value = item.get("Value").cloned().unwrap_or(Value::Null);
        let data_type = str_of(&value, "DataType").ok_or_else(|| {
            SnsError::invalid_value(format!(
                "The message attribute '{name}' must contain non-empty message attribute type."
            ))
        })?;
        let binary_value = match str_of(&value, "BinaryValue") {
            Some(b) => Some(
                base64::engine::general_purpose::STANDARD
                    .decode(b)
                    .map_err(|_| {
                        SnsError::invalid_value(format!(
                            "The message attribute '{name}' has an invalid binary value."
                        ))
                    })?,
            ),
            None => None,
        };
        out.insert(
            name,
            MsgAttr {
                data_type,
                string_value: str_of(&value, "StringValue"),
                binary_value,
            },
        );
    }
    Ok(out)
}

// ── XML helpers ──────────────────────────────────────────────────────────────

fn respond(action: &str, inner: &str) -> Response {
    let body = if inner.is_empty() {
        String::new()
    } else {
        format!("<{action}Result>{inner}</{action}Result>")
    };
    xml_ok(wrap_xml(action, NS, &body))
}

fn el(name: &str, value: &str) -> String {
    format!("<{name}>{}</{name}>", xml_escape(value))
}

fn attributes_xml(attrs: &BTreeMap<String, String>) -> String {
    let entries: String = attrs
        .iter()
        .map(|(k, v)| format!("<entry>{}{}</entry>", el("key", k), el("value", v)))
        .collect();
    format!("<Attributes>{entries}</Attributes>")
}

fn subscription_member(s: &SnsSubscription) -> String {
    format!(
        "<member>{}{}{}{}{}</member>",
        el("SubscriptionArn", &s.arn),
        el("TopicArn", &s.topic_arn),
        el("Protocol", &s.protocol),
        el("Endpoint", &s.endpoint),
        el("Owner", crate::aws::ACCOUNT_ID),
    )
}

fn next_token_xml(token: Option<String>) -> String {
    token.map(|t| el("NextToken", &t)).unwrap_or_default()
}

async fn emit(state: &Arc<AppState>, action: &str, payload: Value) {
    telemetry::emit(&state.db, "aws", "sns", action, &payload.to_string()).await;
}

// ── Dispatch ─────────────────────────────────────────────────────────────────

pub async fn handle_query(
    state: &Arc<AppState>,
    action: &str,
    params: &HashMap<String, String>,
) -> Response {
    match dispatch(state, action, params).await {
        Ok(r) => r,
        Err(e) => xml_error_response(
            StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_REQUEST),
            e.code,
            &e.message,
        ),
    }
}

fn publish_request(v: &Value, topic_arn: Option<String>) -> Result<PublishRequest> {
    Ok(PublishRequest {
        topic_arn,
        phone_number: None,
        message: str_of(v, "Message").unwrap_or_default(),
        subject: str_of(v, "Subject"),
        message_structure: str_of(v, "MessageStructure"),
        attributes: message_attributes(as_array(
            v.get("MessageAttributes").cloned().unwrap_or(Value::Null),
        ))?,
        group_id: str_of(v, "MessageGroupId"),
        dedup_id: str_of(v, "MessageDeduplicationId"),
    })
}

fn valid_batch_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

async fn dispatch(
    state: &Arc<AppState>,
    action: &str,
    params: &HashMap<String, String>,
) -> Result<Response> {
    let s = svc(state);
    match action {
        "CreateTopic" => {
            let name = required(params, "Name")?;
            let arn = s
                .create_topic(
                    name,
                    entry_map(params, "Attributes"),
                    tag_map(params, "Tags"),
                )
                .await?;
            emit(state, "create_topic", json!({"topic_arn": arn})).await;
            Ok(respond(action, &el("TopicArn", &arn)))
        }
        "ListTopics" => {
            let topics = s.list_topics().await?;
            let (items, token) = page(
                &topics,
                |t| t.arn.as_str(),
                decode_token(param(params, "NextToken"))?,
                PAGE_SIZE,
            );
            let members: String = items
                .iter()
                .map(|t| format!("<member>{}</member>", el("TopicArn", &t.arn)))
                .collect();
            Ok(respond(
                action,
                &format!("<Topics>{members}</Topics>{}", next_token_xml(token)),
            ))
        }
        "DeleteTopic" => {
            let arn = required(params, "TopicArn")?;
            s.delete_topic(arn).await?;
            emit(state, "delete_topic", json!({"topic_arn": arn})).await;
            Ok(respond(action, ""))
        }
        "GetTopicAttributes" => {
            let arn = required(params, "TopicArn")?;
            let attrs = s.topic_attributes(arn).await?;
            Ok(respond(action, &attributes_xml(&attrs)))
        }
        "SetTopicAttributes" => {
            let arn = required(params, "TopicArn")?;
            let name = required(params, "AttributeName")?;
            let value = param(params, "AttributeValue").unwrap_or("");
            s.set_topic_attribute(arn, name, value).await?;
            emit(
                state,
                "set_topic_attributes",
                json!({"topic_arn": arn, "attribute": name}),
            )
            .await;
            Ok(respond(action, ""))
        }
        "Subscribe" => {
            let topic_arn = required(params, "TopicArn")?;
            let protocol = required(params, "Protocol")?;
            let endpoint = param(params, "Endpoint").unwrap_or("");
            let sub_arn = s
                .subscribe(
                    topic_arn,
                    protocol,
                    endpoint,
                    entry_map(params, "Attributes"),
                )
                .await?;
            emit(state, "subscribe", json!({"topic_arn": topic_arn, "protocol": protocol, "endpoint": endpoint, "subscription_arn": sub_arn})).await;
            Ok(respond(action, &el("SubscriptionArn", &sub_arn)))
        }
        "Unsubscribe" => {
            let arn = required(params, "SubscriptionArn")?;
            s.unsubscribe(arn).await?;
            emit(state, "unsubscribe", json!({"subscription_arn": arn})).await;
            Ok(respond(action, ""))
        }
        "ListSubscriptions" | "ListSubscriptionsByTopic" => {
            let topic = if action == "ListSubscriptionsByTopic" {
                let arn = required(params, "TopicArn")?;
                s.get_topic(arn).await?;
                Some(arn)
            } else {
                None
            };
            let subs = s.list_subscriptions(topic).await?;
            let (items, token) = page(
                &subs,
                |x| x.arn.as_str(),
                decode_token(param(params, "NextToken"))?,
                PAGE_SIZE,
            );
            let members: String = items.iter().map(subscription_member).collect();
            Ok(respond(
                action,
                &format!(
                    "<Subscriptions>{members}</Subscriptions>{}",
                    next_token_xml(token)
                ),
            ))
        }
        "GetSubscriptionAttributes" => {
            let arn = required(params, "SubscriptionArn")?;
            let sub = s.get_subscription(arn).await?;
            Ok(respond(
                action,
                &attributes_xml(&SnsService::subscription_attributes(&sub)),
            ))
        }
        "SetSubscriptionAttributes" => {
            let arn = required(params, "SubscriptionArn")?;
            let name = required(params, "AttributeName")?;
            let value = param(params, "AttributeValue").unwrap_or("");
            s.set_subscription_attribute(arn, name, value).await?;
            emit(
                state,
                "set_subscription_attributes",
                json!({"subscription_arn": arn, "attribute": name}),
            )
            .await;
            Ok(respond(action, ""))
        }
        "Publish" => {
            let topic = param(params, "TopicArn")
                .or_else(|| param(params, "TargetArn"))
                .filter(|t| !t.is_empty())
                .map(String::from);
            let as_json = serde_json::to_value(params).unwrap_or(Value::Null);
            let mut req = publish_request(&as_json, topic)?;
            req.attributes =
                message_attributes(as_array(query_to_json(params, "MessageAttributes")))?;
            req.phone_number = param(params, "PhoneNumber").map(String::from);
            let topic_arn = req.topic_arn.clone();
            let res = delivery::publish(state, req).await?;
            emit(
                state,
                "publish",
                json!({"topic_arn": topic_arn, "message_id": res.message_id}),
            )
            .await;
            let mut inner = el("MessageId", &res.message_id);
            if let Some(seq) = &res.sequence_number {
                inner.push_str(&el("SequenceNumber", seq));
            }
            Ok(respond(action, &inner))
        }
        "PublishBatch" => {
            let topic_arn = required(params, "TopicArn")?.to_string();
            let entries = as_array(query_to_json(params, "PublishBatchRequestEntries"));
            s.get_topic(&topic_arn).await?;
            if entries.is_empty() {
                return Err(SnsError::batch(
                    "EmptyBatchRequest",
                    "The batch request doesn't contain any entries",
                ));
            }
            if entries.len() > 10 {
                return Err(SnsError::batch(
                    "TooManyEntriesInBatchRequest",
                    "The batch request contains more entries than permissible",
                ));
            }
            let mut seen = HashSet::new();
            for e in &entries {
                let id = str_of(e, "Id").unwrap_or_default();
                if !valid_batch_id(&id) {
                    return Err(SnsError::batch(
                        "InvalidBatchEntryId",
                        "The Id of a batch entry in a batch request doesn't abide by the specification",
                    ));
                }
                if !seen.insert(id) {
                    return Err(SnsError::batch(
                        "BatchEntryIdsNotDistinct",
                        "Two or more batch entries in the request have the same Id",
                    ));
                }
            }
            let total: usize = entries
                .iter()
                .map(|e| str_of(e, "Message").map(|m| m.len()).unwrap_or(0))
                .sum();
            if total > 262_144 {
                return Err(SnsError::batch(
                    "BatchRequestTooLong",
                    "The length of all the messages put together is more than the limit",
                ));
            }
            let (mut ok, mut failed) = (String::new(), String::new());
            for e in &entries {
                let id = str_of(e, "Id").unwrap_or_default();
                let res = match publish_request(e, Some(topic_arn.clone())) {
                    Ok(req) => delivery::publish(state, req).await,
                    Err(err) => Err(err),
                };
                match res {
                    Ok(r) => {
                        let seq = r
                            .sequence_number
                            .map(|s| el("SequenceNumber", &s))
                            .unwrap_or_default();
                        ok.push_str(&format!(
                            "<member>{}{}{seq}</member>",
                            el("Id", &id),
                            el("MessageId", &r.message_id)
                        ));
                    }
                    Err(err) => failed.push_str(&format!(
                        "<member>{}{}{}<SenderFault>{}</SenderFault></member>",
                        el("Id", &id),
                        el("Code", err.code),
                        el("Message", &err.message),
                        err.status < 500
                    )),
                }
            }
            emit(
                state,
                "publish_batch",
                json!({"topic_arn": topic_arn, "count": entries.len()}),
            )
            .await;
            Ok(respond(
                action,
                &format!("<Successful>{ok}</Successful><Failed>{failed}</Failed>"),
            ))
        }
        "TagResource" => {
            let arn = required(params, "ResourceArn")?;
            let topic = s
                .find_topic(arn)
                .await?
                .ok_or_else(SnsError::resource_not_found)?;
            let mut tags = topic.tags;
            tags.extend(tag_map(params, "Tags"));
            s.set_tags(arn, tags).await?;
            emit(state, "tag_resource", json!({"resource_arn": arn})).await;
            Ok(respond(action, ""))
        }
        "UntagResource" => {
            let arn = required(params, "ResourceArn")?;
            let topic = s
                .find_topic(arn)
                .await?
                .ok_or_else(SnsError::resource_not_found)?;
            let mut tags = topic.tags;
            for k in as_array(query_to_json(params, "TagKeys")) {
                if let Some(k) = k.as_str() {
                    tags.remove(k);
                }
            }
            s.set_tags(arn, tags).await?;
            emit(state, "untag_resource", json!({"resource_arn": arn})).await;
            Ok(respond(action, ""))
        }
        "ListTagsForResource" => {
            let arn = required(params, "ResourceArn")?;
            let topic = s
                .find_topic(arn)
                .await?
                .ok_or_else(SnsError::resource_not_found)?;
            let members: String = topic
                .tags
                .iter()
                .map(|(k, v)| format!("<member>{}{}</member>", el("Key", k), el("Value", v)))
                .collect();
            Ok(respond(action, &format!("<Tags>{members}</Tags>")))
        }
        _ => Err(SnsError::invalid(format!("Unknown SNS action: {action}"))),
    }
}

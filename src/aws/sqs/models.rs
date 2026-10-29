//! SQS queue records and message-attribute values.

use std::collections::BTreeMap;

use base64::Engine;
use md5::{Digest, Md5};
use serde_json::{json, Map, Value};

/// One typed message attribute (`String`, `Number`, `Binary`, or a custom
/// `Type.label` subtype).
#[derive(Debug, Clone, PartialEq)]
pub struct MsgAttr {
    pub data_type: String,
    pub string_value: Option<String>,
    pub binary_value: Option<Vec<u8>>,
}

/// Message attributes keyed (and therefore sorted) by name.
pub type MsgAttrs = BTreeMap<String, MsgAttr>;

impl MsgAttr {
    pub fn is_binary(&self) -> bool {
        self.data_type.starts_with("Binary")
    }

    /// Base type before any `.label` suffix.
    pub fn base_type(&self) -> &str {
        self.data_type.split('.').next().unwrap_or("")
    }

    /// Bytes counted toward the message size limit and hashed for the MD5.
    pub fn value_bytes(&self) -> Vec<u8> {
        if self.is_binary() {
            self.binary_value.clone().unwrap_or_default()
        } else {
            self.string_value.clone().unwrap_or_default().into_bytes()
        }
    }

    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("DataType".into(), json!(self.data_type));
        if let Some(s) = &self.string_value {
            m.insert("StringValue".into(), json!(s));
        }
        if let Some(b) = &self.binary_value {
            m.insert(
                "BinaryValue".into(),
                json!(base64::engine::general_purpose::STANDARD.encode(b)),
            );
        }
        Value::Object(m)
    }
}

pub fn attrs_to_json(attrs: &MsgAttrs) -> Value {
    Value::Object(
        attrs
            .iter()
            .map(|(k, v)| (k.clone(), v.to_json()))
            .collect(),
    )
}

/// Parse and validate attributes in the SQS JSON shape
/// `{"Name": {"DataType": "String", "StringValue": "v"}}`.
pub fn parse_attrs(v: Option<&Value>) -> Result<MsgAttrs, String> {
    let mut out = MsgAttrs::new();
    let Some(v) = v else { return Ok(out) };
    if v.is_null() {
        return Ok(out);
    }
    let obj = v.as_object().ok_or("MessageAttributes must be a map")?;
    for (name, val) in obj {
        if name.is_empty() || name.len() > 256 {
            return Err(format!("Message attribute name '{name}' is invalid."));
        }
        let data_type = val
            .get("DataType")
            .and_then(|d| d.as_str())
            .ok_or_else(|| {
                format!("The message attribute '{name}' must contain a non-empty attribute type.")
            })?
            .to_string();
        let base = data_type.split('.').next().unwrap_or("");
        let string_value = val
            .get("StringValue")
            .and_then(|s| s.as_str())
            .map(String::from);
        let binary_value = match val.get("BinaryValue").and_then(|s| s.as_str()) {
            Some(b) => Some(
                base64::engine::general_purpose::STANDARD
                    .decode(b)
                    .map_err(|_| {
                        format!("The message attribute '{name}' has an invalid binary value.")
                    })?,
            ),
            None => None,
        };
        match base {
            "String" | "Number" => {
                let s = string_value.as_deref().ok_or_else(|| {
                    format!("The message attribute '{name}' with type '{base}' must use field 'String'.")
                })?;
                if base == "Number" && s.trim().parse::<f64>().is_err() {
                    return Err(format!("Can't cast the value of message attribute '{name}' to a number."));
                }
            }
            "Binary" => {
                if binary_value.is_none() {
                    return Err(format!(
                        "The message attribute '{name}' with type 'Binary' must use field 'Binary'."
                    ));
                }
            }
            _ => {
                return Err(format!(
                    "The type of message attribute '{name}' is invalid. You must use only the following supported type prefixes: Binary, Number, String."
                ))
            }
        }
        out.insert(
            name.clone(),
            MsgAttr {
                data_type,
                string_value,
                binary_value,
            },
        );
    }
    Ok(out)
}

/// MD5 over message attributes exactly as SQS computes `MD5OfMessageAttributes`:
/// for each attribute sorted by name, length-prefixed (4-byte big endian) name,
/// length-prefixed data type, a transport byte (1 = string, 2 = binary), and the
/// length-prefixed value bytes.
pub fn md5_of_attrs(attrs: &MsgAttrs) -> String {
    let mut h = Md5::new();
    for (name, a) in attrs {
        let push = |h: &mut Md5, b: &[u8]| {
            h.update((b.len() as u32).to_be_bytes());
            h.update(b);
        };
        push(&mut h, name.as_bytes());
        push(&mut h, a.data_type.as_bytes());
        h.update([if a.is_binary() { 2u8 } else { 1u8 }]);
        push(&mut h, &a.value_bytes());
    }
    format!("{:x}", h.finalize())
}

pub fn md5_hex(b: &[u8]) -> String {
    let mut h = Md5::new();
    h.update(b);
    format!("{:x}", h.finalize())
}

/// A queue row with its configured attributes and tags.
#[derive(Debug, Clone)]
pub struct SqsQueue {
    pub id: i64,
    pub name: String,
    pub attributes: BTreeMap<String, String>,
    pub tags: BTreeMap<String, String>,
    pub created_ts: i64,
    pub modified_ts: i64,
}

impl SqsQueue {
    pub fn arn(&self) -> String {
        queue_arn(&self.name)
    }

    pub fn attr_i64(&self, name: &str) -> i64 {
        self.attributes
            .get(name)
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| default_attr(name).and_then(|d| d.parse().ok()).unwrap_or(0))
    }

    pub fn attr_bool(&self, name: &str) -> bool {
        self.attributes
            .get(name)
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    }

    pub fn is_fifo(&self) -> bool {
        self.attr_bool("FifoQueue")
    }

    /// `(deadLetterTargetArn, maxReceiveCount)` from the RedrivePolicy.
    pub fn redrive(&self) -> Option<(String, i64)> {
        let v: Value = serde_json::from_str(self.attributes.get("RedrivePolicy")?).ok()?;
        let arn = v.get("deadLetterTargetArn")?.as_str()?.to_string();
        let max = v.get("maxReceiveCount").and_then(|m| {
            m.as_i64()
                .or_else(|| m.as_str().and_then(|s| s.parse().ok()))
        })?;
        Some((arn, max))
    }
}

pub fn queue_arn(name: &str) -> String {
    format!(
        "arn:aws:sqs:{}:{}:{name}",
        crate::aws::REGION,
        crate::aws::ACCOUNT_ID
    )
}

/// Defaults for configurable queue attributes (as AWS reports them).
pub fn default_attr(name: &str) -> Option<&'static str> {
    Some(match name {
        "VisibilityTimeout" => "30",
        "DelaySeconds" => "0",
        "MessageRetentionPeriod" => "345600",
        "ReceiveMessageWaitTimeSeconds" => "0",
        "MaximumMessageSize" => "262144",
        "SqsManagedSseEnabled" => "false",
        _ => return None,
    })
}

/// A message returned by ReceiveMessage.
#[derive(Debug, Clone)]
pub struct ReceivedMessage {
    pub message_id: String,
    pub receipt_handle: String,
    pub body: String,
    pub attributes: BTreeMap<String, String>,
    pub message_attributes: MsgAttrs,
}

/// Result of a successful send.
#[derive(Debug, Clone)]
pub struct SendOutcome {
    pub message_id: String,
    pub md5_body: String,
    pub md5_attrs: Option<String>,
    pub md5_system_attrs: Option<String>,
    pub sequence_number: Option<String>,
}

/// Input for a single send.
#[derive(Debug, Clone, Default)]
pub struct SendInput {
    pub body: String,
    pub delay_seconds: Option<i64>,
    pub attributes: MsgAttrs,
    pub system_attributes: MsgAttrs,
    pub group_id: Option<String>,
    pub dedup_id: Option<String>,
}

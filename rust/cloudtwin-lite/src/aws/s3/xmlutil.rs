//! XML, encoding and timestamp helpers for the S3 wire format.

use chrono::{DateTime, NaiveDateTime, Utc};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

pub use crate::proto::xml_escape as esc;

pub const XMLNS: &str = "http://s3.amazonaws.com/doc/2006-03-01/";
pub const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";

/// Characters left as-is by S3 URL encoding (unreserved + `/`).
const KEY_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b'/');

/// `encoding-type=url` encoding of keys and prefixes.
pub fn url_encode(s: &str) -> String {
    utf8_percent_encode(s, KEY_SET).to_string()
}

/// Event-notification key encoding: like `url_encode` with spaces as `+`.
pub fn url_encode_plus(s: &str) -> String {
    s.split(' ').map(url_encode).collect::<Vec<_>>().join("+")
}

pub fn url_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .to_string()
}

pub fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").map(|n| n.and_utc()))
        .unwrap_or_else(|_| Utc::now())
}

/// S3 XML timestamp, e.g. `2024-01-02T03:04:05.000Z`.
pub fn iso(s: &str) -> String {
    parse_ts(s).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// RFC 7231 header date, e.g. `Tue, 02 Jan 2024 03:04:05 GMT`.
pub fn http_date(s: &str) -> String {
    parse_ts(s).format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

pub fn parse_http_date(s: &str) -> Option<DateTime<Utc>> {
    // The weekday is redundant (and often wrong in hand-written headers).
    let rest = s.split_once(',').map_or(s, |(_, r)| r).trim();
    NaiveDateTime::parse_from_str(rest, "%d %b %Y %H:%M:%S GMT")
        .map(|n| n.and_utc())
        .ok()
        .or_else(|| {
            DateTime::parse_from_rfc2822(s)
                .map(|d| d.with_timezone(&Utc))
                .ok()
        })
        .or_else(|| {
            DateTime::parse_from_rfc3339(s)
                .map(|d| d.with_timezone(&Utc))
                .ok()
        })
}

/// Build `<Name>text</Name>` with escaping.
pub fn tag(name: &str, value: &str) -> String {
    format!("<{name}>{}</{name}>", esc(value))
}

/// Convert a JSON value (boto3 request shape) to S3 XML children. Arrays
/// repeat their element with a singular name (`Rules` → `<Rule>`).
pub fn json_to_xml_children(v: &serde_json::Value) -> String {
    let mut out = String::new();
    if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            match val {
                serde_json::Value::Array(items) => {
                    let name = singular(k);
                    for item in items {
                        out.push_str(&format!(
                            "<{name}>{}</{name}>",
                            json_scalar_or_children(item)
                        ));
                    }
                }
                _ => out.push_str(&format!("<{k}>{}</{k}>", json_scalar_or_children(val))),
            }
        }
    }
    out
}

fn json_scalar_or_children(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Object(_) => json_to_xml_children(v),
        serde_json::Value::String(s) => esc(s),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Null => String::new(),
        serde_json::Value::Array(_) => String::new(),
    }
}

fn singular(k: &str) -> String {
    match k {
        "Tags" | "TagSet" => "Tag".into(),
        _ => k.strip_suffix('s').unwrap_or(k).to_string(),
    }
}

/// Parse `<Tagging><TagSet><Tag><Key/><Value/></Tag>...` into pairs.
pub fn parse_tagging(body: &[u8]) -> Result<Vec<(String, String)>, super::errors::S3Error> {
    use super::errors::S3Error;
    let doc = crate::xml::parse(body).map_err(|_| S3Error::malformed_xml())?;
    if doc.name != "Tagging" {
        return Err(S3Error::malformed_xml());
    }
    let mut tags = Vec::new();
    if let Some(set) = doc.child("TagSet") {
        for t in set.children_named("Tag") {
            let k = t.child_text("Key").ok_or_else(S3Error::malformed_xml)?;
            let v = t.child("Value").map(|v| v.text.clone()).unwrap_or_default();
            if tags.iter().any(|(ek, _): &(String, String)| *ek == k) {
                return Err(S3Error::new(
                    "InvalidTag",
                    "Cannot provide multiple Tags with the same key",
                )
                .with("TagKey", k));
            }
            tags.push((k, v));
        }
    }
    validate_tags(&tags, 50)?;
    Ok(tags)
}

pub fn validate_tags(tags: &[(String, String)], max: usize) -> Result<(), super::errors::S3Error> {
    use super::errors::S3Error;
    if tags.len() > max {
        return Err(S3Error::new(
            "BadRequest",
            format!("Object tags cannot be greater than {max}"),
        ));
    }
    for (k, v) in tags {
        if k.is_empty() || k.chars().count() > 128 {
            return Err(
                S3Error::new("InvalidTag", "The TagKey you have provided is invalid")
                    .with("TagKey", k.clone()),
            );
        }
        if v.chars().count() > 256 {
            return Err(
                S3Error::new("InvalidTag", "The TagValue you have provided is too long")
                    .with("TagKey", k.clone()),
            );
        }
    }
    Ok(())
}

pub fn tagging_xml(tags: &[(String, String)]) -> String {
    let set: String = tags
        .iter()
        .map(|(k, v)| format!("<Tag>{}{}</Tag>", tag("Key", k), tag("Value", v)))
        .collect();
    format!("{XML_DECL}<Tagging xmlns=\"{XMLNS}\"><TagSet>{set}</TagSet></Tagging>")
}

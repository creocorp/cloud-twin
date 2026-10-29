//! Bucket CORS configuration: parsing, rule matching and response headers.

use axum::http::{HeaderMap, HeaderValue};

use super::errors::{S3Error, S3Result};
use super::xmlutil::{esc, XMLNS, XML_DECL};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CorsRule {
    pub id: Option<String>,
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub expose_headers: Vec<String>,
    pub max_age: Option<i64>,
}

const METHODS: &[&str] = &["GET", "PUT", "POST", "DELETE", "HEAD"];

/// Parse and validate a `<CORSConfiguration>` document.
pub fn parse_rules(xml: &[u8]) -> S3Result<Vec<CorsRule>> {
    let doc = crate::xml::parse(xml).map_err(|_| S3Error::malformed_xml())?;
    if doc.name != "CORSConfiguration" {
        return Err(S3Error::malformed_xml());
    }
    let texts = |e: &crate::xml::Element, n: &str| -> Vec<String> {
        e.children_named(n)
            .map(|c| c.text.trim().to_string())
            .collect()
    };
    let mut rules = Vec::new();
    for r in doc.children_named("CORSRule") {
        let rule = CorsRule {
            id: r.child_text("ID"),
            allowed_origins: texts(r, "AllowedOrigin"),
            allowed_methods: texts(r, "AllowedMethod"),
            allowed_headers: texts(r, "AllowedHeader"),
            expose_headers: texts(r, "ExposeHeader"),
            max_age: r.child_text("MaxAgeSeconds").and_then(|s| s.parse().ok()),
        };
        validate(&rule)?;
        rules.push(rule);
    }
    if rules.is_empty() {
        return Err(S3Error::malformed_xml());
    }
    Ok(rules)
}

fn validate(rule: &CorsRule) -> S3Result<()> {
    if rule.allowed_origins.is_empty() || rule.allowed_methods.is_empty() {
        return Err(S3Error::malformed_xml());
    }
    for m in &rule.allowed_methods {
        if !METHODS.contains(&m.as_str()) {
            return Err(S3Error::new(
                "InvalidRequest",
                format!("Found unsupported HTTP method in CORS config. Unsupported method is {m}"),
            ));
        }
    }
    for o in &rule.allowed_origins {
        if o.matches('*').count() > 1 {
            return Err(S3Error::new(
                "InvalidRequest",
                format!("AllowedOrigin \"{o}\" can not have more than one wildcard."),
            ));
        }
    }
    Ok(())
}

/// Build rules from the boto3-style JSON shape used in seed documents.
pub fn rules_from_json(v: &serde_json::Value) -> S3Result<Vec<CorsRule>> {
    let list = |r: &serde_json::Value, n: &str| -> Vec<String> {
        match r.get(n) {
            Some(serde_json::Value::Array(a)) => a
                .iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect(),
            Some(serde_json::Value::String(s)) => vec![s.clone()],
            _ => Vec::new(),
        }
    };
    let items = v
        .as_array()
        .cloned()
        .or_else(|| v.get("CORSRules").and_then(|r| r.as_array().cloned()))
        .unwrap_or_default();
    let mut rules = Vec::new();
    for r in &items {
        let rule = CorsRule {
            id: r.get("ID").and_then(|x| x.as_str()).map(String::from),
            allowed_origins: list(r, "AllowedOrigins"),
            allowed_methods: list(r, "AllowedMethods")
                .into_iter()
                .map(|m| m.to_ascii_uppercase())
                .collect(),
            allowed_headers: list(r, "AllowedHeaders"),
            expose_headers: list(r, "ExposeHeaders"),
            max_age: r.get("MaxAgeSeconds").and_then(|x| x.as_i64()),
        };
        validate(&rule)?;
        rules.push(rule);
    }
    Ok(rules)
}

pub fn rules_to_xml(rules: &[CorsRule]) -> String {
    let mut out = format!("{XML_DECL}<CORSConfiguration xmlns=\"{XMLNS}\">");
    for r in rules {
        out.push_str("<CORSRule>");
        if let Some(id) = &r.id {
            out.push_str(&format!("<ID>{}</ID>", esc(id)));
        }
        for (name, vals) in [
            ("AllowedHeader", &r.allowed_headers),
            ("AllowedMethod", &r.allowed_methods),
            ("AllowedOrigin", &r.allowed_origins),
            ("ExposeHeader", &r.expose_headers),
        ] {
            for v in vals {
                out.push_str(&format!("<{name}>{}</{name}>", esc(v)));
            }
        }
        if let Some(m) = r.max_age {
            out.push_str(&format!("<MaxAgeSeconds>{m}</MaxAgeSeconds>"));
        }
        out.push_str("</CORSRule>");
    }
    out.push_str("</CORSConfiguration>");
    out
}

/// Match `value` against a pattern with at most a few `*` wildcards.
pub fn wildcard_match(pattern: &str, value: &str, case_insensitive: bool) -> bool {
    let (p, v) = if case_insensitive {
        (pattern.to_ascii_lowercase(), value.to_ascii_lowercase())
    } else {
        (pattern.to_string(), value.to_string())
    };
    let parts: Vec<&str> = p.split('*').collect();
    if parts.len() == 1 {
        return p == v;
    }
    let mut rest = v.as_str();
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.len() >= part.len() && rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(idx) => rest = &rest[idx + part.len()..],
                None => return false,
            }
        }
    }
    true
}

/// First rule allowing `origin` + `method` (+ all `request_headers`).
pub fn find_rule<'a>(
    rules: &'a [CorsRule],
    origin: &str,
    method: &str,
    request_headers: &[String],
) -> Option<&'a CorsRule> {
    rules.iter().find(|r| {
        r.allowed_origins
            .iter()
            .any(|o| wildcard_match(o, origin, false))
            && r.allowed_methods.iter().any(|m| m == method)
            && request_headers
                .iter()
                .all(|h| r.allowed_headers.iter().any(|a| wildcard_match(a, h, true)))
    })
}

pub fn split_header_list(v: &str) -> Vec<String> {
    v.split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Add CORS response headers for a matched rule.
pub fn apply(
    headers: &mut HeaderMap,
    rule: &CorsRule,
    origin: &str,
    preflight: bool,
    request_headers: &[String],
) {
    let mut set = |name: &'static str, value: String| {
        if let Ok(v) = HeaderValue::from_str(&value) {
            headers.insert(name, v);
        }
    };
    let wildcard = rule.allowed_origins.iter().any(|o| o == "*");
    if wildcard {
        set("access-control-allow-origin", "*".into());
    } else {
        set("access-control-allow-origin", origin.to_string());
        set("access-control-allow-credentials", "true".into());
    }
    set(
        "access-control-allow-methods",
        rule.allowed_methods.join(", "),
    );
    if preflight && !request_headers.is_empty() {
        set("access-control-allow-headers", request_headers.join(", "));
    }
    if !rule.expose_headers.is_empty() {
        set(
            "access-control-expose-headers",
            rule.expose_headers.join(", "),
        );
    }
    if let Some(m) = rule.max_age {
        set("access-control-max-age", m.to_string());
    }
    set(
        "vary",
        "Origin, Access-Control-Request-Headers, Access-Control-Request-Method".into(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*", "http://a.com", false));
        assert!(wildcard_match(
            "http://*.example.com",
            "http://app.example.com",
            false
        ));
        assert!(!wildcard_match(
            "http://*.example.com",
            "http://example.org",
            false
        ));
        assert!(wildcard_match("x-amz-*", "X-Amz-Date", true));
        assert!(!wildcard_match("https://a.com", "https://b.com", false));
    }
}

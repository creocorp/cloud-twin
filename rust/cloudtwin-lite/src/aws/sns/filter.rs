//! SNS subscription filter policies.
//!
//! Supports exact string/number/boolean/null matches, `prefix`, `suffix`,
//! `equals-ignore-case`, `wildcard`, `anything-but` (value, list, or
//! prefix/suffix), `exists`, `numeric` (`=`, `<`, `<=`, `>`, `>=`, ranges), and
//! `$or`, for both `MessageAttributes` and `MessageBody` scopes.

use serde_json::Value;

use crate::aws::sqs::MessageAttributes;

/// Validate a filter policy document (must be a JSON object).
pub fn validate(policy: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_str(policy)
        .map_err(|e| format!("Invalid parameter: FilterPolicy: failed to parse JSON. {e}"))?;
    if !v.is_object() {
        return Err("Invalid parameter: FilterPolicy: Filter policy must be a JSON object".into());
    }
    Ok(v)
}

/// Does a message match `policy` under `scope` (`MessageAttributes` or `MessageBody`)?
pub fn matches(policy: &Value, scope: &str, attrs: &MessageAttributes, body: &str) -> bool {
    if scope == "MessageBody" {
        match serde_json::from_str::<Value>(body) {
            Ok(doc) => match_body(policy, Some(&doc)),
            Err(_) => policy.as_object().map(|o| o.is_empty()).unwrap_or(true),
        }
    } else {
        match_attrs(policy, attrs)
    }
}

fn match_attrs(policy: &Value, attrs: &MessageAttributes) -> bool {
    let Some(obj) = policy.as_object() else {
        return false;
    };
    obj.iter().all(|(key, cond)| {
        if key == "$or" {
            return cond
                .as_array()
                .map(|alts| alts.iter().any(|p| match_attrs(p, attrs)))
                .unwrap_or(false);
        }
        let values = attrs.get(key).and_then(|a| {
            let s = a.string_value.clone()?;
            Some(match a.base_type() {
                "Number" => vec![s
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                    .map(Value::Number)
                    .unwrap_or(Value::String(s))],
                _ if a.data_type == "String.Array" => match serde_json::from_str::<Value>(&s) {
                    Ok(Value::Array(items)) => items,
                    _ => vec![Value::String(s)],
                },
                _ => vec![Value::String(s)],
            })
        });
        match cond {
            Value::Array(conds) => match_conditions(conds, values.as_deref()),
            other => match_conditions(std::slice::from_ref(other), values.as_deref()),
        }
    })
}

fn match_body(policy: &Value, doc: Option<&Value>) -> bool {
    let Some(obj) = policy.as_object() else {
        return false;
    };
    obj.iter().all(|(key, cond)| {
        if key == "$or" {
            return cond
                .as_array()
                .map(|alts| alts.iter().any(|p| match_body(p, doc)))
                .unwrap_or(false);
        }
        let field = doc.and_then(|d| d.get(key));
        match cond {
            Value::Object(_) => match_body(cond, field),
            Value::Array(conds) => {
                let values: Option<Vec<Value>> = field.map(|f| match f {
                    Value::Array(items) => items.clone(),
                    other => vec![other.clone()],
                });
                match_conditions(conds, values.as_deref())
            }
            other => {
                let values = field.map(|f| vec![f.clone()]);
                match_conditions(std::slice::from_ref(other), values.as_deref())
            }
        }
    })
}

fn match_conditions(conds: &[Value], values: Option<&[Value]>) -> bool {
    conds.iter().any(|c| match_condition(c, values))
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn match_condition(cond: &Value, values: Option<&[Value]>) -> bool {
    match cond {
        Value::Object(o) => {
            if let Some(e) = o.get("exists") {
                let want = e.as_bool().unwrap_or(true);
                return want == values.is_some();
            }
            let Some(values) = values else { return false };
            if let Some(p) = o.get("prefix") {
                return string_op(
                    p,
                    values,
                    |s, p| s.starts_with(p),
                    |s, p| s.to_lowercase().starts_with(&p.to_lowercase()),
                );
            }
            if let Some(p) = o.get("suffix") {
                return string_op(
                    p,
                    values,
                    |s, p| s.ends_with(p),
                    |s, p| s.to_lowercase().ends_with(&p.to_lowercase()),
                );
            }
            if let Some(p) = o.get("equals-ignore-case").and_then(|p| p.as_str()) {
                return values.iter().any(|v| {
                    v.as_str()
                        .map(|s| s.eq_ignore_ascii_case(p))
                        .unwrap_or(false)
                });
            }
            if let Some(p) = o.get("wildcard").and_then(|p| p.as_str()) {
                return values
                    .iter()
                    .any(|v| v.as_str().map(|s| wildcard(p, s)).unwrap_or(false));
            }
            if let Some(ab) = o.get("anything-but") {
                return values.iter().all(|v| !anything_but_excludes(ab, v));
            }
            if let Some(Value::Array(ops)) = o.get("numeric") {
                return values
                    .iter()
                    .any(|v| num(v).map(|n| numeric(ops, n)).unwrap_or(false));
            }
            false
        }
        Value::Number(n) => {
            let Some(values) = values else { return false };
            let target = n.as_f64().unwrap_or(f64::NAN);
            values.iter().any(|v| num(v) == Some(target))
        }
        Value::Null => values
            .map(|vs| vs.iter().any(|v| v.is_null()))
            .unwrap_or(false),
        other => values
            .map(|vs| vs.iter().any(|v| v == other))
            .unwrap_or(false),
    }
}

/// `prefix`/`suffix` take a string or `{"equals-ignore-case": "..."}`.
fn string_op(
    p: &Value,
    values: &[Value],
    exact: impl Fn(&str, &str) -> bool,
    ignore_case: impl Fn(&str, &str) -> bool,
) -> bool {
    if let Some(p) = p.as_str() {
        return values
            .iter()
            .any(|v| v.as_str().map(|s| exact(s, p)).unwrap_or(false));
    }
    if let Some(p) = p.get("equals-ignore-case").and_then(|p| p.as_str()) {
        return values
            .iter()
            .any(|v| v.as_str().map(|s| ignore_case(s, p)).unwrap_or(false));
    }
    false
}

fn anything_but_excludes(ab: &Value, v: &Value) -> bool {
    match ab {
        Value::Array(items) => items.iter().any(|i| anything_but_excludes(i, v)),
        Value::Object(o) => {
            let s = v.as_str().unwrap_or("");
            if let Some(p) = o.get("prefix").and_then(|p| p.as_str()) {
                return v.is_string() && s.starts_with(p);
            }
            if let Some(p) = o.get("suffix").and_then(|p| p.as_str()) {
                return v.is_string() && s.ends_with(p);
            }
            if let Some(p) = o.get("equals-ignore-case") {
                let list: Vec<&Value> = match p {
                    Value::Array(a) => a.iter().collect(),
                    other => vec![other],
                };
                return list.iter().any(|x| {
                    x.as_str()
                        .map(|x| x.eq_ignore_ascii_case(s))
                        .unwrap_or(false)
                });
            }
            false
        }
        Value::Number(n) => num(v) == n.as_f64(),
        other => v == other,
    }
}

fn numeric(ops: &[Value], n: f64) -> bool {
    let mut i = 0;
    let mut matched_any = false;
    while i + 1 < ops.len() {
        let (Some(op), Some(rhs)) = (ops[i].as_str(), ops[i + 1].as_f64()) else {
            return false;
        };
        let ok = match op {
            "=" => n == rhs,
            "<" => n < rhs,
            "<=" => n <= rhs,
            ">" => n > rhs,
            ">=" => n >= rhs,
            _ => false,
        };
        if !ok {
            return false;
        }
        matched_any = true;
        i += 2;
    }
    matched_any
}

/// Glob match where `*` matches any run of characters.
fn wildcard(pattern: &str, s: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == s;
    }
    let mut rest = s;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else if let Some(pos) = rest.find(part) {
            rest = &rest[pos + part.len()..];
        } else {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod unit {
    use super::*;
    use crate::aws::sqs::MsgAttr;
    use serde_json::json;

    fn attrs(items: &[(&str, &str, &str)]) -> MessageAttributes {
        items
            .iter()
            .map(|(k, t, v)| {
                (
                    k.to_string(),
                    MsgAttr {
                        data_type: t.to_string(),
                        string_value: Some(v.to_string()),
                        binary_value: None,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn attribute_policies() {
        let a = attrs(&[
            ("color", "String", "red"),
            ("price", "Number", "15"),
            ("tags", "String.Array", "[\"a\",\"b\"]"),
        ]);
        let m = |p: Value| matches(&p, "MessageAttributes", &a, "");
        assert!(m(json!({"color": ["red", "blue"]})));
        assert!(!m(json!({"color": ["blue"]})));
        assert!(m(json!({"color": [{"prefix": "re"}]})));
        assert!(m(json!({"color": [{"suffix": "ed"}]})));
        assert!(m(json!({"color": [{"anything-but": ["blue", "green"]}]})));
        assert!(!m(json!({"color": [{"anything-but": "red"}]})));
        assert!(m(json!({"color": [{"anything-but": {"prefix": "bl"}}]})));
        assert!(m(json!({"price": [{"numeric": [">", 10, "<=", 20]}]})));
        assert!(!m(json!({"price": [{"numeric": ["<", 10]}]})));
        assert!(m(json!({"price": [15]})));
        assert!(m(json!({"missing": [{"exists": false}]})));
        assert!(!m(json!({"missing": [{"exists": true}]})));
        assert!(!m(json!({"missing": [{"anything-but": "x"}]})));
        assert!(m(json!({"tags": ["b"]})));
        assert!(m(
            json!({"color": ["red"], "price": [{"numeric": ["=", 15]}]})
        ));
        assert!(!m(
            json!({"color": ["red"], "price": [{"numeric": ["=", 16]}]})
        ));
        assert!(m(json!({"$or": [{"color": ["blue"]}, {"price": [15]}]})));
        assert!(m(json!({"color": [{"wildcard": "r*d"}]})));
        assert!(m(json!({"color": [{"equals-ignore-case": "RED"}]})));
    }

    #[test]
    fn body_policies() {
        let body =
            r#"{"order": {"status": "shipped", "total": 42, "items": ["x", "y"]}, "flag": true}"#;
        let m = |p: Value| matches(&p, "MessageBody", &MessageAttributes::new(), body);
        assert!(m(json!({"order": {"status": ["shipped"]}})));
        assert!(!m(json!({"order": {"status": ["pending"]}})));
        assert!(m(
            json!({"order": {"total": [{"numeric": [">=", 40]}]}, "flag": [true]})
        ));
        assert!(m(json!({"order": {"items": ["y"]}})));
        assert!(m(json!({"order": {"missing": [{"exists": false}]}})));
        assert!(!matches(
            &json!({"a": ["b"]}),
            "MessageBody",
            &MessageAttributes::new(),
            "not json"
        ));
    }
}

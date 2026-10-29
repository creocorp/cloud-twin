//! DynamoDB `Query` key-condition evaluation.
//!
//! Supports the KeyConditionExpression grammar AWS allows: a required
//! partition-key equality, optionally `AND` one sort-key condition
//! (`=`, `<`, `<=`, `>`, `>=`, `BETWEEN :a AND :b`, `begins_with(sk, :v)`).
//! Attribute names may be `#placeholders`.

use std::cmp::Ordering;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum SortCond {
    Cmp(String, Value),
    Between(Value, Value),
    BeginsWith(Value),
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyCondition {
    pub pk_name: String,
    pub pk_value: Value,
    pub sort: Option<(String, SortCond)>,
}

fn resolve_name(token: &str, names: &Value) -> String {
    if token.starts_with('#') {
        names
            .get(token)
            .and_then(|v| v.as_str())
            .unwrap_or(token)
            .to_string()
    } else {
        token.to_string()
    }
}

fn resolve_value(token: &str, values: &Value) -> Result<Value, String> {
    values.get(token.trim()).cloned().ok_or_else(|| {
        format!(
            "Value provided in ExpressionAttributeValues unused in expressions or missing: {}",
            token.trim()
        )
    })
}

/// Split on a case-insensitive ` AND ` that is not part of `BETWEEN x AND y`.
fn split_and(expr: &str) -> Vec<String> {
    let upper = expr.to_ascii_uppercase();
    let mut parts = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let mut pending_between = false;
    while let Some(pos) = upper[i..].find(" AND ") {
        let at = i + pos;
        if upper[start..at].contains(" BETWEEN ") && !pending_between {
            pending_between = true;
            i = at + 5;
            continue;
        }
        parts.push(expr[start..at].trim().to_string());
        start = at + 5;
        i = start;
        pending_between = false;
    }
    parts.push(expr[start..].trim().to_string());
    parts
}

fn parse_condition(
    part: &str,
    names: &Value,
    values: &Value,
) -> Result<(String, SortCond), String> {
    let mut p = part.trim();
    while p.starts_with('(') && p.ends_with(')') {
        p = p[1..p.len() - 1].trim();
    }
    let lower = p.to_ascii_lowercase();
    if lower.starts_with("begins_with") {
        let inner = p
            .split_once('(')
            .and_then(|(_, r)| r.rsplit_once(')'))
            .map(|(i, _)| i)
            .ok_or("Invalid begins_with condition")?;
        let (name, val) = inner
            .split_once(',')
            .ok_or("Invalid begins_with condition")?;
        return Ok((
            resolve_name(name.trim(), names),
            SortCond::BeginsWith(resolve_value(val, values)?),
        ));
    }
    let upper = p.to_ascii_uppercase();
    if let Some(bpos) = upper.find(" BETWEEN ") {
        let name = resolve_name(p[..bpos].trim(), names);
        let rest = &p[bpos + 9..];
        let apos = rest
            .to_ascii_uppercase()
            .find(" AND ")
            .ok_or("Invalid BETWEEN condition")?;
        let lo = resolve_value(&rest[..apos], values)?;
        let hi = resolve_value(&rest[apos + 5..], values)?;
        return Ok((name, SortCond::Between(lo, hi)));
    }
    for op in ["<=", ">=", "<", ">", "="] {
        if let Some(pos) = p.find(op) {
            let name = resolve_name(p[..pos].trim(), names);
            let val = resolve_value(&p[pos + op.len()..], values)?;
            return Ok((name, SortCond::Cmp(op.to_string(), val)));
        }
    }
    Err(format!("Invalid KeyConditionExpression: {part}"))
}

/// Parse a KeyConditionExpression against the table's key names.
pub fn parse(
    expr: &str,
    names: &Value,
    values: &Value,
    hash_key: &str,
) -> Result<KeyCondition, String> {
    let parts = split_and(expr);
    if parts.is_empty() || parts.len() > 2 {
        return Err("Invalid KeyConditionExpression".into());
    }
    let mut pk: Option<Value> = None;
    let mut sort = None;
    for part in parts {
        let (name, cond) = parse_condition(&part, names, values)?;
        match cond {
            SortCond::Cmp(op, v) if op == "=" && name == hash_key && pk.is_none() => pk = Some(v),
            other => sort = Some((name, other)),
        }
    }
    let pk_value =
        pk.ok_or_else(|| format!("Query condition missed key schema element: {hash_key}"))?;
    Ok(KeyCondition {
        pk_name: hash_key.to_string(),
        pk_value,
        sort,
    })
}

/// Order two typed attribute values (`{"S": ..}`, `{"N": ..}`, `{"B": ..}`).
pub fn compare(a: &Value, b: &Value) -> Option<Ordering> {
    if let (Some(x), Some(y)) = (
        a.get("N").and_then(|v| v.as_str()),
        b.get("N").and_then(|v| v.as_str()),
    ) {
        return x.parse::<f64>().ok()?.partial_cmp(&y.parse::<f64>().ok()?);
    }
    for t in ["S", "B"] {
        if let (Some(x), Some(y)) = (
            a.get(t).and_then(|v| v.as_str()),
            b.get(t).and_then(|v| v.as_str()),
        ) {
            return Some(x.as_bytes().cmp(y.as_bytes()));
        }
    }
    None
}

impl KeyCondition {
    pub fn matches(&self, item: &Value) -> bool {
        if item.get(&self.pk_name) != Some(&self.pk_value) {
            return false;
        }
        let Some((name, cond)) = &self.sort else {
            return true;
        };
        let Some(v) = item.get(name) else {
            return false;
        };
        match cond {
            SortCond::Cmp(op, rhs) => match compare(v, rhs) {
                Some(o) => match op.as_str() {
                    "=" => o == Ordering::Equal,
                    "<" => o == Ordering::Less,
                    "<=" => o != Ordering::Greater,
                    ">" => o == Ordering::Greater,
                    ">=" => o != Ordering::Less,
                    _ => false,
                },
                None => false,
            },
            SortCond::Between(lo, hi) => {
                matches!(compare(v, lo), Some(Ordering::Greater | Ordering::Equal))
                    && matches!(compare(v, hi), Some(Ordering::Less | Ordering::Equal))
            }
            SortCond::BeginsWith(prefix) => {
                let s = |x: &Value, t: &str| x.get(t).and_then(|v| v.as_str()).map(String::from);
                match (s(v, "S").or(s(v, "B")), s(prefix, "S").or(s(prefix, "B"))) {
                    (Some(a), Some(b)) => a.starts_with(&b),
                    _ => false,
                }
            }
        }
    }

    pub fn sort_key(&self) -> Option<&str> {
        self.sort.as_ref().map(|(n, _)| n.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_and_matches_key_conditions() {
        let values =
            json!({":p": {"S": "u1"}, ":lo": {"N": "2"}, ":hi": {"N": "5"}, ":pre": {"S": "ord#"}});
        let names = json!({"#pk": "pk", "#sk": "sk"});
        let item = |sk: Value| json!({"pk": {"S": "u1"}, "sk": sk});

        let c = parse(
            "#pk = :p AND #sk BETWEEN :lo AND :hi",
            &names,
            &values,
            "pk",
        )
        .unwrap();
        assert!(c.matches(&item(json!({"N": "3"}))));
        assert!(!c.matches(&item(json!({"N": "6"}))));

        let c = parse("pk = :p and begins_with(sk, :pre)", &names, &values, "pk").unwrap();
        assert!(c.matches(&item(json!({"S": "ord#1"}))));
        assert!(!c.matches(&item(json!({"S": "inv#1"}))));

        let c = parse("sk >= :lo AND pk = :p", &names, &values, "pk").unwrap();
        assert!(c.matches(&item(json!({"N": "10"}))));
        assert!(!c.matches(&json!({"pk": {"S": "u2"}, "sk": {"N": "10"}})));

        assert!(parse("sk = :lo", &names, &values, "pk").is_err());
        assert!(parse("pk = :missing", &names, &values, "pk").is_err());
    }
}

use anyhow::{bail, Result};
use chrono::Utc;
use rusqlite::OptionalExtension;

use crate::db::Database;
use super::models::{DynamoItem, DynamoTable};

const ACCOUNT_ID: &str = "000000000000";
const REGION:     &str = "us-east-1";

pub struct DynamoDBService {
    db: Database,
}

impl DynamoDBService {
    pub fn new(db: Database) -> Self { DynamoDBService { db } }

    fn table_arn(name: &str) -> String {
        format!("arn:aws:dynamodb:{REGION}:{ACCOUNT_ID}:table/{name}")
    }

    // ── Tables ────────────────────────────────────────────────────────────────

    pub async fn create_table(
        &self,
        name: &str,
        key_schema: &str,
        attr_defs: &str,
    ) -> Result<DynamoTable> {
        let existing = self.get_table(name).await?;
        if let Some(t) = existing { return Ok(t); }

        let n   = name.to_string();
        let ks  = key_schema.to_string();
        let ad  = attr_defs.to_string();
        let now = Utc::now().to_rfc3339();
        self.db.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO dynamo_tables (name, key_schema, attribute_definitions, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![n, ks, ad, now],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        self.get_table(name).await?.ok_or_else(|| anyhow::anyhow!("create_table: not found after insert"))
    }

    pub async fn get_table(&self, name: &str) -> Result<Option<DynamoTable>> {
        let n = name.to_string();
        self.db.conn.call(move |conn| {
            Ok(conn.query_row(
                "SELECT name, key_schema, attribute_definitions, created_at
                 FROM dynamo_tables WHERE name = ?1",
                rusqlite::params![n],
                |r| Ok(DynamoTable {
                    name:                  r.get(0)?,
                    key_schema:            r.get(1)?,
                    attribute_definitions: r.get(2)?,
                    created_at:            r.get(3)?,
                }),
            ).optional()?)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn list_tables(&self) -> Result<Vec<String>> {
        self.db.conn.call(|conn| {
            let mut stmt = conn.prepare("SELECT name FROM dynamo_tables ORDER BY name")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn delete_table(&self, name: &str) -> Result<()> {
        let n = name.to_string();
        let n2 = n.clone();
        self.db.conn.call(move |conn| {
            conn.execute("DELETE FROM dynamo_items WHERE table_name = ?1",  rusqlite::params![n])?;
            conn.execute("DELETE FROM dynamo_tables WHERE name = ?1",        rusqlite::params![n2])?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub fn table_to_json(&self, t: &DynamoTable) -> serde_json::Value {
        let ks: serde_json::Value  = serde_json::from_str(&t.key_schema).unwrap_or_default();
        let ad: serde_json::Value  = serde_json::from_str(&t.attribute_definitions).unwrap_or_default();
        serde_json::json!({
            "TableName":              t.name,
            "TableArn":               Self::table_arn(&t.name),
            "TableStatus":            "ACTIVE",
            "TableSizeBytes":         0,
            "ItemCount":              0,
            "CreationDateTime":       t.created_at,
            "KeySchema":              ks,
            "AttributeDefinitions":   ad,
            "ProvisionedThroughput":  { "ReadCapacityUnits": 5, "WriteCapacityUnits": 5 },
        })
    }

    // ── Items ─────────────────────────────────────────────────────────────────

    fn extract_key(key_schema: &str, item: &serde_json::Value) -> (String, String) {
        let schema: Vec<serde_json::Value> = serde_json::from_str(key_schema).unwrap_or_default();
        let pk_name = schema.iter()
            .find(|k| k.get("KeyType").and_then(|v| v.as_str()) == Some("HASH"))
            .and_then(|k| k.get("AttributeName")).and_then(|v| v.as_str())
            .unwrap_or("pk");
        let sk_name = schema.iter()
            .find(|k| k.get("KeyType").and_then(|v| v.as_str()) == Some("RANGE"))
            .and_then(|k| k.get("AttributeName")).and_then(|v| v.as_str());
        let pk_val = item.get(pk_name).map(|v| v.to_string()).unwrap_or_default();
        let sk_val = sk_name.and_then(|n| item.get(n)).map(|v| v.to_string()).unwrap_or_default();
        (pk_val, sk_val)
    }

    pub async fn put_item(&self, table_name: &str, item: serde_json::Value) -> Result<()> {
        let table = self.get_table(table_name).await?
            .ok_or_else(|| anyhow::anyhow!("ResourceNotFoundException: table {table_name} not found"))?;
        let (pk, sk) = Self::extract_key(&table.key_schema, &item);
        let tn   = table_name.to_string();
        let item_str = item.to_string();
        let now  = Utc::now().to_rfc3339();
        self.db.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO dynamo_items (table_name, pk, sk, item, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(table_name, pk, sk) DO UPDATE SET item=excluded.item",
                rusqlite::params![tn, pk, sk, item_str, now],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn get_item(&self, table_name: &str, key: &serde_json::Value) -> Result<Option<serde_json::Value>> {
        let table = self.get_table(table_name).await?
            .ok_or_else(|| anyhow::anyhow!("ResourceNotFoundException: table {table_name} not found"))?;
        let (pk, sk) = Self::extract_key(&table.key_schema, key);
        let tn = table_name.to_string();
        let item_str: Option<String> = self.db.conn.call(move |conn| {
            Ok(conn.query_row(
                "SELECT item FROM dynamo_items WHERE table_name=?1 AND pk=?2 AND sk=?3",
                rusqlite::params![tn, pk, sk],
                |r| r.get(0),
            ).optional()?)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(item_str.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)))
    }

    pub async fn delete_item(&self, table_name: &str, key: &serde_json::Value) -> Result<()> {
        let table = self.get_table(table_name).await?
            .ok_or_else(|| anyhow::anyhow!("ResourceNotFoundException: table {table_name} not found"))?;
        let (pk, sk) = Self::extract_key(&table.key_schema, key);
        let tn = table_name.to_string();
        self.db.conn.call(move |conn| {
            conn.execute(
                "DELETE FROM dynamo_items WHERE table_name=?1 AND pk=?2 AND sk=?3",
                rusqlite::params![tn, pk, sk],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn scan(&self, table_name: &str, limit: Option<i64>) -> Result<Vec<serde_json::Value>> {
        let tn  = table_name.to_string();
        let lim = limit.unwrap_or(1000);
        let rows: Vec<String> = self.db.conn.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT item FROM dynamo_items WHERE table_name=?1 ORDER BY id LIMIT ?2"
            )?;
            let rows = stmt.query_map(rusqlite::params![tn, lim], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(rows.iter().filter_map(|s| serde_json::from_str(s).ok()).collect())
    }

    pub async fn update_item(
        &self,
        table_name: &str,
        key: &serde_json::Value,
        update_expression: Option<&str>,
        expression_attr_values: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let existing = self.get_item(table_name, key).await?;
        let mut item: serde_json::Value = existing.unwrap_or_else(|| key.clone());

        // Simple SET handling: "SET #attr = :val, ..."
        if let (Some(expr), Some(vals)) = (update_expression, expression_attr_values) {
            let expr = expr.trim();
            if let Some(rest) = expr.strip_prefix("SET ").or_else(|| expr.strip_prefix("set ")) {
                for part in rest.split(',') {
                    let parts: Vec<&str> = part.splitn(2, '=').collect();
                    if parts.len() == 2 {
                        let attr  = parts[0].trim().trim_start_matches('#');
                        let value = parts[1].trim();
                        if let Some(v) = vals.get(value) {
                            if let Some(obj) = item.as_object_mut() {
                                obj.insert(attr.to_string(), v.clone());
                            }
                        }
                    }
                }
            }
        }

        self.put_item(table_name, item.clone()).await?;
        Ok(item)
    }

    pub async fn batch_write(&self, request_items: &serde_json::Value) -> Result<()> {
        if let Some(obj) = request_items.as_object() {
            for (table_name, requests) in obj {
                if let Some(arr) = requests.as_array() {
                    for req in arr {
                        if let Some(put) = req.get("PutRequest").and_then(|r| r.get("Item")) {
                            self.put_item(table_name, put.clone()).await?;
                        }
                        if let Some(del) = req.get("DeleteRequest").and_then(|r| r.get("Key")) {
                            self.delete_item(table_name, del).await?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn batch_get(&self, request_items: &serde_json::Value) -> Result<serde_json::Value> {
        let mut responses = serde_json::Map::new();
        if let Some(obj) = request_items.as_object() {
            for (table_name, req) in obj {
                let mut items = Vec::new();
                if let Some(keys) = req.get("Keys").and_then(|k| k.as_array()) {
                    for key in keys {
                        if let Some(item) = self.get_item(table_name, key).await? {
                            items.push(item);
                        }
                    }
                }
                responses.insert(table_name.clone(), serde_json::Value::Array(items));
            }
        }
        Ok(serde_json::Value::Object(responses))
    }
}

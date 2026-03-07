use anyhow::{bail, Result};
use chrono::Utc;
use md5::{Digest, Md5};
use rusqlite::OptionalExtension;
use uuid::Uuid;

use crate::db::Database;
use super::models::{SqsMessage, SqsQueue};

const ACCOUNT_ID: &str = "000000000000";

pub struct SqsService {
    db:       Database,
    base_url: String,
}

impl SqsService {
    pub fn new(db: Database, base_url: String) -> Self {
        SqsService { db, base_url }
    }

    fn queue_url(&self, name: &str) -> String {
        format!("{}/{}/{}", self.base_url.trim_end_matches('/'), ACCOUNT_ID, name)
    }

    fn name_from_url(url: &str) -> String {
        url.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string()
    }

    pub fn md5_body(s: &str) -> String {
        let mut h = Md5::new();
        h.update(s.as_bytes());
        format!("{:x}", h.finalize())
    }

    // ── Queues ────────────────────────────────────────────────────────────────

    pub async fn create_queue(&self, name: &str) -> Result<String> {
        let n   = name.to_string();
        let url = self.queue_url(name);
        let u   = url.clone();
        let now = Utc::now().to_rfc3339();
        self.db.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO sqs_queues (name, url, created_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(name) DO NOTHING",
                rusqlite::params![n, u, now],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(url)
    }

    pub async fn list_queues(&self, prefix: &str) -> Result<Vec<String>> {
        let p = prefix.to_string();
        self.db.conn.call(move |conn| {
            let mut stmt = conn.prepare("SELECT url, name FROM sqs_queues ORDER BY name")?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows.into_iter().filter(|(_, n)| n.starts_with(&p)).map(|(u, _)| u).collect())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn get_queue_url(&self, name: &str) -> Result<String> {
        let n = name.to_string();
        let result: Option<String> = self.db.conn.call(move |conn| {
            Ok(conn.query_row(
                "SELECT url FROM sqs_queues WHERE name = ?1",
                rusqlite::params![n], |r| r.get(0),
            ).optional()?)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        result.ok_or_else(|| anyhow::anyhow!("QueueDoesNotExist"))
    }

    pub async fn delete_queue(&self, queue_url: &str) -> Result<()> {
        let name = Self::name_from_url(queue_url);
        self.db.conn.call(move |conn| {
            conn.execute("DELETE FROM sqs_queues WHERE name = ?1", rusqlite::params![name])?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    // ── Messages ──────────────────────────────────────────────────────────────

    async fn queue_id(&self, queue_url: &str) -> Result<i64> {
        let name = Self::name_from_url(queue_url);
        let result: Option<i64> = self.db.conn.call(move |conn| {
            Ok(conn.query_row(
                "SELECT id FROM sqs_queues WHERE name = ?1",
                rusqlite::params![name], |r| r.get(0),
            ).optional()?)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        result.ok_or_else(|| anyhow::anyhow!("QueueDoesNotExist"))
    }

    pub async fn send_message(&self, queue_url: &str, body: &str) -> Result<(String, String)> {
        let qid            = self.queue_id(queue_url).await?;
        let message_id     = Uuid::new_v4().to_string();
        let receipt_handle = Uuid::new_v4().to_string();
        let mid            = message_id.clone();
        let rh             = receipt_handle.clone();
        let b              = body.to_string();
        let now            = Utc::now().to_rfc3339();
        self.db.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO sqs_messages (message_id, queue_id, body, receipt_handle, visible, created_at)
                 VALUES (?1, ?2, ?3, ?4, 1, ?5)",
                rusqlite::params![mid, qid, b, rh, now],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok((message_id, receipt_handle))
    }

    pub async fn receive_messages(&self, queue_url: &str, max: i64) -> Result<Vec<SqsMessage>> {
        let qid = self.queue_id(queue_url).await?;
        let lim = max.min(10).max(1);
        let msgs: Vec<SqsMessage> = self.db.conn.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT message_id, queue_id, body, receipt_handle, visible, created_at
                 FROM sqs_messages
                 WHERE queue_id = ?1 AND visible = 1
                 ORDER BY id
                 LIMIT ?2"
            )?;
            let rows = stmt.query_map(rusqlite::params![qid, lim], |r| Ok(SqsMessage {
                message_id:     r.get(0)?,
                queue_id:       r.get(1)?,
                body:           r.get(2)?,
                receipt_handle: r.get(3)?,
                visible:        r.get::<_, i64>(4)? != 0,
                created_at:     r.get(5)?,
            }))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;

        // Mark received messages invisible
        for msg in &msgs {
            let rh = msg.receipt_handle.clone();
            self.db.conn.call(move |conn| {
                conn.execute(
                    "UPDATE sqs_messages SET visible = 0 WHERE receipt_handle = ?1",
                    rusqlite::params![rh],
                )?;
                Ok(())
            }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        Ok(msgs)
    }

    pub async fn delete_message(&self, receipt_handle: &str) -> Result<()> {
        let rh = receipt_handle.to_string();
        self.db.conn.call(move |conn| {
            conn.execute(
                "DELETE FROM sqs_messages WHERE receipt_handle = ?1",
                rusqlite::params![rh],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn change_visibility(&self, receipt_handle: &str, timeout: i64) -> Result<()> {
        let rh      = receipt_handle.to_string();
        let visible = if timeout == 0 { 1i64 } else { 0i64 };
        self.db.conn.call(move |conn| {
            conn.execute(
                "UPDATE sqs_messages SET visible = ?1 WHERE receipt_handle = ?2",
                rusqlite::params![visible, rh],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn get_queue_attributes(&self, queue_url: &str) -> Result<serde_json::Value> {
        let name = Self::name_from_url(queue_url);
        let queue_id = self.queue_id(queue_url).await?;
        let n2   = name.clone();
        let (total, invisible): (i64, i64) = self.db.conn.call(move |conn| {
            let total: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqs_messages WHERE queue_id = ?1",
                rusqlite::params![queue_id], |r| r.get(0),
            )?;
            let invis: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqs_messages WHERE queue_id = ?1 AND visible = 0",
                rusqlite::params![queue_id], |r| r.get(0),
            )?;
            Ok((total, invis))
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(serde_json::json!({
            "Attributes": {
                "ApproximateNumberOfMessages":           (total - invisible).to_string(),
                "ApproximateNumberOfMessagesNotVisible": invisible.to_string(),
                "QueueArn": format!("arn:aws:sqs:us-east-1:{ACCOUNT_ID}:{n2}"),
            }
        }))
    }
}

//! GCP Pub/Sub business logic.
//!
//! This service owns topics, subscriptions, published messages, and the
//! pull/ack lifecycle represented by `ack_id` rows.

use anyhow::{bail, Result};
use chrono::Utc;
use rusqlite::OptionalExtension;
use uuid::Uuid;

use crate::db::Database;
use super::models::{PubsubAckable, PubsubMessage, PubsubSubscription, PubsubTopic};

/// Implements a minimal Pub/Sub runtime over SQLite.
pub struct PubsubService {
    db:      Database,
    project: String,
}

impl PubsubService {
    pub fn new(db: Database, project: String) -> Self {
        PubsubService { db, project }
    }

    // ── Topics ────────────────────────────────────────────────────────────────

    pub async fn create_topic(&self, topic_name: &str) -> Result<PubsubTopic> {
        // topic_name may be fully qualified or short
        let full_name = self.full_topic(topic_name);
        let n         = full_name.clone();
        let now       = Utc::now().to_rfc3339();
        let now2      = now.clone();
        self.db.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO pubsub_topics (name, created_at) VALUES (?1, ?2)
                 ON CONFLICT(name) DO NOTHING",
                rusqlite::params![n, now],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(PubsubTopic { name: full_name, created_at: now2 })
    }

    pub async fn get_topic(&self, topic_name: &str) -> Result<Option<PubsubTopic>> {
        let n = self.full_topic(topic_name);
        self.db.conn.call(move |conn| {
            Ok(conn.query_row(
                "SELECT name, created_at FROM pubsub_topics WHERE name=?1",
                rusqlite::params![n],
                |r| Ok(PubsubTopic { name: r.get(0)?, created_at: r.get(1)? }),
            ).optional()?)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn list_topics(&self) -> Result<Vec<PubsubTopic>> {
        let prefix = format!("projects/{}/topics/", self.project);
        self.db.conn.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT name, created_at FROM pubsub_topics WHERE name LIKE ?1 ORDER BY name",
            )?;
            let like = format!("{prefix}%");
            let rows = stmt.query_map(rusqlite::params![like], |r| Ok(PubsubTopic {
                name: r.get(0)?, created_at: r.get(1)?,
            }))?;
            let topics: Vec<PubsubTopic> = rows.collect::<rusqlite::Result<_>>()?;
            Ok(topics)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn delete_topic(&self, topic_name: &str) -> Result<()> {
        let n = self.full_topic(topic_name);
        let deleted = self.db.conn.call(move |conn| {
            Ok(conn.execute("DELETE FROM pubsub_topics WHERE name=?1", rusqlite::params![n])?)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        if deleted == 0 { bail!("TopicNotFound"); }
        Ok(())
    }

    // ── Subscriptions ─────────────────────────────────────────────────────────

    pub async fn create_subscription(
        &self,
        sub_name: &str,
        topic_name: &str,
    ) -> Result<PubsubSubscription> {
        let full_sub   = self.full_sub(sub_name);
        let full_topic = self.full_topic(topic_name);
        let s          = full_sub.clone();
        let t          = full_topic.clone();
        let now        = Utc::now().to_rfc3339();
        let now2       = now.clone();
        let s2         = s.clone();
        let t2         = t.clone();
        self.db.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO pubsub_subscriptions (name, topic, created_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(name) DO NOTHING",
                rusqlite::params![s, t, now],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(PubsubSubscription { name: s2, topic: t2, created_at: now2 })
    }

    pub async fn get_subscription(&self, sub_name: &str) -> Result<Option<PubsubSubscription>> {
        let n = self.full_sub(sub_name);
        self.db.conn.call(move |conn| {
            Ok(conn.query_row(
                "SELECT name, topic, created_at FROM pubsub_subscriptions WHERE name=?1",
                rusqlite::params![n],
                |r| Ok(PubsubSubscription { name: r.get(0)?, topic: r.get(1)?, created_at: r.get(2)? }),
            ).optional()?)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn list_subscriptions(&self) -> Result<Vec<PubsubSubscription>> {
        let prefix = format!("projects/{}/subscriptions/", self.project);
        self.db.conn.call(move |conn| {
            let like = format!("{prefix}%");
            let mut stmt = conn.prepare(
                "SELECT name, topic, created_at FROM pubsub_subscriptions WHERE name LIKE ?1 ORDER BY name",
            )?;
            let rows = stmt.query_map(rusqlite::params![like], |r| Ok(PubsubSubscription {
                name: r.get(0)?, topic: r.get(1)?, created_at: r.get(2)?,
            }))?;
            let subs: Vec<PubsubSubscription> = rows.collect::<rusqlite::Result<_>>()?;
            Ok(subs)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn delete_subscription(&self, sub_name: &str) -> Result<()> {
        let n = self.full_sub(sub_name);
        self.db.conn.call(move |conn| {
            conn.execute("DELETE FROM pubsub_subscriptions WHERE name=?1", rusqlite::params![n])?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    // ── Publish ───────────────────────────────────────────────────────────────

    /// Publish messages to a topic. Returns list of (message_id, subscription) ackable IDs.
    pub async fn publish(
        &self,
        topic_name: &str,
        messages: Vec<(String, String)>, // (data_b64, attributes_json)
    ) -> Result<Vec<String>> {
        let full_topic = self.full_topic(topic_name);
        let t          = full_topic.clone();
        let now        = Utc::now().to_rfc3339();

        // Pre-generate IDs
        let mut message_ids = Vec::new();
        let mut inserts: Vec<(String, String, String, String, String)> = Vec::new();
        for (data, attrs) in &messages {
            let mid = Uuid::new_v4().to_string();
            message_ids.push(mid.clone());
            inserts.push((mid, t.clone(), data.clone(), attrs.clone(), now.clone()));
        }

        let topic_clone         = t.clone();
        let inserts_clone       = inserts.clone();
        let message_ids_clone   = message_ids.clone();
        let now_clone           = now.clone();

        self.db.conn.call(move |conn| {
            // Insert messages
            for (mid, topic, data, attrs, ts) in &inserts_clone {
                conn.execute(
                    "INSERT INTO pubsub_messages (message_id, topic, data, attributes, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![mid, topic, data, attrs, ts],
                )?;
            }
            // Fan out to subscriptions
            let mut sub_stmt = conn.prepare(
                "SELECT name FROM pubsub_subscriptions WHERE topic=?1",
            )?;
            let subs: Vec<String> = sub_stmt
                .query_map(rusqlite::params![topic_clone], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            for sub in &subs {
                for mid in &message_ids_clone {
                    let ack_id = Uuid::new_v4().to_string();
                    conn.execute(
                        "INSERT INTO pubsub_ackables (ack_id, subscription, message_id, acked, created_at)
                         VALUES (?1, ?2, ?3, 0, ?4)",
                        rusqlite::params![ack_id, sub, mid, now_clone],
                    )?;
                }
            }
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;

        Ok(message_ids)
    }

    // ── Pull ──────────────────────────────────────────────────────────────────

    pub async fn pull(
        &self,
        sub_name: &str,
        max_messages: i64,
    ) -> Result<Vec<(PubsubAckable, PubsubMessage)>> {
        let sub  = self.full_sub(sub_name);
        let max  = if max_messages <= 0 { 10 } else { max_messages.min(1000) };

        self.db.conn.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT a.ack_id, a.subscription, a.message_id, a.acked, a.created_at,
                        m.message_id, m.topic, m.data, m.attributes, m.created_at
                 FROM pubsub_ackables a
                 JOIN pubsub_messages m ON m.message_id = a.message_id
                 WHERE a.subscription=?1 AND a.acked=0
                 ORDER BY m.id LIMIT ?2",
            )?;
            let rows = stmt.query_map(rusqlite::params![sub, max], |r| {
                let ackable = PubsubAckable {
                    ack_id:       r.get(0)?,
                    subscription: r.get(1)?,
                    message_id:   r.get(2)?,
                    acked:        r.get::<_, i64>(3)? != 0,
                    created_at:   r.get(4)?,
                };
                let msg = PubsubMessage {
                    message_id: r.get(5)?,
                    topic:      r.get(6)?,
                    data:       r.get(7)?,
                    attributes: r.get(8)?,
                    created_at: r.get(9)?,
                };
                Ok((ackable, msg))
            })?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    // ── Acknowledge ───────────────────────────────────────────────────────────

    pub async fn acknowledge(&self, sub_name: &str, ack_ids: Vec<String>) -> Result<()> {
        let sub = self.full_sub(sub_name);
        self.db.conn.call(move |conn| {
            for ack_id in &ack_ids {
                conn.execute(
                    "UPDATE pubsub_ackables SET acked=1 WHERE ack_id=?1 AND subscription=?2",
                    rusqlite::params![ack_id, sub],
                )?;
            }
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn full_topic(&self, name: &str) -> String {
        if name.starts_with("projects/") {
            name.to_string()
        } else {
            format!("projects/{}/topics/{}", self.project, name)
        }
    }

    fn full_sub(&self, name: &str) -> String {
        if name.starts_with("projects/") {
            name.to_string()
        } else {
            format!("projects/{}/subscriptions/{}", self.project, name)
        }
    }
}

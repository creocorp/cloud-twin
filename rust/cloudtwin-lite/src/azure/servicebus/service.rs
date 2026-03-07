//! Azure Service Bus business logic.
//!
//! This is the provider-specific service layer for queues, topics,
//! subscriptions, and queue messages.

use anyhow::{bail, Result};
use chrono::Utc;
use rusqlite::OptionalExtension;
use uuid::Uuid;

use crate::db::Database;
use super::models::{AsbMessage, AsbQueue, AsbSubscription, AsbTopic};

/// Implements a simplified Service Bus over SQLite.
pub struct ServiceBusService {
    db:        Database,
    namespace: String,
}

impl ServiceBusService {
    pub fn new(db: Database, namespace: String) -> Self {
        ServiceBusService { db, namespace }
    }

    // ── Queues ────────────────────────────────────────────────────────────────

    pub async fn create_queue(&self, name: &str) -> Result<AsbQueue> {
        let n   = name.to_string();
        let ns  = self.namespace.clone();
        let now = Utc::now().to_rfc3339();
        let n2  = n.clone();
        let ns2 = ns.clone();
        let n3  = now.clone();
        self.db.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO asb_queues (name, namespace, created_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(namespace, name) DO NOTHING",
                rusqlite::params![n, ns, now],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(AsbQueue { name: n2, namespace: ns2, created_at: n3 })
    }

    pub async fn list_queues(&self) -> Result<Vec<AsbQueue>> {
        let ns = self.namespace.clone();
        self.db.conn.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT name, namespace, created_at FROM asb_queues
                 WHERE namespace=?1 ORDER BY name"
            )?;
            let rows = stmt.query_map(rusqlite::params![ns], |r| Ok(AsbQueue {
                name:       r.get(0)?,
                namespace:  r.get(1)?,
                created_at: r.get(2)?,
            }))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn delete_queue(&self, name: &str) -> Result<()> {
        let n  = name.to_string();
        let ns = self.namespace.clone();
        self.db.conn.call(move |conn| {
            conn.execute(
                "DELETE FROM asb_queues WHERE namespace=?1 AND name=?2",
                rusqlite::params![ns, n],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    // ── Topics ────────────────────────────────────────────────────────────────

    pub async fn create_topic(&self, name: &str) -> Result<AsbTopic> {
        let n   = name.to_string();
        let ns  = self.namespace.clone();
        let now = Utc::now().to_rfc3339();
        let n2  = n.clone();
        let ns2 = ns.clone();
        let n3  = now.clone();
        self.db.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO asb_topics (name, namespace, created_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(namespace, name) DO NOTHING",
                rusqlite::params![n, ns, now],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(AsbTopic { name: n2, namespace: ns2, created_at: n3 })
    }

    pub async fn list_topics(&self) -> Result<Vec<AsbTopic>> {
        let ns = self.namespace.clone();
        self.db.conn.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT name, namespace, created_at FROM asb_topics
                 WHERE namespace=?1 ORDER BY name"
            )?;
            let rows = stmt.query_map(rusqlite::params![ns], |r| Ok(AsbTopic {
                name:       r.get(0)?,
                namespace:  r.get(1)?,
                created_at: r.get(2)?,
            }))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn delete_topic(&self, name: &str) -> Result<()> {
        let n  = name.to_string();
        let ns = self.namespace.clone();
        self.db.conn.call(move |conn| {
            conn.execute(
                "DELETE FROM asb_topics WHERE namespace=?1 AND name=?2",
                rusqlite::params![ns, n],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    // ── Subscriptions ─────────────────────────────────────────────────────────

    pub async fn create_subscription(&self, topic: &str, name: &str) -> Result<AsbSubscription> {
        let n   = name.to_string();
        let t   = topic.to_string();
        let ns  = self.namespace.clone();
        let now = Utc::now().to_rfc3339();
        let n2  = n.clone();
        let t2  = t.clone();
        let ns2 = ns.clone();
        let n3  = now.clone();
        self.db.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO asb_subscriptions (name, topic, namespace, created_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(namespace, topic, name) DO NOTHING",
                rusqlite::params![n, t, ns, now],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(AsbSubscription { name: n2, topic: t2, namespace: ns2, created_at: n3 })
    }

    pub async fn list_subscriptions(&self, topic: &str) -> Result<Vec<AsbSubscription>> {
        let t  = topic.to_string();
        let ns = self.namespace.clone();
        self.db.conn.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT name, topic, namespace, created_at FROM asb_subscriptions
                 WHERE namespace=?1 AND topic=?2 ORDER BY name"
            )?;
            let rows = stmt.query_map(rusqlite::params![ns, t], |r| Ok(AsbSubscription {
                name:       r.get(0)?,
                topic:      r.get(1)?,
                namespace:  r.get(2)?,
                created_at: r.get(3)?,
            }))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    // ── Messages ──────────────────────────────────────────────────────────────

    pub async fn send_message(&self, queue_name: &str, body: &str) -> Result<String> {
        let mid  = Uuid::new_v4().to_string();
        let lt   = Uuid::new_v4().to_string();
        let qn   = queue_name.to_string();
        let ns   = self.namespace.clone();
        let b    = body.to_string();
        let mid2 = mid.clone();
        let lt2  = lt.clone();
        let now  = Utc::now().to_rfc3339();
        self.db.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO asb_messages
                 (message_id, namespace, queue_name, body, lock_token, visible, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
                rusqlite::params![mid2, ns, qn, b, lt2, now],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(mid)
    }

    pub async fn receive_messages(&self, queue_name: &str, max: i64) -> Result<Vec<AsbMessage>> {
        let qn  = queue_name.to_string();
        let ns  = self.namespace.clone();
        let lim = max.min(10).max(1);
        let msgs: Vec<AsbMessage> = self.db.conn.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT message_id, namespace, queue_name, body, lock_token, visible, created_at
                 FROM asb_messages
                 WHERE namespace=?1 AND queue_name=?2 AND visible=1
                 ORDER BY id LIMIT ?3"
            )?;
            let rows = stmt.query_map(rusqlite::params![ns, qn, lim], |r| Ok(AsbMessage {
                message_id:  r.get(0)?,
                namespace:   r.get(1)?,
                queue_name:  r.get(2)?,
                body:        r.get(3)?,
                lock_token:  r.get(4)?,
                visible:     r.get::<_, i64>(5)? != 0,
                created_at:  r.get(6)?,
            }))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;

        for msg in &msgs {
            let lt = msg.lock_token.clone();
            self.db.conn.call(move |conn| {
                conn.execute(
                    "UPDATE asb_messages SET visible=0 WHERE lock_token=?1",
                    rusqlite::params![lt],
                )?;
                Ok(())
            }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        Ok(msgs)
    }

    pub async fn complete_message(&self, lock_token: &str) -> Result<()> {
        let lt = lock_token.to_string();
        self.db.conn.call(move |conn| {
            conn.execute(
                "DELETE FROM asb_messages WHERE lock_token=?1",
                rusqlite::params![lt],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn abandon_message(&self, lock_token: &str) -> Result<()> {
        let lt = lock_token.to_string();
        self.db.conn.call(move |conn| {
            conn.execute(
                "UPDATE asb_messages SET visible=1 WHERE lock_token=?1",
                rusqlite::params![lt],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn send_topic_message(&self, topic: &str, body: &str) -> Result<String> {
        // Fan out to all subscriptions treated as virtual queues
        self.send_message(&format!("topic:{topic}"), body).await
    }

    pub async fn receive_subscription_messages(
        &self,
        topic: &str,
        subscription: &str,
        max: i64,
    ) -> Result<Vec<AsbMessage>> {
        let virtual_queue = format!("topic:{topic}:sub:{subscription}");
        self.receive_messages(&virtual_queue, max).await
    }
}

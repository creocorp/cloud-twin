//! GCP Pub/Sub business logic.
//!
//! This service owns topics, subscriptions, published messages, and the
//! pull/ack lifecycle represented by `ack_id` rows. Pulled messages are leased
//! for the subscription's ack deadline; unacknowledged messages become
//! deliverable again once their lease expires (or is reset with
//! `modifyAckDeadline`).

use chrono::Utc;
use rusqlite::OptionalExtension;
use uuid::Uuid;

use super::models::{now_ts, PubsubAckable, PubsubMessage, PubsubSubscription, PubsubTopic};
use crate::db::Database;

/// Domain errors, mapped onto Google RPC errors by the handlers.
#[derive(Debug)]
pub enum PubsubError {
    NotFound(String),
    AlreadyExists(String),
    InvalidArgument(String),
    Internal(String),
}

impl From<anyhow::Error> for PubsubError {
    fn from(e: anyhow::Error) -> Self {
        PubsubError::Internal(e.to_string())
    }
}

pub type PubsubResult<T> = Result<T, PubsubError>;

/// A message to publish: base64 `data` plus a JSON object of attributes.
#[derive(Debug, Clone)]
pub struct OutgoingMessage {
    pub data: String,
    pub attributes: serde_json::Value,
}

const DEFAULT_ACK_DEADLINE: i64 = 10;
const MAX_ACK_DEADLINE: i64 = 600;

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn exists(conn: &rusqlite::Connection, table: &str, name: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        &format!("SELECT 1 FROM {table} WHERE name=?1"),
        [name],
        |_| Ok(()),
    )
    .optional()
    .map(|o| o.is_some())
}

fn row_to_sub(r: &rusqlite::Row<'_>) -> rusqlite::Result<PubsubSubscription> {
    Ok(PubsubSubscription {
        name: r.get(0)?,
        topic: r.get(1)?,
        ack_deadline_seconds: r.get(2)?,
        created_at: r.get(3)?,
    })
}

const SUB_COLS: &str = "name, topic, ack_deadline_seconds, created_at";

/// Implements a minimal Pub/Sub runtime over SQLite.
pub struct PubsubService {
    db: Database,
    project: String,
}

impl PubsubService {
    pub fn new(db: Database, project: String) -> Self {
        PubsubService { db, project }
    }

    /// Run `f` on the connection thread.
    async fn run<T, F>(&self, f: F) -> PubsubResult<T>
    where
        F: FnOnce(&mut rusqlite::Connection) -> rusqlite::Result<PubsubResult<T>> + Send + 'static,
        T: Send + 'static,
    {
        self.db.call(f).await?
    }

    // ── Topics ────────────────────────────────────────────────────────────────

    pub async fn create_topic(&self, topic_name: &str) -> PubsubResult<PubsubTopic> {
        let topic = PubsubTopic {
            name: self.full_topic(topic_name),
            created_at: now_ts(),
        };
        let t = topic.clone();
        self.run(move |conn| {
            let n = conn.execute(
                "INSERT INTO pubsub_topics (name, created_at) VALUES (?1, ?2)
                 ON CONFLICT(name) DO NOTHING",
                rusqlite::params![t.name, t.created_at],
            )?;
            Ok(if n == 0 {
                Err(PubsubError::AlreadyExists(format!(
                    "Topic already exists: {}",
                    t.name
                )))
            } else {
                Ok(())
            })
        })
        .await?;
        Ok(topic)
    }

    pub async fn get_topic(&self, topic_name: &str) -> PubsubResult<PubsubTopic> {
        let n = self.full_topic(topic_name);
        self.run(move |conn| {
            let t = conn
                .query_row(
                    "SELECT name, created_at FROM pubsub_topics WHERE name=?1",
                    [&n],
                    |r| {
                        Ok(PubsubTopic {
                            name: r.get(0)?,
                            created_at: r.get(1)?,
                        })
                    },
                )
                .optional()?;
            Ok(t.ok_or_else(|| PubsubError::NotFound(format!("Topic not found: {n}"))))
        })
        .await
    }

    /// List the topics of `project` (defaults to the configured project).
    pub async fn list_topics(&self, project: Option<&str>) -> PubsubResult<Vec<PubsubTopic>> {
        let prefix = format!("projects/{}/topics/", project.unwrap_or(&self.project));
        self.run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT name, created_at FROM pubsub_topics
                 WHERE substr(name, 1, length(?1)) = ?1 ORDER BY name",
            )?;
            let rows = stmt
                .query_map([prefix], |r| {
                    Ok(PubsubTopic {
                        name: r.get(0)?,
                        created_at: r.get(1)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(Ok(rows))
        })
        .await
    }

    pub async fn delete_topic(&self, topic_name: &str) -> PubsubResult<()> {
        let n = self.full_topic(topic_name);
        self.run(move |conn| {
            let deleted = conn.execute("DELETE FROM pubsub_topics WHERE name=?1", [&n])?;
            Ok(if deleted == 0 {
                Err(PubsubError::NotFound(format!("Topic not found: {n}")))
            } else {
                Ok(())
            })
        })
        .await
    }

    /// Names of the subscriptions attached to a topic.
    pub async fn topic_subscriptions(&self, topic_name: &str) -> PubsubResult<Vec<String>> {
        let n = self.full_topic(topic_name);
        self.run(move |conn| {
            if !exists(conn, "pubsub_topics", &n)? {
                return Ok(Err(PubsubError::NotFound(format!("Topic not found: {n}"))));
            }
            let mut stmt =
                conn.prepare("SELECT name FROM pubsub_subscriptions WHERE topic=?1 ORDER BY name")?;
            let rows = stmt
                .query_map([&n], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(Ok(rows))
        })
        .await
    }

    // ── Subscriptions ─────────────────────────────────────────────────────────

    pub async fn create_subscription(
        &self,
        sub_name: &str,
        topic_name: &str,
        ack_deadline_seconds: Option<i64>,
    ) -> PubsubResult<PubsubSubscription> {
        if topic_name.is_empty() {
            return Err(PubsubError::InvalidArgument(
                "Subscription topic is required".into(),
            ));
        }
        let sub = PubsubSubscription {
            name: self.full_sub(sub_name),
            topic: self.full_topic(topic_name),
            ack_deadline_seconds: ack_deadline_seconds
                .filter(|s| *s > 0)
                .unwrap_or(DEFAULT_ACK_DEADLINE)
                .clamp(DEFAULT_ACK_DEADLINE, MAX_ACK_DEADLINE),
            created_at: now_ts(),
        };
        let s = sub.clone();
        self.run(move |conn| {
            if !exists(conn, "pubsub_topics", &s.topic)? {
                return Ok(Err(PubsubError::NotFound(format!(
                    "Topic not found: {}",
                    s.topic
                ))));
            }
            let n = conn.execute(
                "INSERT INTO pubsub_subscriptions (name, topic, ack_deadline_seconds, created_at)
                 VALUES (?1, ?2, ?3, ?4) ON CONFLICT(name) DO NOTHING",
                rusqlite::params![s.name, s.topic, s.ack_deadline_seconds, s.created_at],
            )?;
            Ok(if n == 0 {
                Err(PubsubError::AlreadyExists(format!(
                    "Subscription already exists: {}",
                    s.name
                )))
            } else {
                Ok(())
            })
        })
        .await?;
        Ok(sub)
    }

    pub async fn get_subscription(&self, sub_name: &str) -> PubsubResult<PubsubSubscription> {
        let n = self.full_sub(sub_name);
        self.run(move |conn| {
            let s = conn
                .query_row(
                    &format!("SELECT {SUB_COLS} FROM pubsub_subscriptions WHERE name=?1"),
                    [&n],
                    row_to_sub,
                )
                .optional()?;
            Ok(s.ok_or_else(|| PubsubError::NotFound(format!("Subscription does not exist: {n}"))))
        })
        .await
    }

    /// List the subscriptions of `project` (defaults to the configured project).
    pub async fn list_subscriptions(
        &self,
        project: Option<&str>,
    ) -> PubsubResult<Vec<PubsubSubscription>> {
        let prefix = format!(
            "projects/{}/subscriptions/",
            project.unwrap_or(&self.project)
        );
        self.run(move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {SUB_COLS} FROM pubsub_subscriptions
                 WHERE substr(name, 1, length(?1)) = ?1 ORDER BY name"
            ))?;
            let rows = stmt
                .query_map([prefix], row_to_sub)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(Ok(rows))
        })
        .await
    }

    pub async fn delete_subscription(&self, sub_name: &str) -> PubsubResult<()> {
        let n = self.full_sub(sub_name);
        self.run(move |conn| {
            let deleted = conn.execute("DELETE FROM pubsub_subscriptions WHERE name=?1", [&n])?;
            if deleted == 0 {
                return Ok(Err(PubsubError::NotFound(format!(
                    "Subscription does not exist: {n}"
                ))));
            }
            conn.execute("DELETE FROM pubsub_ackables WHERE subscription=?1", [&n])?;
            Ok(Ok(()))
        })
        .await
    }

    // ── Publish ───────────────────────────────────────────────────────────────

    /// Publish messages to a topic and fan them out to its subscriptions.
    /// Returns the message IDs in request order.
    pub async fn publish(
        &self,
        topic_name: &str,
        messages: Vec<OutgoingMessage>,
    ) -> PubsubResult<Vec<String>> {
        if messages.is_empty() {
            return Err(PubsubError::InvalidArgument(
                "The request contains no messages".into(),
            ));
        }
        let empty = |m: &OutgoingMessage| {
            m.data.is_empty() && m.attributes.as_object().is_none_or(|a| a.is_empty())
        };
        if messages.iter().any(empty) {
            return Err(PubsubError::InvalidArgument(
                "Some messages contain neither data nor attributes".into(),
            ));
        }
        let topic = self.full_topic(topic_name);
        let now = now_ts();
        let rows: Vec<(String, String, String)> = messages
            .into_iter()
            .map(|m| (Uuid::new_v4().to_string(), m.data, m.attributes.to_string()))
            .collect();
        let ids: Vec<String> = rows.iter().map(|r| r.0.clone()).collect();

        self.run(move |conn| {
            if !exists(conn, "pubsub_topics", &topic)? {
                return Ok(Err(PubsubError::NotFound(format!("Topic not found: {topic}"))));
            }
            let tx = conn.transaction()?;
            let subs: Vec<String> = tx
                .prepare("SELECT name FROM pubsub_subscriptions WHERE topic=?1")?
                .query_map([&topic], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (mid, data, attrs) in &rows {
                tx.execute(
                    "INSERT INTO pubsub_messages (message_id, topic, data, attributes, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![mid, topic, data, attrs, now],
                )?;
                for sub in &subs {
                    tx.execute(
                        "INSERT INTO pubsub_ackables (ack_id, subscription, message_id, acked, created_at)
                         VALUES (?1, ?2, ?3, 0, ?4)",
                        rusqlite::params![Uuid::new_v4().to_string(), sub, mid, now],
                    )?;
                }
            }
            tx.commit()?;
            Ok(Ok(()))
        })
        .await?;
        Ok(ids)
    }

    // ── Pull ──────────────────────────────────────────────────────────────────

    /// Lease up to `max_messages` deliverable messages for the subscription's
    /// ack deadline.
    pub async fn pull(
        &self,
        sub_name: &str,
        max_messages: i64,
    ) -> PubsubResult<Vec<(PubsubAckable, PubsubMessage)>> {
        let sub = self.full_sub(sub_name);
        let max = if max_messages <= 0 {
            10
        } else {
            max_messages.min(1000)
        };

        self.run(move |conn| {
            let deadline: Option<i64> = conn
                .query_row(
                    "SELECT ack_deadline_seconds FROM pubsub_subscriptions WHERE name=?1",
                    [&sub],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(deadline) = deadline else {
                return Ok(Err(PubsubError::NotFound(format!(
                    "Subscription does not exist: {sub}"
                ))));
            };
            let now = now_ms();
            let tx = conn.transaction()?;
            let rows = {
                let mut stmt = tx.prepare(
                    "SELECT a.ack_id, a.subscription, a.message_id,
                            m.message_id, m.topic, m.data, m.attributes, m.created_at
                     FROM pubsub_ackables a
                     JOIN pubsub_messages m ON m.message_id = a.message_id
                     WHERE a.subscription=?1 AND a.acked=0
                       AND (a.lease_until IS NULL OR a.lease_until <= ?2)
                     ORDER BY m.id LIMIT ?3",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![sub, now, max], |r| {
                        let ackable = PubsubAckable {
                            ack_id: r.get(0)?,
                            subscription: r.get(1)?,
                            message_id: r.get(2)?,
                        };
                        let msg = PubsubMessage {
                            message_id: r.get(3)?,
                            topic: r.get(4)?,
                            data: r.get(5)?,
                            attributes: r.get(6)?,
                            created_at: r.get(7)?,
                        };
                        Ok((ackable, msg))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows
            };
            for (a, _) in &rows {
                tx.execute(
                    "UPDATE pubsub_ackables SET lease_until=?2, delivery_attempt=delivery_attempt+1
                     WHERE ack_id=?1",
                    rusqlite::params![a.ack_id, now + deadline * 1000],
                )?;
            }
            tx.commit()?;
            Ok(Ok(rows))
        })
        .await
    }

    // ── Acknowledge / lease management ────────────────────────────────────────

    pub async fn acknowledge(&self, sub_name: &str, ack_ids: Vec<String>) -> PubsubResult<()> {
        let sub = self.full_sub(sub_name);
        self.run(move |conn| {
            if !exists(conn, "pubsub_subscriptions", &sub)? {
                return Ok(Err(PubsubError::NotFound(format!(
                    "Subscription does not exist: {sub}"
                ))));
            }
            for ack_id in &ack_ids {
                conn.execute(
                    "UPDATE pubsub_ackables SET acked=1 WHERE ack_id=?1 AND subscription=?2",
                    rusqlite::params![ack_id, sub],
                )?;
            }
            Ok(Ok(()))
        })
        .await
    }

    /// Extend (or with `0`, release) the lease of pulled messages.
    pub async fn modify_ack_deadline(
        &self,
        sub_name: &str,
        ack_ids: Vec<String>,
        seconds: i64,
    ) -> PubsubResult<()> {
        if !(0..=MAX_ACK_DEADLINE).contains(&seconds) {
            return Err(PubsubError::InvalidArgument(format!(
                "Invalid ack deadline: {seconds}; must be between 0 and {MAX_ACK_DEADLINE} seconds"
            )));
        }
        let sub = self.full_sub(sub_name);
        self.run(move |conn| {
            if !exists(conn, "pubsub_subscriptions", &sub)? {
                return Ok(Err(PubsubError::NotFound(format!(
                    "Subscription does not exist: {sub}"
                ))));
            }
            let until = now_ms() + seconds * 1000;
            for ack_id in &ack_ids {
                conn.execute(
                    "UPDATE pubsub_ackables SET lease_until=?3
                     WHERE ack_id=?1 AND subscription=?2 AND acked=0",
                    rusqlite::params![ack_id, sub, until],
                )?;
            }
            Ok(Ok(()))
        })
        .await
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

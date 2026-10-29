//! Azure Service Bus business logic.
//!
//! This is the provider-specific service layer for queues, topics,
//! subscriptions, and messages. Subscriptions are stored as virtual queues
//! (`topic:{topic}:sub:{subscription}`) in `asb_messages`; a message's
//! `visible` column encodes its state (1 = active, 0 = locked, 2 = dead-lettered).

use anyhow::Result;
use chrono::Utc;
use rusqlite::OptionalExtension;
use uuid::Uuid;

use super::models::{AsbMessage, AsbQueue, AsbSubscription, AsbTopic};
use crate::db::Database;

const ACTIVE: i64 = 1;
const LOCKED: i64 = 0;
const DEADLETTER: i64 = 2;

/// Maximum number of messages returned by a single receive.
pub const MAX_RECEIVE: i64 = 1000;

/// Implements a simplified Service Bus over SQLite.
pub struct ServiceBusService {
    db: Database,
    namespace: String,
}

fn map_err(e: tokio_rusqlite::Error) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}

/// Virtual queue name holding a subscription's messages.
pub fn subscription_queue(topic: &str, subscription: &str) -> String {
    format!("topic:{topic}:sub:{subscription}")
}

fn message_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AsbMessage> {
    Ok(AsbMessage {
        message_id: r.get(0)?,
        namespace: r.get(1)?,
        queue_name: r.get(2)?,
        body: r.get(3)?,
        lock_token: r.get(4)?,
        visible: r.get(5)?,
        created_at: r.get(6)?,
    })
}

fn insert_message(
    conn: &rusqlite::Connection,
    message_id: &str,
    ns: &str,
    queue: &str,
    body: &str,
    now: &str,
) -> rusqlite::Result<AsbMessage> {
    let lock_token = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO asb_messages
         (message_id, namespace, queue_name, body, lock_token, visible, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![message_id, ns, queue, body, lock_token, ACTIVE, now],
    )?;
    Ok(AsbMessage {
        message_id: message_id.to_string(),
        namespace: ns.to_string(),
        queue_name: queue.to_string(),
        body: body.to_string(),
        lock_token,
        visible: ACTIVE,
        created_at: now.to_string(),
    })
}

impl ServiceBusService {
    pub fn new(db: Database, namespace: String) -> Self {
        ServiceBusService { db, namespace }
    }

    // ── Queues ────────────────────────────────────────────────────────────────

    pub async fn create_queue(&self, name: &str) -> Result<AsbQueue> {
        let n = name.to_string();
        let ns = self.namespace.clone();
        let now = Utc::now().to_rfc3339();
        self.db
            .conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO asb_queues (name, namespace, created_at)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(namespace, name) DO NOTHING",
                    rusqlite::params![n, ns, now],
                )?;
                Ok(conn.query_row(
                    "SELECT name, namespace, created_at FROM asb_queues WHERE namespace=?1 AND name=?2",
                    rusqlite::params![ns, n],
                    |r| Ok(AsbQueue { name: r.get(0)?, namespace: r.get(1)?, created_at: r.get(2)? }),
                )?)
            })
            .await
            .map_err(map_err)
    }

    pub async fn get_queue(&self, name: &str) -> Result<Option<AsbQueue>> {
        let n = name.to_string();
        let ns = self.namespace.clone();
        self.db
            .conn
            .call(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT name, namespace, created_at FROM asb_queues WHERE namespace=?1 AND name=?2",
                        rusqlite::params![ns, n],
                        |r| Ok(AsbQueue { name: r.get(0)?, namespace: r.get(1)?, created_at: r.get(2)? }),
                    )
                    .optional()?)
            })
            .await
            .map_err(map_err)
    }

    pub async fn list_queues(&self) -> Result<Vec<AsbQueue>> {
        let ns = self.namespace.clone();
        self.db
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT name, namespace, created_at FROM asb_queues
                     WHERE namespace=?1 ORDER BY name",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![ns], |r| {
                        Ok(AsbQueue {
                            name: r.get(0)?,
                            namespace: r.get(1)?,
                            created_at: r.get(2)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
            .map_err(map_err)
    }

    /// Delete a queue and its messages; returns `false` when it did not exist.
    pub async fn delete_queue(&self, name: &str) -> Result<bool> {
        let n = name.to_string();
        let ns = self.namespace.clone();
        self.db
            .conn
            .call(move |conn| {
                let tx = conn.transaction()?;
                let deleted = tx.execute(
                    "DELETE FROM asb_queues WHERE namespace=?1 AND name=?2",
                    rusqlite::params![ns, n],
                )?;
                tx.execute(
                    "DELETE FROM asb_messages WHERE namespace=?1 AND queue_name=?2",
                    rusqlite::params![ns, n],
                )?;
                tx.commit()?;
                Ok(deleted > 0)
            })
            .await
            .map_err(map_err)
    }

    // ── Topics ────────────────────────────────────────────────────────────────

    pub async fn create_topic(&self, name: &str) -> Result<AsbTopic> {
        let n = name.to_string();
        let ns = self.namespace.clone();
        let now = Utc::now().to_rfc3339();
        self.db
            .conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO asb_topics (name, namespace, created_at)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(namespace, name) DO NOTHING",
                    rusqlite::params![n, ns, now],
                )?;
                Ok(conn.query_row(
                    "SELECT name, namespace, created_at FROM asb_topics WHERE namespace=?1 AND name=?2",
                    rusqlite::params![ns, n],
                    |r| Ok(AsbTopic { name: r.get(0)?, namespace: r.get(1)?, created_at: r.get(2)? }),
                )?)
            })
            .await
            .map_err(map_err)
    }

    pub async fn get_topic(&self, name: &str) -> Result<Option<AsbTopic>> {
        let n = name.to_string();
        let ns = self.namespace.clone();
        self.db
            .conn
            .call(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT name, namespace, created_at FROM asb_topics WHERE namespace=?1 AND name=?2",
                        rusqlite::params![ns, n],
                        |r| Ok(AsbTopic { name: r.get(0)?, namespace: r.get(1)?, created_at: r.get(2)? }),
                    )
                    .optional()?)
            })
            .await
            .map_err(map_err)
    }

    pub async fn list_topics(&self) -> Result<Vec<AsbTopic>> {
        let ns = self.namespace.clone();
        self.db
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT name, namespace, created_at FROM asb_topics
                     WHERE namespace=?1 ORDER BY name",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![ns], |r| {
                        Ok(AsbTopic {
                            name: r.get(0)?,
                            namespace: r.get(1)?,
                            created_at: r.get(2)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
            .map_err(map_err)
    }

    /// Delete a topic with its subscriptions and their messages; returns
    /// `false` when the topic did not exist.
    pub async fn delete_topic(&self, name: &str) -> Result<bool> {
        let n = name.to_string();
        let ns = self.namespace.clone();
        let sub_prefix = subscription_queue(name, "");
        self.db
            .conn
            .call(move |conn| {
                let tx = conn.transaction()?;
                let deleted = tx.execute(
                    "DELETE FROM asb_topics WHERE namespace=?1 AND name=?2",
                    rusqlite::params![ns, n],
                )?;
                tx.execute(
                    "DELETE FROM asb_subscriptions WHERE namespace=?1 AND topic=?2",
                    rusqlite::params![ns, n],
                )?;
                tx.execute(
                    "DELETE FROM asb_messages
                     WHERE namespace=?1 AND substr(queue_name, 1, length(?2)) = ?2",
                    rusqlite::params![ns, sub_prefix],
                )?;
                tx.commit()?;
                Ok(deleted > 0)
            })
            .await
            .map_err(map_err)
    }

    // ── Subscriptions ─────────────────────────────────────────────────────────

    /// Create a subscription; `None` when the topic does not exist.
    pub async fn create_subscription(
        &self,
        topic: &str,
        name: &str,
    ) -> Result<Option<AsbSubscription>> {
        let n = name.to_string();
        let t = topic.to_string();
        let ns = self.namespace.clone();
        let now = Utc::now().to_rfc3339();
        self.db
            .conn
            .call(move |conn| {
                let topics: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM asb_topics WHERE namespace=?1 AND name=?2",
                    rusqlite::params![ns, t],
                    |r| r.get(0),
                )?;
                if topics == 0 {
                    return Ok(None);
                }
                conn.execute(
                    "INSERT INTO asb_subscriptions (name, topic, namespace, created_at)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(namespace, topic, name) DO NOTHING",
                    rusqlite::params![n, t, ns, now],
                )?;
                Ok(Some(conn.query_row(
                    "SELECT name, topic, namespace, created_at FROM asb_subscriptions
                     WHERE namespace=?1 AND topic=?2 AND name=?3",
                    rusqlite::params![ns, t, n],
                    |r| {
                        Ok(AsbSubscription {
                            name: r.get(0)?,
                            topic: r.get(1)?,
                            namespace: r.get(2)?,
                            created_at: r.get(3)?,
                        })
                    },
                )?))
            })
            .await
            .map_err(map_err)
    }

    pub async fn subscription_exists(&self, topic: &str, name: &str) -> Result<bool> {
        let n = name.to_string();
        let t = topic.to_string();
        let ns = self.namespace.clone();
        self.db
            .conn
            .call(move |conn| {
                let c: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM asb_subscriptions WHERE namespace=?1 AND topic=?2 AND name=?3",
                    rusqlite::params![ns, t, n],
                    |r| r.get(0),
                )?;
                Ok(c > 0)
            })
            .await
            .map_err(map_err)
    }

    pub async fn list_subscriptions(&self, topic: &str) -> Result<Vec<AsbSubscription>> {
        let t = topic.to_string();
        let ns = self.namespace.clone();
        self.db
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT name, topic, namespace, created_at FROM asb_subscriptions
                     WHERE namespace=?1 AND topic=?2 ORDER BY name",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![ns, t], |r| {
                        Ok(AsbSubscription {
                            name: r.get(0)?,
                            topic: r.get(1)?,
                            namespace: r.get(2)?,
                            created_at: r.get(3)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
            .map_err(map_err)
    }

    /// Delete a subscription and its messages; returns `false` when it did not exist.
    pub async fn delete_subscription(&self, topic: &str, name: &str) -> Result<bool> {
        let n = name.to_string();
        let t = topic.to_string();
        let ns = self.namespace.clone();
        let vq = subscription_queue(topic, name);
        self.db
            .conn
            .call(move |conn| {
                let tx = conn.transaction()?;
                let deleted = tx.execute(
                    "DELETE FROM asb_subscriptions WHERE namespace=?1 AND topic=?2 AND name=?3",
                    rusqlite::params![ns, t, n],
                )?;
                tx.execute(
                    "DELETE FROM asb_messages WHERE namespace=?1 AND queue_name=?2",
                    rusqlite::params![ns, vq],
                )?;
                tx.commit()?;
                Ok(deleted > 0)
            })
            .await
            .map_err(map_err)
    }

    // ── Messages ──────────────────────────────────────────────────────────────

    /// Send to a queue; `None` when the queue does not exist.
    pub async fn send_message(&self, queue_name: &str, body: &str) -> Result<Option<AsbMessage>> {
        let qn = queue_name.to_string();
        let ns = self.namespace.clone();
        let b = body.to_string();
        let now = Utc::now().to_rfc3339();
        self.db
            .conn
            .call(move |conn| {
                let queues: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM asb_queues WHERE namespace=?1 AND name=?2",
                    rusqlite::params![ns, qn],
                    |r| r.get(0),
                )?;
                if queues == 0 {
                    return Ok(None);
                }
                let mid = Uuid::new_v4().to_string();
                Ok(Some(insert_message(conn, &mid, &ns, &qn, &b, &now)?))
            })
            .await
            .map_err(map_err)
    }

    /// Publish to a topic, copying the message to every subscription.
    /// Returns the copies, or `None` when the topic does not exist.
    pub async fn send_topic_message(
        &self,
        topic: &str,
        body: &str,
    ) -> Result<Option<Vec<AsbMessage>>> {
        let t = topic.to_string();
        let ns = self.namespace.clone();
        let b = body.to_string();
        let now = Utc::now().to_rfc3339();
        self.db
            .conn
            .call(move |conn| {
                let tx = conn.transaction()?;
                let topics: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM asb_topics WHERE namespace=?1 AND name=?2",
                    rusqlite::params![ns, t],
                    |r| r.get(0),
                )?;
                if topics == 0 {
                    return Ok(None);
                }
                let subs: Vec<String> = {
                    let mut stmt = tx.prepare(
                        "SELECT name FROM asb_subscriptions WHERE namespace=?1 AND topic=?2 ORDER BY name",
                    )?;
                    let rows = stmt
                        .query_map(rusqlite::params![ns, t], |r| r.get(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    rows
                };
                let mid = Uuid::new_v4().to_string();
                let mut out = Vec::with_capacity(subs.len());
                for s in subs {
                    out.push(insert_message(&tx, &mid, &ns, &subscription_queue(&t, &s), &b, &now)?);
                }
                tx.commit()?;
                Ok(Some(out))
            })
            .await
            .map_err(map_err)
    }

    /// Peek-lock up to `max` active messages from a (virtual) queue.
    pub async fn receive_messages(&self, queue_name: &str, max: i64) -> Result<Vec<AsbMessage>> {
        let qn = queue_name.to_string();
        let ns = self.namespace.clone();
        let lim = max.clamp(1, MAX_RECEIVE);
        self.db
            .conn
            .call(move |conn| {
                let tx = conn.transaction()?;
                let mut msgs = {
                    let mut stmt = tx.prepare(
                        "SELECT message_id, namespace, queue_name, body, lock_token, visible, created_at
                         FROM asb_messages
                         WHERE namespace=?1 AND queue_name=?2 AND visible=?3
                         ORDER BY id LIMIT ?4",
                    )?;
                    let rows = stmt
                        .query_map(rusqlite::params![ns, qn, ACTIVE, lim], message_from_row)?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    rows
                };
                for m in &mut msgs {
                    tx.execute(
                        "UPDATE asb_messages SET visible=?1 WHERE lock_token=?2",
                        rusqlite::params![LOCKED, m.lock_token],
                    )?;
                    m.visible = LOCKED;
                }
                tx.commit()?;
                Ok(msgs)
            })
            .await
            .map_err(map_err)
    }

    /// Move a locked message to `state`, or delete it (`None`, any state).
    /// Returns `false` when no matching message exists in `queue_name`.
    async fn settle(&self, queue_name: &str, lock_token: &str, state: Option<i64>) -> Result<bool> {
        let lt = lock_token.to_string();
        let qn = queue_name.to_string();
        let ns = self.namespace.clone();
        let n = self
            .db
            .conn
            .call(move |conn| {
                Ok(match state {
                    Some(v) => conn.execute(
                        "UPDATE asb_messages SET visible=?1
                         WHERE namespace=?2 AND queue_name=?3 AND lock_token=?4 AND visible=?5",
                        rusqlite::params![v, ns, qn, lt, LOCKED],
                    )?,
                    None => conn.execute(
                        "DELETE FROM asb_messages WHERE namespace=?1 AND queue_name=?2 AND lock_token=?3",
                        rusqlite::params![ns, qn, lt],
                    )?,
                })
            })
            .await
            .map_err(map_err)?;
        Ok(n > 0)
    }

    /// Complete (delete) a message.
    pub async fn complete_message(&self, queue_name: &str, lock_token: &str) -> Result<bool> {
        self.settle(queue_name, lock_token, None).await
    }

    /// Release a lock so the message can be received again.
    pub async fn abandon_message(&self, queue_name: &str, lock_token: &str) -> Result<bool> {
        self.settle(queue_name, lock_token, Some(ACTIVE)).await
    }

    /// Move a message to the dead-letter state; it is never received again.
    pub async fn deadletter_message(&self, queue_name: &str, lock_token: &str) -> Result<bool> {
        self.settle(queue_name, lock_token, Some(DEADLETTER)).await
    }
}

/// Wire name of a message's state.
pub fn state_name(visible: i64) -> &'static str {
    match visible {
        ACTIVE => "active",
        DEADLETTER => "deadletter",
        _ => "locked",
    }
}

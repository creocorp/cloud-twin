//! SNS business logic.
//!
//! This service persists topics, subscriptions, and published messages while
//! staying agnostic of the AWS Query HTTP envelope.

use anyhow::{bail, Result};
use chrono::Utc;
use uuid::Uuid;

use super::models::{SnsSubscription, SnsTopic};
use crate::db::Database;

const ACCOUNT_ID: &str = "000000000000";
const REGION: &str = "us-east-1";

/// Encapsulates SNS topic, subscription, and publish operations.
pub struct SnsService {
    db: Database,
}

impl SnsService {
    pub fn new(db: Database) -> Self {
        SnsService { db }
    }

    fn topic_arn(name: &str) -> String {
        format!("arn:aws:sns:{REGION}:{ACCOUNT_ID}:{name}")
    }
    #[allow(dead_code)]
    fn sub_arn(name: &str) -> String {
        format!(
            "arn:aws:sns:{REGION}:{ACCOUNT_ID}:{name}:{}",
            Uuid::new_v4().simple()
        )
    }

    pub async fn create_topic(&self, name: &str) -> Result<String> {
        let arn = Self::topic_arn(name);
        let n = name.to_string();
        let a = arn.clone();
        let now = Utc::now().to_rfc3339();
        self.db
            .conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO sns_topics (arn, name, created_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(name) DO NOTHING",
                    rusqlite::params![a, n, now],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(arn)
    }

    pub async fn list_topics(&self) -> Result<Vec<SnsTopic>> {
        self.db
            .conn
            .call(|conn| {
                let mut stmt =
                    conn.prepare("SELECT arn, name, created_at FROM sns_topics ORDER BY name")?;
                let rows = stmt
                    .query_map([], |r| {
                        Ok(SnsTopic {
                            arn: r.get(0)?,
                            name: r.get(1)?,
                            created_at: r.get(2)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn delete_topic(&self, arn: &str) -> Result<()> {
        let a = arn.to_string();
        self.db
            .conn
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM sns_topics WHERE arn = ?1",
                    rusqlite::params![a],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn subscribe(
        &self,
        topic_arn: &str,
        protocol: &str,
        endpoint: &str,
    ) -> Result<String> {
        // Validate topic exists
        let ta = topic_arn.to_string();
        let exists: bool = self
            .db
            .conn
            .call(move |conn| {
                let n: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM sns_topics WHERE arn = ?1",
                    rusqlite::params![ta],
                    |r| r.get(0),
                )?;
                Ok(n > 0)
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        if !exists {
            bail!("NotFound: topic {topic_arn}");
        }

        // Derive sub name from endpoint
        let sub_name = topic_arn.rsplit(':').next().unwrap_or("topic");
        let sub_arn = format!(
            "arn:aws:sns:{REGION}:{ACCOUNT_ID}:{sub_name}:{}",
            Uuid::new_v4().simple()
        );
        let ta2 = topic_arn.to_string();
        let pr = protocol.to_string();
        let ep = endpoint.to_string();
        let sa = sub_arn.clone();
        let now = Utc::now().to_rfc3339();
        self.db
            .conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO sns_subscriptions (arn, topic_arn, protocol, endpoint, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(arn) DO NOTHING",
                    rusqlite::params![sa, ta2, pr, ep, now],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(sub_arn)
    }

    pub async fn list_subscriptions_by_topic(
        &self,
        topic_arn: &str,
    ) -> Result<Vec<SnsSubscription>> {
        let ta = topic_arn.to_string();
        self.db
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT arn, topic_arn, protocol, endpoint, created_at
                 FROM sns_subscriptions WHERE topic_arn = ?1 ORDER BY arn",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![ta], |r| {
                        Ok(SnsSubscription {
                            arn: r.get(0)?,
                            topic_arn: r.get(1)?,
                            protocol: r.get(2)?,
                            endpoint: r.get(3)?,
                            created_at: r.get(4)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn publish(
        &self,
        topic_arn: &str,
        message: &str,
        subject: Option<&str>,
    ) -> Result<String> {
        let message_id = Uuid::new_v4().to_string();
        let ta = topic_arn.to_string();
        let mid = message_id.clone();
        let msg = message.to_string();
        let subj = subject.map(|s| s.to_string());
        let now = Utc::now().to_rfc3339();
        self.db
            .conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO sns_messages (message_id, topic_arn, subject, body, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![mid, ta, subj, msg, now],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(message_id)
    }
}

use anyhow::{bail, Result};
use chrono::Utc;
use rusqlite::OptionalExtension;

use crate::db::Database;
use super::models::Secret;

const ACCOUNT_ID: &str = "000000000000";
const REGION:     &str = "us-east-1";

pub struct SecretsManagerService {
    db: Database,
}

impl SecretsManagerService {
    pub fn new(db: Database) -> Self { SecretsManagerService { db } }

    fn secret_arn(name: &str) -> String {
        format!("arn:aws:secretsmanager:{REGION}:{ACCOUNT_ID}:secret:{name}")
    }

    pub async fn create_secret(&self, name: &str, secret_string: Option<&str>) -> Result<Secret> {
        let existing = self.get_secret(name).await?;
        if existing.is_some() {
            bail!("ResourceExistsException: secret {name} already exists");
        }
        let arn = Self::secret_arn(name);
        let n   = name.to_string();
        let a   = arn.clone();
        let ss  = secret_string.map(|s| s.to_string());
        let now = Utc::now().to_rfc3339();
        let n2  = now.clone();
        self.db.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO secrets (name, arn, secret_string, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![n, a, ss, now, n2],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        self.get_secret(name).await?.ok_or_else(|| anyhow::anyhow!("not found after insert"))
    }

    pub async fn get_secret(&self, name: &str) -> Result<Option<Secret>> {
        let n = name.to_string();
        self.db.conn.call(move |conn| {
            Ok(conn.query_row(
                "SELECT name, arn, secret_string, created_at, updated_at
                 FROM secrets WHERE name = ?1",
                rusqlite::params![n],
                |r| Ok(Secret {
                    name:          r.get(0)?,
                    arn:           r.get(1)?,
                    secret_string: r.get(2)?,
                    created_at:    r.get(3)?,
                    updated_at:    r.get(4)?,
                }),
            ).optional()?)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn get_secret_value(&self, secret_id: &str) -> Result<Secret> {
        self.get_secret(secret_id).await?
            .ok_or_else(|| anyhow::anyhow!("ResourceNotFoundException: secret {secret_id} not found"))
    }

    pub async fn put_secret_value(&self, secret_id: &str, secret_string: &str) -> Result<()> {
        let n   = secret_id.to_string();
        let ss  = secret_string.to_string();
        let now = Utc::now().to_rfc3339();
        let affected = self.db.conn.call(move |conn| {
            Ok(conn.execute(
                "UPDATE secrets SET secret_string=?1, updated_at=?2 WHERE name=?3",
                rusqlite::params![ss, now, n],
            )?)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        if affected == 0 { bail!("ResourceNotFoundException: secret {secret_id} not found"); }
        Ok(())
    }

    pub async fn list_secrets(&self) -> Result<Vec<Secret>> {
        self.db.conn.call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT name, arn, secret_string, created_at, updated_at
                 FROM secrets ORDER BY name"
            )?;
            let rows = stmt.query_map([], |r| Ok(Secret {
                name:          r.get(0)?,
                arn:           r.get(1)?,
                secret_string: r.get(2)?,
                created_at:    r.get(3)?,
                updated_at:    r.get(4)?,
            }))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn delete_secret(&self, secret_id: &str) -> Result<()> {
        let n = secret_id.to_string();
        self.db.conn.call(move |conn| {
            conn.execute("DELETE FROM secrets WHERE name = ?1", rusqlite::params![n])?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }
}

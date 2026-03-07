//! SES business logic shared by both SES v1 and SES v2 handlers.
//!
//! Multiple protocol adapters feed one service so identity/message state stays
//! unified regardless of which client API is used.

use anyhow::Result;
use chrono::Utc;
use rusqlite::OptionalExtension;
use uuid::Uuid;

use crate::db::Database;
use super::models::{SesIdentity, SesMessage};

/// Implements SES identity and outbound-email behavior.
pub struct SesService {
    db: Database,
}

impl SesService {
    pub fn new(db: Database) -> Self { SesService { db } }

    pub async fn verify_email(&self, email: &str) -> Result<()> {
        let email = email.to_string();
        let now   = Utc::now().to_rfc3339();
        self.db.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO ses_identities (identity, kind, verified, token, created_at)
                 VALUES (?1, 'email', 1, NULL, ?2)
                 ON CONFLICT(identity) DO UPDATE SET verified=1",
                rusqlite::params![email, now],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn verify_domain(&self, domain: &str) -> Result<String> {
        let d = domain.to_string();
        // Return existing token if one already exists.
        let existing: Option<Option<String>> = self.db.conn.call(move |conn| {
            Ok(conn.query_row(
                "SELECT token FROM ses_identities WHERE identity = ?1",
                rusqlite::params![d],
                |row| row.get::<_, Option<String>>(0),
            ).optional()?)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;

        if let Some(Some(token)) = existing {
            return Ok(token);
        }

        let token  = Uuid::new_v4().simple().to_string();
        let domain = domain.to_string();
        let t      = token.clone();
        let now    = Utc::now().to_rfc3339();
        self.db.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO ses_identities (identity, kind, verified, token, created_at)
                 VALUES (?1, 'domain', 1, ?2, ?3)
                 ON CONFLICT(identity) DO UPDATE SET token=excluded.token, verified=1",
                rusqlite::params![domain, t, now],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(token)
    }

    pub async fn list_identities(&self) -> Result<Vec<SesIdentity>> {
        self.db.conn.call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT identity, kind, verified, token, created_at
                 FROM ses_identities ORDER BY created_at"
            )?;
            let rows = stmt.query_map([], |r| Ok(SesIdentity {
                identity:   r.get(0)?,
                kind:       r.get(1)?,
                verified:   r.get::<_, i64>(2)? != 0,
                token:      r.get(3)?,
                created_at: r.get(4)?,
            }))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn get_identity(&self, identity: &str) -> Result<Option<SesIdentity>> {
        let id = identity.to_string();
        self.db.conn.call(move |conn| {
            Ok(conn.query_row(
                "SELECT identity, kind, verified, token, created_at
                 FROM ses_identities WHERE identity = ?1",
                rusqlite::params![id],
                |r| Ok(SesIdentity {
                    identity:   r.get(0)?,
                    kind:       r.get(1)?,
                    verified:   r.get::<_, i64>(2)? != 0,
                    token:      r.get(3)?,
                    created_at: r.get(4)?,
                }),
            ).optional()?)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn delete_identity(&self, identity: &str) -> Result<()> {
        let id = identity.to_string();
        self.db.conn.call(move |conn| {
            conn.execute("DELETE FROM ses_identities WHERE identity = ?1", rusqlite::params![id])?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn send_email(
        &self,
        source: &str,
        destinations: Vec<String>,
        subject: &str,
        text_body: Option<String>,
        html_body: Option<String>,
    ) -> Result<String> {
        let message_id = format!("{}@cloudtwin.local", Uuid::new_v4());
        let src        = source.to_string();
        let dest_json  = serde_json::to_string(&destinations).unwrap_or_else(|_| "[]".into());
        let subj       = subject.to_string();
        let now        = Utc::now().to_rfc3339();
        let mid        = message_id.clone();
        self.db.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO ses_messages
                 (message_id, source, destinations, subject, text_body, html_body, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'sent', ?7)",
                rusqlite::params![mid, src, dest_json, subj, text_body, html_body, now],
            )?;
            Ok(())
        }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(message_id)
    }

    pub async fn list_messages(&self) -> Result<Vec<SesMessage>> {
        self.db.conn.call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT message_id, source, destinations, subject, text_body, html_body, status, created_at
                 FROM ses_messages ORDER BY created_at DESC"
            )?;
            let rows = stmt.query_map([], |r| {
                let dest_json: String = r.get(2)?;
                Ok(SesMessage {
                    message_id:   r.get(0)?,
                    source:       r.get(1)?,
                    destinations: serde_json::from_str(&dest_json).unwrap_or_default(),
                    subject:      r.get(3)?,
                    text_body:    r.get(4)?,
                    html_body:    r.get(5)?,
                    status:       r.get(6)?,
                    created_at:   r.get(7)?,
                })
            })?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).await.map_err(|e| anyhow::anyhow!("{e}"))
    }
}

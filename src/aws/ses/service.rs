//! SES domain logic shared by the v1 Query and v2 REST handlers.
//!
//! Identities (verification lifecycle, DKIM, MAIL FROM), account settings, the
//! account suppression list, configuration sets, and the event log live here.
//! Outbound sending is in `sending.rs`; inbound receipt rules in `receiving.rs`.

use std::sync::Arc;

use chrono::{Duration, Utc};
use rand::Rng;
use rusqlite::OptionalExtension;
use serde_json::{json, Value};

use super::models::*;
use crate::AppState;

pub struct SesService {
    pub(super) state: Arc<AppState>,
}

const IDENTITY_COLS: &str = "identity, kind, status, verification, token, dkim_tokens, \
    dkim_signing_enabled, dkim_origin, mail_from_domain, mail_from_behavior, \
    mail_from_verification, feedback_forwarding, configuration_set, tags, created_at, \
    last_checked_at, last_success_at";

fn identity_from_row(r: &rusqlite::Row) -> rusqlite::Result<SesIdentity> {
    let status: String = r.get(2)?;
    let verification: VerificationState = serde_json::from_str(&r.get::<_, String>(3)?)
        .ok()
        .filter(|v: &VerificationState| !v.status.is_empty())
        .unwrap_or_else(|| VerificationState::resolved(&status));
    let mail_from: VerificationState =
        serde_json::from_str(&r.get::<_, String>(10)?).unwrap_or_default();
    Ok(SesIdentity {
        identity: r.get(0)?,
        kind: r.get(1)?,
        status: verification.status.clone(),
        verification,
        token: r.get(4)?,
        dkim_tokens: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
        dkim_signing_enabled: r.get::<_, i64>(6)? != 0,
        dkim_origin: r.get(7)?,
        mail_from_domain: r.get(8)?,
        mail_from_behavior: r.get(9)?,
        mail_from,
        feedback_forwarding: r.get::<_, i64>(11)? != 0,
        configuration_set: r.get(12)?,
        tags: serde_json::from_str(&r.get::<_, String>(13)?).unwrap_or_else(|_| json!([])),
        created_at: r.get(14)?,
        last_checked_at: r.get(15)?,
        last_success_at: r.get(16)?,
    })
}

pub fn random_token(len: usize, alphabet: &[u8]) -> String {
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| alphabet[rng.gen_range(0..alphabet.len())] as char)
        .collect()
}

pub const LOWER_ALNUM: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// Domain part of an address (`Name <a@b.c>` and bare forms), lowercased.
pub fn address_domain(addr: &str) -> Option<String> {
    bare_address(addr)
        .rsplit_once('@')
        .map(|(_, d)| d.to_ascii_lowercase())
}

/// Strip a display name: `"Name" <a@b.c>` → `a@b.c`.
pub fn bare_address(addr: &str) -> String {
    let a = addr.trim();
    match (a.rfind('<'), a.rfind('>')) {
        (Some(s), Some(e)) if e > s => a[s + 1..e].trim().to_string(),
        _ => a.to_string(),
    }
}

/// Canonical identity key: domains lowercased, email domain part lowercased.
pub fn normalize_identity(identity: &str) -> String {
    let id = identity.trim();
    match id.rsplit_once('@') {
        Some((local, domain)) => format!("{local}@{}", domain.to_ascii_lowercase()),
        None => id.trim_end_matches('.').to_ascii_lowercase(),
    }
}

/// Options accepted by `CreateEmailIdentity`.
#[derive(Default)]
pub struct CreateIdentityOpts {
    pub tags: Option<Value>,
    pub configuration_set: Option<String>,
    /// BYODKIM selector (`DkimSigningAttributes.DomainSigningSelector`).
    pub byodkim_selector: Option<String>,
}

/// Effective account-level settings (`GetAccount`).
#[derive(Debug, Clone)]
pub struct AccountView {
    pub production_access_enabled: bool,
    pub sending_enabled: bool,
    pub enforcement_status: String,
    pub max_24_hour_send: f64,
    pub max_send_rate: f64,
    pub sent_last_24_hours: f64,
    pub suppressed_reasons: Vec<String>,
}

impl SesService {
    pub fn new(state: Arc<AppState>) -> Self {
        SesService { state }
    }

    fn db(&self) -> &crate::db::Database {
        &self.state.db
    }

    // ── Identities ────────────────────────────────────────────────────────

    /// Create an identity. With `fail_if_exists` an existing identity yields
    /// `AlreadyExistsException` (SES v2); otherwise it is returned as-is (v1).
    pub async fn create_identity(
        &self,
        identity: &str,
        opts: CreateIdentityOpts,
        fail_if_exists: bool,
    ) -> SesResult<SesIdentity> {
        let identity = normalize_identity(identity);
        if identity.is_empty() {
            return Err(bad_request("EmailIdentity is required."));
        }
        if let Some(existing) = self.load_identity(&identity).await? {
            if fail_if_exists {
                return Err(SesError::new(
                    SesErrorKind::AlreadyExists,
                    format!("Email identity {identity} already exist."),
                ));
            }
            return Ok(existing);
        }
        if let Some(cs) = &opts.configuration_set {
            self.require_configuration_set(cs).await?;
        }

        let now = Utc::now();
        let rule = self.state.cfg.ses.verification.rule_for(&identity);
        let verification = VerificationState::start(&rule, now);
        let is_domain = !identity.contains('@');
        let (token, dkim_tokens, origin) = if is_domain {
            let token = base64_token();
            match &opts.byodkim_selector {
                Some(sel) => (Some(token), vec![sel.clone()], "EXTERNAL".to_string()),
                None => (
                    Some(token),
                    (0..3).map(|_| random_token(32, LOWER_ALNUM)).collect(),
                    "AWS_SES".to_string(),
                ),
            }
        } else {
            (None, Vec::new(), "AWS_SES".to_string())
        };

        let rec = SesIdentity {
            identity: identity.clone(),
            kind: if is_domain { "domain" } else { "email" }.into(),
            status: verification.status.clone(),
            verification,
            token,
            dkim_tokens,
            dkim_signing_enabled: is_domain,
            dkim_origin: origin,
            mail_from_domain: None,
            mail_from_behavior: "USE_DEFAULT_VALUE".into(),
            mail_from: VerificationState::default(),
            feedback_forwarding: true,
            configuration_set: opts.configuration_set,
            tags: opts.tags.unwrap_or_else(|| json!([])),
            created_at: now.to_rfc3339(),
            last_checked_at: None,
            last_success_at: None,
        };
        self.insert_identity(&rec).await?;

        if !is_domain && rec.status == PENDING {
            self.send_verification_email(&rec.identity).await;
        }
        Ok(rec)
    }

    /// Create or force an identity to a given status (seeds).
    pub async fn ensure_identity_with_status(&self, identity: &str, status: &str) -> SesResult<()> {
        let rec = self
            .create_identity(identity, CreateIdentityOpts::default(), false)
            .await?;
        if rec.status != status.to_ascii_uppercase() {
            self.set_verification(&rec.identity, Some(status), None)
                .await?;
        }
        Ok(())
    }

    async fn insert_identity(&self, rec: &SesIdentity) -> SesResult<()> {
        let r = rec.clone();
        self.db()
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO ses_identities
                     (identity, kind, verified, status, verification, token, dkim_tokens,
                      dkim_signing_enabled, dkim_origin, mail_from_behavior, feedback_forwarding,
                      configuration_set, tags, created_at, last_success_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                    rusqlite::params![
                        r.identity,
                        r.kind,
                        r.verified() as i64,
                        r.status,
                        serde_json::to_string(&r.verification).unwrap_or_default(),
                        r.token,
                        serde_json::to_string(&r.dkim_tokens).unwrap_or_default(),
                        r.dkim_signing_enabled as i64,
                        r.dkim_origin,
                        r.mail_from_behavior,
                        r.feedback_forwarding as i64,
                        r.configuration_set,
                        r.tags.to_string(),
                        r.created_at,
                        r.verified().then(|| r.created_at.clone()),
                    ],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    async fn save_identity(&self, rec: &SesIdentity) -> SesResult<()> {
        let r = rec.clone();
        self.db()
            .call(move |conn| {
                conn.execute(
                    "UPDATE ses_identities SET
                       verified = ?2, status = ?3, verification = ?4, dkim_tokens = ?5,
                       dkim_signing_enabled = ?6, dkim_origin = ?7, mail_from_domain = ?8,
                       mail_from_behavior = ?9, mail_from_verification = ?10,
                       feedback_forwarding = ?11, configuration_set = ?12, tags = ?13,
                       last_checked_at = ?14, last_success_at = ?15
                     WHERE identity = ?1",
                    rusqlite::params![
                        r.identity,
                        r.verified() as i64,
                        r.verification.status,
                        serde_json::to_string(&r.verification).unwrap_or_default(),
                        serde_json::to_string(&r.dkim_tokens).unwrap_or_default(),
                        r.dkim_signing_enabled as i64,
                        r.dkim_origin,
                        r.mail_from_domain,
                        r.mail_from_behavior,
                        serde_json::to_string(&r.mail_from).unwrap_or_default(),
                        r.feedback_forwarding as i64,
                        r.configuration_set,
                        r.tags.to_string(),
                        r.last_checked_at,
                        r.last_success_at,
                    ],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    async fn load_identity(&self, identity: &str) -> SesResult<Option<SesIdentity>> {
        let id = normalize_identity(identity);
        Ok(self
            .db()
            .call(move |conn| {
                conn.query_row(
                    &format!("SELECT {IDENTITY_COLS} FROM ses_identities WHERE identity = ?1"),
                    rusqlite::params![id],
                    identity_from_row,
                )
                .optional()
            })
            .await?)
    }

    /// Apply time/check-based verification progress and persist changes.
    async fn refresh(&self, mut rec: SesIdentity, count_check: bool) -> SesResult<SesIdentity> {
        let now = Utc::now();
        let mut changed = rec.verification.advance(count_check, now);
        changed |= rec.mail_from.advance(count_check, now);
        // A pending `after_checks` read changes the remaining-check counter.
        changed |= count_check && (rec.status == PENDING || rec.mail_from.status == PENDING);
        if count_check {
            rec.last_checked_at = Some(now.to_rfc3339());
            changed = true;
        }
        rec.status = rec.verification.status.clone();
        if rec.verified() && rec.last_success_at.is_none() {
            rec.last_success_at = Some(now.to_rfc3339());
        }
        if changed {
            self.save_identity(&rec).await?;
        }
        Ok(rec)
    }

    /// Fetch an identity. `count_check` counts this read toward
    /// `after_checks` verification (GetEmailIdentity and v1 equivalents).
    pub async fn get_identity(
        &self,
        identity: &str,
        count_check: bool,
    ) -> SesResult<Option<SesIdentity>> {
        match self.load_identity(identity).await? {
            Some(rec) => Ok(Some(self.refresh(rec, count_check).await?)),
            None => Ok(None),
        }
    }

    pub async fn require_identity(
        &self,
        identity: &str,
        count_check: bool,
    ) -> SesResult<SesIdentity> {
        self.get_identity(identity, count_check)
            .await?
            .ok_or_else(|| not_found(format!("Email identity {identity} does not exist.")))
    }

    pub async fn list_identities(&self) -> SesResult<Vec<SesIdentity>> {
        let rows = self
            .db()
            .call(|conn| {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {IDENTITY_COLS} FROM ses_identities ORDER BY created_at, identity"
                ))?;
                let rows = stmt
                    .query_map([], identity_from_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(self.refresh(r, false).await?);
        }
        Ok(out)
    }

    /// Delete an identity; returns whether it existed.
    pub async fn delete_identity(&self, identity: &str) -> SesResult<bool> {
        let id = normalize_identity(identity);
        Ok(self
            .db()
            .call(move |conn| {
                Ok(conn.execute(
                    "DELETE FROM ses_identities WHERE identity = ?1",
                    rusqlite::params![id],
                )? > 0)
            })
            .await?)
    }

    /// Force verification statuses (control endpoint / verification link).
    pub async fn set_verification(
        &self,
        identity: &str,
        status: Option<&str>,
        mail_from_status: Option<&str>,
    ) -> SesResult<SesIdentity> {
        let mut rec = self
            .load_identity(identity)
            .await?
            .ok_or_else(|| not_found(format!("Email identity {identity} does not exist.")))?;
        if let Some(s) = status {
            rec.verification.force(s);
            rec.status = rec.verification.status.clone();
            if rec.verified() {
                rec.last_success_at = Some(Utc::now().to_rfc3339());
            }
        }
        if let Some(s) = mail_from_status {
            if rec.mail_from_domain.is_none() {
                return Err(bad_request(format!(
                    "Identity {identity} has no custom MAIL FROM domain."
                )));
            }
            rec.mail_from.force(s);
        }
        self.save_identity(&rec).await?;
        Ok(rec)
    }

    /// `PutEmailIdentityMailFromAttributes` / v1 `SetIdentityMailFromDomain`.
    pub async fn put_mail_from(
        &self,
        identity: &str,
        domain: Option<&str>,
        behavior: Option<&str>,
    ) -> SesResult<()> {
        let mut rec = self.require_identity(identity, false).await?;
        let behavior = match behavior.map(|b| b.to_ascii_uppercase().replace('_', "")) {
            None => "USE_DEFAULT_VALUE".to_string(),
            Some(b) if b == "USEDEFAULTVALUE" => "USE_DEFAULT_VALUE".to_string(),
            Some(b) if b == "REJECTMESSAGE" => "REJECT_MESSAGE".to_string(),
            Some(b) => return Err(bad_request(format!("Invalid BehaviorOnMxFailure: {b}"))),
        };
        match domain.map(|d| d.trim().trim_end_matches('.').to_ascii_lowercase()) {
            Some(d) if !d.is_empty() => {
                let parent = if rec.is_domain() {
                    rec.identity.clone()
                } else {
                    address_domain(&rec.identity).unwrap_or_default()
                };
                if !d.ends_with(&format!(".{parent}")) {
                    return Err(bad_request(format!(
                        "MAIL FROM domain {d} must be a subdomain of {parent}."
                    )));
                }
                if rec.mail_from_domain.as_deref() != Some(d.as_str()) {
                    let rule = self.state.cfg.ses.verification.rule_for(&d);
                    rec.mail_from = VerificationState::start(&rule, Utc::now());
                }
                rec.mail_from_domain = Some(d);
            }
            _ => {
                rec.mail_from_domain = None;
                rec.mail_from = VerificationState::default();
            }
        }
        rec.mail_from_behavior = behavior;
        self.save_identity(&rec).await
    }

    pub async fn put_dkim_signing(&self, identity: &str, enabled: bool) -> SesResult<()> {
        let mut rec = self.require_identity(identity, false).await?;
        rec.dkim_signing_enabled = enabled;
        self.save_identity(&rec).await
    }

    pub async fn put_feedback_forwarding(&self, identity: &str, enabled: bool) -> SesResult<()> {
        let mut rec = self.require_identity(identity, false).await?;
        rec.feedback_forwarding = enabled;
        self.save_identity(&rec).await
    }

    pub async fn put_identity_configuration_set(
        &self,
        identity: &str,
        cs: Option<String>,
    ) -> SesResult<()> {
        let mut rec = self.require_identity(identity, false).await?;
        if let Some(name) = &cs {
            self.require_configuration_set(name).await?;
        }
        rec.configuration_set = cs;
        self.save_identity(&rec).await
    }

    /// The verified identity that authorizes sending as `address`: the address
    /// itself, its domain, or a parent domain (SES identity inheritance).
    pub async fn sending_identity(&self, address: &str) -> SesResult<Option<SesIdentity>> {
        let addr = normalize_identity(&bare_address(address));
        let mut candidates = vec![addr.clone()];
        if let Some(domain) = address_domain(&addr) {
            let labels: Vec<&str> = domain.split('.').collect();
            for i in 0..labels.len().saturating_sub(1) {
                candidates.push(labels[i..].join("."));
            }
        }
        for c in candidates {
            if let Some(rec) = self.get_identity(&c, false).await? {
                if rec.verified() {
                    return Ok(Some(rec));
                }
            }
        }
        Ok(None)
    }

    /// Capture the AWS "Email Address Verification Request" in the inbox; the
    /// link verifies the identity via the control API.
    async fn send_verification_email(&self, identity: &str) {
        let link = format!(
            "{}/_cloudtwin/ses/verify-email?identity={}",
            self.state.cfg.base_url.trim_end_matches('/'),
            percent_encoding::utf8_percent_encode(identity, percent_encoding::NON_ALPHANUMERIC)
        );
        let raw = format!(
            "From: no-reply-aws@amazon.com\r\nTo: {identity}\r\n\
             Subject: Amazon Web Services - Email Address Verification Request in region US East (N. Virginia)\r\n\
             MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n\
             Dear Amazon Web Services Customer,\r\n\r\n\
             We have received a request to authorize this email address for use with Amazon SES \
             in region US East (N. Virginia). To confirm that you are authorized to use this email \
             address, go to the following URL:\r\n\r\n{link}\r\n"
        );
        if let Err(e) =
            crate::mailbox::service::capture_raw(&self.state.db, raw.as_bytes(), "ses").await
        {
            tracing::warn!("failed to capture SES verification email: {e}");
        }
    }

    // ── Account ───────────────────────────────────────────────────────────

    async fn account_settings(&self) -> SesResult<serde_json::Map<String, Value>> {
        let rows: Vec<(String, String)> = self
            .db()
            .call(|conn| {
                let mut stmt = conn.prepare("SELECT key, value FROM ses_account_settings")?;
                let rows = stmt
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(k, v)| serde_json::from_str(&v).ok().map(|v| (k, v)))
            .collect())
    }

    pub async fn set_account_setting(&self, key: &str, value: Value) -> SesResult<()> {
        let k = key.to_string();
        let v = value.to_string();
        self.db()
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO ses_account_settings (key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    rusqlite::params![k, v],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn account(&self) -> SesResult<AccountView> {
        let cfg = &self.state.cfg.ses.account;
        let s = self.account_settings().await?;
        let since = (Utc::now() - Duration::hours(24)).to_rfc3339();
        let sent: i64 = self
            .db()
            .call(move |conn| {
                conn.query_row(
                    "SELECT COALESCE(SUM(json_array_length(destinations)), 0)
                     FROM ses_messages WHERE created_at >= ?1",
                    rusqlite::params![since],
                    |r| r.get(0),
                )
            })
            .await?;
        Ok(AccountView {
            production_access_enabled: s
                .get("production_access_enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(cfg.production_access_enabled),
            sending_enabled: s
                .get("sending_enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(cfg.sending_enabled),
            enforcement_status: s
                .get("enforcement_status")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string())
                .unwrap_or_else(|| cfg.enforcement_status.clone()),
            max_24_hour_send: s
                .get("max_24_hour_send")
                .and_then(|v| v.as_f64())
                .unwrap_or(cfg.max_24_hour_send),
            max_send_rate: s
                .get("max_send_rate")
                .and_then(|v| v.as_f64())
                .unwrap_or(cfg.max_send_rate),
            sent_last_24_hours: sent as f64,
            suppressed_reasons: s
                .get("suppressed_reasons")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_else(|| cfg.suppressed_reasons.clone()),
        })
    }

    // ── Suppression list ──────────────────────────────────────────────────

    pub async fn put_suppressed(
        &self,
        email: &str,
        reason: &str,
        message_id: Option<String>,
        feedback_id: Option<String>,
    ) -> SesResult<()> {
        let reason = reason.to_ascii_uppercase();
        if reason != "BOUNCE" && reason != "COMPLAINT" {
            return Err(bad_request("Reason must be BOUNCE or COMPLAINT."));
        }
        let e = bare_address(email).to_ascii_lowercase();
        if !e.contains('@') {
            return Err(bad_request(format!("Invalid email address: {email}")));
        }
        let now = Utc::now().to_rfc3339();
        self.db()
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO ses_suppressed_destinations
                     (email, reason, message_id, feedback_id, last_update_time)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(email) DO UPDATE SET reason = excluded.reason,
                       message_id = excluded.message_id, feedback_id = excluded.feedback_id,
                       last_update_time = excluded.last_update_time",
                    rusqlite::params![e, reason, message_id, feedback_id, now],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// `(email, reason, message_id, feedback_id, last_update_time)`.
    pub async fn get_suppressed(
        &self,
        email: &str,
    ) -> SesResult<Option<(String, String, Option<String>, Option<String>, String)>> {
        let e = bare_address(email).to_ascii_lowercase();
        Ok(self
            .db()
            .call(move |conn| {
                conn.query_row(
                    "SELECT email, reason, message_id, feedback_id, last_update_time
                     FROM ses_suppressed_destinations WHERE email = ?1",
                    rusqlite::params![e],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .optional()
            })
            .await?)
    }

    /// `(email, reason, last_update_time)` ordered by email.
    pub async fn list_suppressed(&self) -> SesResult<Vec<(String, String, String)>> {
        Ok(self
            .db()
            .call(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT email, reason, last_update_time
                     FROM ses_suppressed_destinations ORDER BY email",
                )?;
                let rows = stmt
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }

    pub async fn delete_suppressed(&self, email: &str) -> SesResult<bool> {
        let e = bare_address(email).to_ascii_lowercase();
        Ok(self
            .db()
            .call(move |conn| {
                Ok(conn.execute(
                    "DELETE FROM ses_suppressed_destinations WHERE email = ?1",
                    rusqlite::params![e],
                )? > 0)
            })
            .await?)
    }

    // ── Configuration sets ────────────────────────────────────────────────

    pub async fn create_configuration_set(&self, name: &str, options: Value) -> SesResult<()> {
        if name.is_empty() {
            return Err(bad_request("ConfigurationSetName is required."));
        }
        if self.get_configuration_set(name).await?.is_some() {
            return Err(SesError::new(
                SesErrorKind::AlreadyExists,
                format!("Configuration set <{name}> already exists."),
            ));
        }
        let n = name.to_string();
        let o = options.to_string();
        let now = Utc::now().to_rfc3339();
        self.db()
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO ses_configuration_sets (name, options, created_at) VALUES (?1, ?2, ?3)",
                    rusqlite::params![n, o, now],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn get_configuration_set(&self, name: &str) -> SesResult<Option<Value>> {
        let n = name.to_string();
        let row: Option<String> = self
            .db()
            .call(move |conn| {
                conn.query_row(
                    "SELECT options FROM ses_configuration_sets WHERE name = ?1",
                    rusqlite::params![n],
                    |r| r.get(0),
                )
                .optional()
            })
            .await?;
        Ok(row.map(|o| serde_json::from_str(&o).unwrap_or_else(|_| json!({}))))
    }

    pub async fn require_configuration_set(&self, name: &str) -> SesResult<Value> {
        self.get_configuration_set(name).await?.ok_or_else(|| {
            SesError::new(
                SesErrorKind::ConfigurationSetDoesNotExist,
                format!("Configuration set <{name}> does not exist."),
            )
        })
    }

    pub async fn update_configuration_set_options(
        &self,
        name: &str,
        patch: Value,
    ) -> SesResult<()> {
        let mut opts = self.require_configuration_set(name).await?;
        if let (Some(o), Some(p)) = (opts.as_object_mut(), patch.as_object()) {
            for (k, v) in p {
                o.insert(k.clone(), v.clone());
            }
        }
        let n = name.to_string();
        let s = opts.to_string();
        self.db()
            .call(move |conn| {
                conn.execute(
                    "UPDATE ses_configuration_sets SET options = ?2 WHERE name = ?1",
                    rusqlite::params![n, s],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn list_configuration_sets(&self) -> SesResult<Vec<String>> {
        Ok(self
            .db()
            .call(|conn| {
                let mut stmt =
                    conn.prepare("SELECT name FROM ses_configuration_sets ORDER BY name")?;
                let rows = stmt
                    .query_map([], |r| r.get(0))?
                    .collect::<rusqlite::Result<Vec<String>>>()?;
                Ok(rows)
            })
            .await?)
    }

    pub async fn delete_configuration_set(&self, name: &str) -> SesResult<()> {
        self.require_configuration_set(name).await?;
        let n = name.to_string();
        self.db()
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM ses_configuration_sets WHERE name = ?1",
                    rusqlite::params![n],
                )?;
                conn.execute(
                    "DELETE FROM ses_event_destinations WHERE configuration_set = ?1",
                    rusqlite::params![n],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Validate an event destination definition (`EventDestinationDefinition`).
    async fn validate_destination(&self, def: &Value) -> SesResult<()> {
        let types = def
            .get("MatchingEventTypes")
            .and_then(|v| v.as_array())
            .filter(|a| !a.is_empty())
            .ok_or_else(|| bad_request("MatchingEventTypes is required."))?;
        for t in types {
            let t = t.as_str().unwrap_or("");
            if super::events::event_type_name(t).is_none() {
                return Err(bad_request(format!("Invalid event type: {t}")));
            }
        }
        if let Some(topic) = def
            .get("SnsDestination")
            .and_then(|d| d.get("TopicArn"))
            .and_then(|v| v.as_str())
        {
            if !self.topic_exists(topic).await? {
                return Err(bad_request(format!("Could not find SNS topic: {topic}")));
            }
        }
        Ok(())
    }

    pub async fn topic_exists(&self, topic_arn: &str) -> SesResult<bool> {
        let t = topic_arn.to_string();
        Ok(self
            .db()
            .call(move |conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM sns_topics WHERE arn = ?1",
                    rusqlite::params![t],
                    |r| r.get::<_, i64>(0),
                )
            })
            .await
            .map(|n| n > 0)
            .unwrap_or(false))
    }

    pub async fn create_event_destination(
        &self,
        cs: &str,
        name: &str,
        def: Value,
    ) -> SesResult<()> {
        self.require_configuration_set(cs).await?;
        if name.is_empty() {
            return Err(bad_request("EventDestinationName is required."));
        }
        if self
            .event_destinations(cs)
            .await?
            .iter()
            .any(|(n, _)| n == name)
        {
            return Err(SesError::new(
                SesErrorKind::AlreadyExists,
                format!("Event destination {name} already exists in configuration set {cs}."),
            ));
        }
        self.validate_destination(&def).await?;
        self.write_event_destination(cs, name, def).await
    }

    pub async fn update_event_destination(
        &self,
        cs: &str,
        name: &str,
        def: Value,
    ) -> SesResult<()> {
        self.require_configuration_set(cs).await?;
        if !self
            .event_destinations(cs)
            .await?
            .iter()
            .any(|(n, _)| n == name)
        {
            return Err(SesError::new(
                SesErrorKind::EventDestinationDoesNotExist,
                format!("Event destination {name} does not exist."),
            ));
        }
        self.validate_destination(&def).await?;
        self.write_event_destination(cs, name, def).await
    }

    async fn write_event_destination(&self, cs: &str, name: &str, def: Value) -> SesResult<()> {
        let (c, n, d) = (cs.to_string(), name.to_string(), def.to_string());
        self.db()
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO ses_event_destinations (configuration_set, name, definition)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(configuration_set, name) DO UPDATE SET definition = excluded.definition",
                    rusqlite::params![c, n, d],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn event_destinations(&self, cs: &str) -> SesResult<Vec<(String, Value)>> {
        let c = cs.to_string();
        let rows: Vec<(String, String)> = self
            .db()
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT name, definition FROM ses_event_destinations
                     WHERE configuration_set = ?1 ORDER BY name",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![c], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;
        Ok(rows
            .into_iter()
            .map(|(n, d)| (n, serde_json::from_str(&d).unwrap_or_else(|_| json!({}))))
            .collect())
    }

    pub async fn delete_event_destination(&self, cs: &str, name: &str) -> SesResult<()> {
        self.require_configuration_set(cs).await?;
        let (c, n) = (cs.to_string(), name.to_string());
        let deleted = self
            .db()
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM ses_event_destinations WHERE configuration_set = ?1 AND name = ?2",
                    rusqlite::params![c, n],
                )
            })
            .await?;
        if deleted == 0 {
            return Err(SesError::new(
                SesErrorKind::EventDestinationDoesNotExist,
                format!("Event destination {name} does not exist."),
            ));
        }
        Ok(())
    }

    // ── Event log ─────────────────────────────────────────────────────────

    pub async fn record_event(
        &self,
        message_id: &str,
        event_type: &str,
        cs: Option<&str>,
        payload: &Value,
    ) {
        let (m, t, c, p) = (
            message_id.to_string(),
            event_type.to_string(),
            cs.map(|s| s.to_string()),
            payload.to_string(),
        );
        let now = Utc::now().to_rfc3339();
        if let Err(e) = self
            .db()
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO ses_events (message_id, event_type, configuration_set, payload, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![m, t, c, p, now],
                )?;
                Ok(())
            })
            .await
        {
            tracing::warn!("failed to record SES event: {e}");
        }
    }

    /// Recorded events, newest last, optionally filtered.
    pub async fn list_events(
        &self,
        message_id: Option<String>,
        event_type: Option<String>,
    ) -> SesResult<Vec<Value>> {
        Ok(self
            .db()
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT message_id, event_type, configuration_set, payload, created_at
                     FROM ses_events
                     WHERE (?1 IS NULL OR message_id = ?1) AND (?2 IS NULL OR event_type = ?2)
                     ORDER BY id",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![message_id, event_type], |r| {
                        let payload: String = r.get(3)?;
                        Ok(json!({
                            "message_id": r.get::<_, String>(0)?,
                            "event_type": r.get::<_, String>(1)?,
                            "configuration_set": r.get::<_, Option<String>>(2)?,
                            "event": serde_json::from_str::<Value>(&payload).unwrap_or(Value::Null),
                            "created_at": r.get::<_, String>(4)?,
                        }))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }
}

fn base64_token() -> String {
    use base64::Engine;
    let bytes: [u8; 32] = rand::thread_rng().gen();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

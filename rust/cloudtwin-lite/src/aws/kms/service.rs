//! KMS business logic: key lifecycle, aliases, and AES-256-GCM envelope crypto.
//!
//! Ciphertext blob layout (version 1):
//! `b"CTK" | 0x01 | key id (36 ASCII bytes) | nonce (12 bytes) | AES-GCM ciphertext+tag`.
//! The encryption context is bound as AAD (canonical sorted JSON, empty when absent).

use std::collections::BTreeMap;

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use rand::RngCore;
use rusqlite::{params, OptionalExtension, Row};
use sha2::{Digest, Sha256};

use super::models::{default_policy, KmsAlias, KmsKey};
use crate::db::Database;

const MAGIC: &[u8; 4] = b"CTK\x01";
const ID_LEN: usize = 36;
const NONCE_LEN: usize = 12;
const HEADER_LEN: usize = MAGIC.len() + ID_LEN + NONCE_LEN;

pub const MAX_ENCRYPT_PLAINTEXT: usize = 4096;

const KEY_SPECS: &[&str] = &[
    "SYMMETRIC_DEFAULT",
    "RSA_2048",
    "RSA_3072",
    "RSA_4096",
    "ECC_NIST_P256",
    "ECC_NIST_P384",
    "ECC_NIST_P521",
    "ECC_SECG_P256K1",
    "HMAC_224",
    "HMAC_256",
    "HMAC_384",
    "HMAC_512",
    "SM2",
];
const KEY_USAGES: &[&str] = &[
    "ENCRYPT_DECRYPT",
    "SIGN_VERIFY",
    "GENERATE_VERIFY_MAC",
    "KEY_AGREEMENT",
];

#[derive(Debug)]
pub enum KmsError {
    NotFound(String),
    AlreadyExists(String),
    Disabled(String),
    InvalidState(String),
    InvalidCiphertext(String),
    InvalidKeyUsage(String),
    IncorrectKey(String),
    Validation(String),
    Unsupported(String),
    InvalidAliasName(String),
    InvalidMarker(String),
    MalformedPolicy(String),
    Internal(String),
}

impl KmsError {
    pub fn code(&self) -> &'static str {
        match self {
            KmsError::NotFound(_) => "NotFoundException",
            KmsError::AlreadyExists(_) => "AlreadyExistsException",
            KmsError::Disabled(_) => "DisabledException",
            KmsError::InvalidState(_) => "KMSInvalidStateException",
            KmsError::InvalidCiphertext(_) => "InvalidCiphertextException",
            KmsError::InvalidKeyUsage(_) => "InvalidKeyUsageException",
            KmsError::IncorrectKey(_) => "IncorrectKeyException",
            KmsError::Validation(_) => "ValidationException",
            KmsError::Unsupported(_) => "UnsupportedOperationException",
            KmsError::InvalidAliasName(_) => "InvalidAliasNameException",
            KmsError::InvalidMarker(_) => "InvalidMarkerException",
            KmsError::MalformedPolicy(_) => "MalformedPolicyDocumentException",
            KmsError::Internal(_) => "KMSInternalException",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            KmsError::NotFound(m)
            | KmsError::AlreadyExists(m)
            | KmsError::Disabled(m)
            | KmsError::InvalidState(m)
            | KmsError::InvalidCiphertext(m)
            | KmsError::InvalidKeyUsage(m)
            | KmsError::IncorrectKey(m)
            | KmsError::Validation(m)
            | KmsError::Unsupported(m)
            | KmsError::InvalidAliasName(m)
            | KmsError::InvalidMarker(m)
            | KmsError::MalformedPolicy(m)
            | KmsError::Internal(m) => m,
        }
    }
}

impl std::fmt::Display for KmsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for KmsError {}

impl From<anyhow::Error> for KmsError {
    fn from(e: anyhow::Error) -> Self {
        KmsError::Internal(e.to_string())
    }
}

pub type KmsResult<T> = Result<T, KmsError>;

#[derive(Debug, Default)]
pub struct CreateKeyInput {
    pub id: Option<String>,
    pub description: Option<String>,
    pub key_usage: Option<String>,
    pub key_spec: Option<String>,
    pub origin: Option<String>,
    pub policy: Option<String>,
    pub tags: Vec<(String, String)>,
    pub multi_region: bool,
    /// Explicit key material (seeded keys); random when `None`.
    pub material: Option<Vec<u8>>,
}

pub struct KmsService {
    db: Database,
}

pub fn now_secs() -> f64 {
    let now = chrono::Utc::now();
    now.timestamp() as f64 + f64::from(now.timestamp_subsec_millis()) / 1000.0
}

pub fn random_bytes(n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut v);
    v
}

/// Deterministic emulator-only key material for seeded keys with a fixed id, so
/// ciphertexts survive restarts and resets. Not secret.
pub fn seeded_material(id: &str) -> Vec<u8> {
    Sha256::digest(format!("cloudtwin-kms-seed:{id}").as_bytes()).to_vec()
}

/// Canonical AAD for an encryption context: sorted JSON, empty for no context.
pub fn canonical_context(ctx: &BTreeMap<String, String>) -> Vec<u8> {
    if ctx.is_empty() {
        Vec::new()
    } else {
        serde_json::to_vec(ctx).unwrap_or_default()
    }
}

fn key_from_row(r: &Row) -> rusqlite::Result<KmsKey> {
    let tags_raw: String = r.get("tags")?;
    let tags: Vec<(String, String)> = serde_json::from_str::<Vec<serde_json::Value>>(&tags_raw)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|t| {
            Some((
                t.get("TagKey")?.as_str()?.to_string(),
                t.get("TagValue")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            ))
        })
        .collect();
    Ok(KmsKey {
        id: r.get("id")?,
        description: r.get("description")?,
        key_usage: r.get("key_usage")?,
        key_spec: r.get("key_spec")?,
        origin: r.get("origin")?,
        key_state: r.get("key_state")?,
        created_at: r.get("created_at")?,
        deletion_date: r.get("deletion_date")?,
        pending_window_days: r.get("pending_window_days")?,
        material: r.get("material")?,
        rotation_enabled: r.get::<_, i64>("rotation_enabled")? != 0,
        rotation_period_days: r.get("rotation_period_days")?,
        policy: r.get("policy")?,
        tags,
        multi_region: r.get::<_, i64>("multi_region")? != 0,
    })
}

fn alias_from_row(r: &Row) -> rusqlite::Result<KmsAlias> {
    Ok(KmsAlias {
        name: r.get(0)?,
        target_key_id: r.get(1)?,
        created_at: r.get(2)?,
        updated_at: r.get(3)?,
    })
}

fn not_found_key(id: &str) -> KmsError {
    KmsError::NotFound(format!(
        "Key '{}' does not exist",
        super::models::key_arn(id)
    ))
}

fn not_found_alias(name: &str) -> KmsError {
    KmsError::NotFound(format!(
        "Alias {} is not found.",
        super::models::alias_arn(name)
    ))
}

fn invalid_state(key: &KmsKey) -> KmsError {
    KmsError::InvalidState(format!("{} is pending deletion.", key.arn()))
}

/// Fails unless the key is in a state that permits configuration changes.
fn require_not_pending(key: &KmsKey) -> KmsResult<()> {
    if key.key_state == "PendingDeletion" {
        Err(invalid_state(key))
    } else {
        Ok(())
    }
}

/// Fails unless the key is enabled (cryptographic operations).
fn require_enabled(key: &KmsKey) -> KmsResult<()> {
    match key.key_state.as_str() {
        "Enabled" => Ok(()),
        "Disabled" => Err(KmsError::Disabled(format!("{} is disabled.", key.arn()))),
        _ => Err(invalid_state(key)),
    }
}

/// Fails unless the key can perform symmetric encrypt/decrypt.
fn require_symmetric_crypto(key: &KmsKey) -> KmsResult<()> {
    require_enabled(key)?;
    if key.key_usage != "ENCRYPT_DECRYPT" {
        return Err(KmsError::InvalidKeyUsage(format!(
            "{} key usage is {} which is not valid for this operation.",
            key.arn(),
            key.key_usage
        )));
    }
    if !key.is_symmetric() || key.material.as_ref().map(|m| m.len()) != Some(32) {
        return Err(KmsError::Unsupported(format!(
            "{} has key spec {}; only SYMMETRIC_DEFAULT keys are supported for this operation by CloudTwin.",
            key.arn(),
            key.key_spec
        )));
    }
    Ok(())
}

pub fn validate_alias_name(name: &str) -> KmsResult<()> {
    let Some(rest) = name.strip_prefix("alias/") else {
        return Err(KmsError::InvalidAliasName(format!(
            "Alias must start with the prefix \"alias/\": {name}"
        )));
    };
    if name.starts_with("alias/aws/") {
        return Err(KmsError::InvalidAliasName(
            "Alias names that begin with \"alias/aws/\" are reserved.".into(),
        ));
    }
    if rest.is_empty()
        || name.len() > 256
        || !rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-'))
    {
        return Err(KmsError::InvalidAliasName(format!(
            "Alias name {name} must satisfy regular expression pattern: ^alias/[a-zA-Z0-9/_-]+$"
        )));
    }
    Ok(())
}

enum KeyRef {
    Id(String),
    Alias(String),
}

fn parse_key_ref(key_ref: &str) -> KmsResult<KeyRef> {
    let r = key_ref.trim();
    if r.is_empty() {
        return Err(KmsError::Validation("KeyId must not be empty".into()));
    }
    if let Some(rest) = r.strip_prefix("arn:") {
        // arn:aws:kms:<region>:<account>:key/<id> | alias/<name>
        let resource = rest.splitn(5, ':').nth(4).unwrap_or("");
        if let Some(id) = resource.strip_prefix("key/") {
            return Ok(KeyRef::Id(id.to_string()));
        }
        if resource.starts_with("alias/") {
            return Ok(KeyRef::Alias(resource.to_string()));
        }
        return Err(KmsError::NotFound(format!("Invalid keyId {r}")));
    }
    if r.starts_with("alias/") {
        return Ok(KeyRef::Alias(r.to_string()));
    }
    Ok(KeyRef::Id(r.to_string()))
}

impl KmsService {
    pub fn new(db: Database) -> Self {
        KmsService { db }
    }

    /// Delete keys whose scheduled deletion date has passed, with their aliases.
    pub async fn purge_expired(&self) -> KmsResult<()> {
        let now = now_secs();
        self.db
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM kms_aliases WHERE target_key_id IN (SELECT id FROM kms_keys \
                     WHERE key_state = 'PendingDeletion' AND deletion_date <= ?1)",
                    params![now],
                )?;
                conn.execute(
                    "DELETE FROM kms_keys WHERE key_state = 'PendingDeletion' AND deletion_date <= ?1",
                    params![now],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn load_key(&self, id: &str) -> KmsResult<Option<KmsKey>> {
        let id = id.to_string();
        Ok(self
            .db
            .call(move |conn| {
                conn.query_row(
                    "SELECT * FROM kms_keys WHERE id = ?1",
                    params![id],
                    key_from_row,
                )
                .optional()
            })
            .await?)
    }

    pub async fn get_alias(&self, name: &str) -> KmsResult<Option<KmsAlias>> {
        let name = name.to_string();
        Ok(self
            .db
            .call(move |conn| {
                conn.query_row(
                    "SELECT name, target_key_id, created_at, updated_at FROM kms_aliases WHERE name = ?1",
                    params![name],
                    alias_from_row,
                )
                .optional()
            })
            .await?)
    }

    /// Resolve a key id, key ARN, alias name or alias ARN.
    pub async fn resolve(&self, key_ref: &str) -> KmsResult<KmsKey> {
        match parse_key_ref(key_ref)? {
            KeyRef::Id(id) => self.load_key(&id).await?.ok_or_else(|| not_found_key(&id)),
            KeyRef::Alias(name) => {
                let alias = self
                    .get_alias(&name)
                    .await?
                    .ok_or_else(|| not_found_alias(&name))?;
                self.load_key(&alias.target_key_id)
                    .await?
                    .ok_or_else(|| not_found_alias(&name))
            }
        }
    }

    async fn save(&self, key: &KmsKey) -> KmsResult<()> {
        let k = key.clone();
        let tags = key.tags_json().to_string();
        self.db
            .call(move |conn| {
                conn.execute(
                    "UPDATE kms_keys SET description = ?2, key_state = ?3, deletion_date = ?4, \
                     pending_window_days = ?5, rotation_enabled = ?6, rotation_period_days = ?7, \
                     policy = ?8, tags = ?9 WHERE id = ?1",
                    params![
                        k.id,
                        k.description,
                        k.key_state,
                        k.deletion_date,
                        k.pending_window_days,
                        k.rotation_enabled as i64,
                        k.rotation_period_days,
                        k.policy,
                        tags
                    ],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    // ── Key lifecycle ────────────────────────────────────────────────────────

    pub async fn create_key(&self, input: CreateKeyInput) -> KmsResult<KmsKey> {
        let key_spec = input.key_spec.unwrap_or_else(|| "SYMMETRIC_DEFAULT".into());
        if !KEY_SPECS.contains(&key_spec.as_str()) {
            return Err(KmsError::Validation(format!(
                "1 validation error detected: Value '{key_spec}' at 'keySpec' failed to satisfy constraint: \
                 Member must satisfy enum value set: {KEY_SPECS:?}"
            )));
        }
        let default_usage = if key_spec.starts_with("HMAC_") {
            "GENERATE_VERIFY_MAC"
        } else if key_spec.starts_with("ECC_") {
            "SIGN_VERIFY"
        } else {
            "ENCRYPT_DECRYPT"
        };
        let key_usage = input.key_usage.unwrap_or_else(|| default_usage.into());
        if !KEY_USAGES.contains(&key_usage.as_str()) {
            return Err(KmsError::Validation(format!(
                "1 validation error detected: Value '{key_usage}' at 'keyUsage' failed to satisfy constraint: \
                 Member must satisfy enum value set: {KEY_USAGES:?}"
            )));
        }
        let compatible = match key_spec.as_str() {
            "SYMMETRIC_DEFAULT" => key_usage == "ENCRYPT_DECRYPT",
            s if s.starts_with("HMAC_") => key_usage == "GENERATE_VERIFY_MAC",
            s if s.starts_with("RSA_") => {
                matches!(key_usage.as_str(), "ENCRYPT_DECRYPT" | "SIGN_VERIFY")
            }
            "SM2" => key_usage != "GENERATE_VERIFY_MAC",
            _ => matches!(key_usage.as_str(), "SIGN_VERIFY" | "KEY_AGREEMENT"),
        };
        if !compatible {
            return Err(KmsError::Validation(format!(
                "KeyUsage {key_usage} is not compatible with KeySpec {key_spec}"
            )));
        }
        let origin = input.origin.unwrap_or_else(|| "AWS_KMS".into());
        if origin != "AWS_KMS" {
            return Err(KmsError::Unsupported(format!(
                "Origin {origin} is not supported by CloudTwin; only AWS_KMS is."
            )));
        }
        let description = input.description.unwrap_or_default();
        if description.len() > 8192 {
            return Err(KmsError::Validation(
                "Description must be at most 8192 characters".into(),
            ));
        }
        let policy = match input.policy {
            Some(p) => {
                validate_policy(&p)?;
                p
            }
            None => default_policy(),
        };
        let id = input.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let material = if key_spec == "SYMMETRIC_DEFAULT" {
            Some(input.material.unwrap_or_else(|| random_bytes(32)))
        } else if key_spec.starts_with("HMAC_") {
            Some(random_bytes(64))
        } else {
            None
        };
        let key = KmsKey {
            id,
            description,
            key_usage,
            key_spec,
            origin,
            key_state: "Enabled".into(),
            created_at: now_secs(),
            deletion_date: None,
            pending_window_days: None,
            material,
            rotation_enabled: false,
            rotation_period_days: 365,
            policy,
            tags: dedupe_tags(input.tags),
            multi_region: input.multi_region,
        };
        let k = key.clone();
        let tags = key.tags_json().to_string();
        let inserted = self
            .db
            .call(move |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO kms_keys (id, description, key_usage, key_spec, origin, key_state, \
                     created_at, material, policy, tags, multi_region) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![
                        k.id,
                        k.description,
                        k.key_usage,
                        k.key_spec,
                        k.origin,
                        k.key_state,
                        k.created_at,
                        k.material,
                        k.policy,
                        tags,
                        k.multi_region as i64
                    ],
                )
            })
            .await?;
        if inserted == 0 {
            return Err(KmsError::AlreadyExists(format!(
                "Key {} already exists",
                key.arn()
            )));
        }
        Ok(key)
    }

    /// All key ids in creation order.
    pub async fn list_key_ids(&self) -> KmsResult<Vec<String>> {
        Ok(self
            .db
            .call(|conn| {
                let mut stmt =
                    conn.prepare("SELECT id FROM kms_keys ORDER BY created_at, rowid")?;
                let ids = stmt
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(ids)
            })
            .await?)
    }

    pub async fn set_enabled(&self, key_ref: &str, enabled: bool) -> KmsResult<KmsKey> {
        let mut key = self.resolve(key_ref).await?;
        require_not_pending(&key)?;
        key.key_state = if enabled { "Enabled" } else { "Disabled" }.into();
        self.save(&key).await?;
        Ok(key)
    }

    pub async fn schedule_deletion(&self, key_ref: &str, days: Option<i64>) -> KmsResult<KmsKey> {
        let days = days.unwrap_or(30);
        if !(7..=30).contains(&days) {
            return Err(KmsError::Validation(format!(
                "1 validation error detected: Value '{days}' at 'pendingWindowInDays' failed to satisfy \
                 constraint: Member must have value between 7 and 30"
            )));
        }
        let mut key = self.resolve(key_ref).await?;
        require_not_pending(&key)?;
        let deletion = (chrono::Utc::now() + chrono::Duration::days(days)).timestamp() as f64;
        key.key_state = "PendingDeletion".into();
        key.deletion_date = Some(deletion);
        key.pending_window_days = Some(days);
        self.save(&key).await?;
        Ok(key)
    }

    pub async fn cancel_deletion(&self, key_ref: &str) -> KmsResult<KmsKey> {
        let mut key = self.resolve(key_ref).await?;
        if key.key_state != "PendingDeletion" {
            return Err(KmsError::InvalidState(format!(
                "{} is not pending deletion (current state: {}).",
                key.arn(),
                key.key_state
            )));
        }
        key.key_state = "Disabled".into();
        key.deletion_date = None;
        key.pending_window_days = None;
        self.save(&key).await?;
        Ok(key)
    }

    pub async fn set_rotation(
        &self,
        key_ref: &str,
        enabled: bool,
        period: Option<i64>,
    ) -> KmsResult<KmsKey> {
        let mut key = self.resolve(key_ref).await?;
        require_enabled(&key)?;
        if !key.is_symmetric() {
            return Err(KmsError::Unsupported(format!(
                "{} does not support automatic key rotation.",
                key.arn()
            )));
        }
        if let Some(p) = period {
            if !(90..=2560).contains(&p) {
                return Err(KmsError::Validation(
                    "RotationPeriodInDays must be between 90 and 2560".into(),
                ));
            }
            key.rotation_period_days = p;
        }
        key.rotation_enabled = enabled;
        self.save(&key).await?;
        Ok(key)
    }

    pub async fn update_description(
        &self,
        key_ref: &str,
        description: String,
    ) -> KmsResult<KmsKey> {
        if description.len() > 8192 {
            return Err(KmsError::Validation(
                "Description must be at most 8192 characters".into(),
            ));
        }
        let mut key = self.resolve(key_ref).await?;
        require_not_pending(&key)?;
        key.description = description;
        self.save(&key).await?;
        Ok(key)
    }

    pub async fn tag(&self, key_ref: &str, tags: Vec<(String, String)>) -> KmsResult<KmsKey> {
        let mut key = self.resolve(key_ref).await?;
        require_not_pending(&key)?;
        for (k, v) in tags {
            if k.is_empty() || k.len() > 128 || v.len() > 256 {
                return Err(KmsError::Validation(format!("Invalid tag: {k}")));
            }
            match key.tags.iter_mut().find(|(ek, _)| *ek == k) {
                Some(existing) => existing.1 = v,
                None => key.tags.push((k, v)),
            }
        }
        if key.tags.len() > 50 {
            return Err(KmsError::Validation(
                "A key can have at most 50 tags".into(),
            ));
        }
        self.save(&key).await?;
        Ok(key)
    }

    pub async fn untag(&self, key_ref: &str, tag_keys: Vec<String>) -> KmsResult<KmsKey> {
        let mut key = self.resolve(key_ref).await?;
        require_not_pending(&key)?;
        key.tags.retain(|(k, _)| !tag_keys.contains(k));
        self.save(&key).await?;
        Ok(key)
    }

    pub async fn put_policy(
        &self,
        key_ref: &str,
        policy_name: &str,
        policy: String,
    ) -> KmsResult<KmsKey> {
        require_default_policy_name(policy_name)?;
        validate_policy(&policy)?;
        let mut key = self.resolve(key_ref).await?;
        require_not_pending(&key)?;
        key.policy = policy;
        self.save(&key).await?;
        Ok(key)
    }

    // ── Aliases ──────────────────────────────────────────────────────────────

    pub async fn create_alias(&self, name: &str, target_ref: &str) -> KmsResult<KmsAlias> {
        validate_alias_name(name)?;
        let key = self.resolve(target_ref).await?;
        require_not_pending(&key)?;
        let now = now_secs();
        let alias = KmsAlias {
            name: name.to_string(),
            target_key_id: key.id.clone(),
            created_at: now,
            updated_at: now,
        };
        let a = alias.clone();
        let inserted = self
            .db
            .call(move |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO kms_aliases (name, target_key_id, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, ?4)",
                    params![a.name, a.target_key_id, a.created_at, a.updated_at],
                )
            })
            .await?;
        if inserted == 0 {
            return Err(KmsError::AlreadyExists(format!(
                "An alias with the name {} already exists",
                super::models::alias_arn(name)
            )));
        }
        Ok(alias)
    }

    pub async fn update_alias(&self, name: &str, target_ref: &str) -> KmsResult<KmsAlias> {
        validate_alias_name(name)?;
        let mut alias = self
            .get_alias(name)
            .await?
            .ok_or_else(|| not_found_alias(name))?;
        let key = self.resolve(target_ref).await?;
        require_not_pending(&key)?;
        alias.target_key_id = key.id;
        alias.updated_at = now_secs();
        let a = alias.clone();
        self.db
            .call(move |conn| {
                conn.execute(
                    "UPDATE kms_aliases SET target_key_id = ?2, updated_at = ?3 WHERE name = ?1",
                    params![a.name, a.target_key_id, a.updated_at],
                )
            })
            .await?;
        Ok(alias)
    }

    pub async fn delete_alias(&self, name: &str) -> KmsResult<()> {
        if !name.starts_with("alias/") {
            return Err(KmsError::InvalidAliasName(format!(
                "Alias must start with the prefix \"alias/\": {name}"
            )));
        }
        let n = name.to_string();
        let deleted = self
            .db
            .call(move |conn| conn.execute("DELETE FROM kms_aliases WHERE name = ?1", params![n]))
            .await?;
        if deleted == 0 {
            return Err(not_found_alias(name));
        }
        Ok(())
    }

    pub async fn list_aliases(&self, key_id: Option<String>) -> KmsResult<Vec<KmsAlias>> {
        Ok(self
            .db
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT name, target_key_id, created_at, updated_at FROM kms_aliases \
                     WHERE ?1 IS NULL OR target_key_id = ?1 ORDER BY name",
                )?;
                let rows = stmt
                    .query_map(params![key_id], alias_from_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }

    // ── Cryptography ─────────────────────────────────────────────────────────

    fn seal(key: &KmsKey, plaintext: &[u8], ctx: &BTreeMap<String, String>) -> KmsResult<Vec<u8>> {
        require_symmetric_crypto(key)?;
        let material = key.material.as_deref().unwrap_or_default();
        let cipher = Aes256Gcm::new_from_slice(material)
            .map_err(|e| KmsError::Internal(format!("bad key material: {e}")))?;
        let nonce = random_bytes(NONCE_LEN);
        let aad = canonical_context(ctx);
        let sealed = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|e| KmsError::Internal(format!("encryption failed: {e}")))?;
        let mut blob = Vec::with_capacity(HEADER_LEN + sealed.len());
        blob.extend_from_slice(MAGIC);
        blob.extend_from_slice(key.id.as_bytes());
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&sealed);
        Ok(blob)
    }

    pub async fn encrypt(
        &self,
        key_ref: &str,
        plaintext: &[u8],
        ctx: &BTreeMap<String, String>,
    ) -> KmsResult<(KmsKey, Vec<u8>)> {
        if plaintext.is_empty() || plaintext.len() > MAX_ENCRYPT_PLAINTEXT {
            return Err(KmsError::Validation(format!(
                "1 validation error detected: Value at 'plaintext' failed to satisfy constraint: \
                 Member must have length between 1 and {MAX_ENCRYPT_PLAINTEXT}"
            )));
        }
        let key = self.resolve(key_ref).await?;
        let blob = Self::seal(&key, plaintext, ctx)?;
        Ok((key, blob))
    }

    /// Decrypt a blob produced by `encrypt`. When `key_ref` is given it must
    /// identify the key that produced the blob.
    pub async fn decrypt(
        &self,
        blob: &[u8],
        ctx: &BTreeMap<String, String>,
        key_ref: Option<&str>,
    ) -> KmsResult<(KmsKey, Vec<u8>)> {
        let invalid = || KmsError::InvalidCiphertext(String::new());
        if blob.len() < HEADER_LEN + 16 || &blob[..MAGIC.len()] != MAGIC {
            return Err(invalid());
        }
        let id =
            std::str::from_utf8(&blob[MAGIC.len()..MAGIC.len() + ID_LEN]).map_err(|_| invalid())?;
        let key = self.load_key(id).await?.ok_or_else(|| not_found_key(id))?;
        if let Some(r) = key_ref {
            let requested = self.resolve(r).await?;
            if requested.id != key.id {
                return Err(KmsError::IncorrectKey(
                    "The key ID in the request does not identify a KMS key that can perform this operation."
                        .into(),
                ));
            }
        }
        require_symmetric_crypto(&key)?;
        let cipher = Aes256Gcm::new_from_slice(key.material.as_deref().unwrap_or_default())
            .map_err(|e| KmsError::Internal(format!("bad key material: {e}")))?;
        let nonce = &blob[MAGIC.len() + ID_LEN..HEADER_LEN];
        let aad = canonical_context(ctx);
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: &blob[HEADER_LEN..],
                    aad: &aad,
                },
            )
            .map_err(|_| invalid())?;
        Ok((key, plaintext))
    }

    pub async fn re_encrypt(
        &self,
        blob: &[u8],
        source_ctx: &BTreeMap<String, String>,
        source_key_ref: Option<&str>,
        dest_key_ref: &str,
        dest_ctx: &BTreeMap<String, String>,
    ) -> KmsResult<(KmsKey, KmsKey, Vec<u8>)> {
        let (source, plaintext) = self.decrypt(blob, source_ctx, source_key_ref).await?;
        let dest = self.resolve(dest_key_ref).await?;
        let out = Self::seal(&dest, &plaintext, dest_ctx)?;
        Ok((source, dest, out))
    }

    /// Returns `(key, plaintext data key, encrypted data key)`.
    pub async fn generate_data_key(
        &self,
        key_ref: &str,
        num_bytes: usize,
        ctx: &BTreeMap<String, String>,
    ) -> KmsResult<(KmsKey, Vec<u8>, Vec<u8>)> {
        let key = self.resolve(key_ref).await?;
        let plaintext = random_bytes(num_bytes);
        let blob = Self::seal(&key, &plaintext, ctx)?;
        Ok((key, plaintext, blob))
    }
}

pub fn require_default_policy_name(name: &str) -> KmsResult<()> {
    if name == "default" {
        Ok(())
    } else {
        Err(KmsError::NotFound(format!("No such policy exists: {name}")))
    }
}

fn validate_policy(policy: &str) -> KmsResult<()> {
    match serde_json::from_str::<serde_json::Value>(policy) {
        Ok(v) if v.is_object() => Ok(()),
        _ => Err(KmsError::MalformedPolicy(
            "The policy document is not valid JSON.".into(),
        )),
    }
}

fn dedupe_tags(tags: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (k, v) in tags {
        match out.iter_mut().find(|(ek, _)| *ek == k) {
            Some(e) => e.1 = v,
            None => out.push((k, v)),
        }
    }
    out
}

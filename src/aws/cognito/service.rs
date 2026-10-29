//! Cognito business logic: pools, clients, users, passwords, sessions and tokens.
//!
//! Everything here is synchronous and runs on the database thread inside
//! `Database::call`, so each operation is atomic.

use std::sync::Arc;

use rand::{seq::SliceRandom, Rng};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::models::{now_f, now_i, Client, Pool, TokenKind, User};
use super::tokens::SigningKeys;
use super::CognitoConfig;
use crate::aws::REGION;
use crate::AppState;

// ─── Errors ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CogError {
    pub code: &'static str,
    pub msg: String,
}

pub type CResult<T> = Result<T, CogError>;

pub fn err(code: &'static str, msg: impl Into<String>) -> CogError {
    CogError {
        code,
        msg: msg.into(),
    }
}

impl From<rusqlite::Error> for CogError {
    fn from(e: rusqlite::Error) -> Self {
        err("InternalErrorException", e.to_string())
    }
}

pub fn not_authorized(msg: &str) -> CogError {
    err("NotAuthorizedException", msg)
}

pub const BAD_CREDENTIALS: &str = "Incorrect username or password.";

/// Run a service operation in a transaction on the database thread; any error
/// rolls the whole operation back.
pub async fn run<T: Send + 'static>(
    state: &AppState,
    f: impl FnOnce(&Connection) -> CResult<T> + Send + 'static,
) -> CResult<T> {
    let res = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let r = f(&tx);
            if r.is_ok() {
                tx.commit()?;
            }
            Ok(r)
        })
        .await;
    match res {
        Ok(r) => r,
        Err(e) => Err(err("InternalErrorException", e.to_string())),
    }
}

// ─── Context ────────────────────────────────────────────────────────────────

/// Per-request context needed to mint and verify tokens.
#[derive(Clone)]
pub struct Ctx {
    pub keys: Arc<SigningKeys>,
    pub issuer_base: String,
    pub base_url: String,
    pub cfg: CognitoConfig,
}

impl Ctx {
    pub async fn load(state: &AppState) -> CResult<Ctx> {
        let keys = super::tokens::keys(state)
            .await
            .map_err(|e| err("InternalErrorException", e.to_string()))?;
        let base_url = state.cfg.base_url.trim_end_matches('/').to_string();
        let issuer_base = state
            .cfg
            .cognito
            .issuer_base_url
            .clone()
            .map(|s| s.trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| base_url.clone());
        Ok(Ctx {
            keys,
            issuer_base,
            base_url,
            cfg: state.cfg.cognito.clone(),
        })
    }

    pub fn issuer(&self, pool_id: &str) -> String {
        format!("{}/{pool_id}", self.issuer_base)
    }
}

// ─── Random identifiers ─────────────────────────────────────────────────────

const ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const LOWER_ALNUM: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

fn rand_from(charset: &[u8], n: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..n)
        .map(|_| charset[rng.gen_range(0..charset.len())] as char)
        .collect()
}

pub fn new_pool_id() -> String {
    format!("{REGION}_{}", rand_from(ALNUM, 9))
}

pub fn new_client_id() -> String {
    rand_from(LOWER_ALNUM, 26)
}

pub fn new_client_secret() -> String {
    rand_from(LOWER_ALNUM, 51)
}

pub fn random_token(bytes: usize) -> String {
    use base64::Engine;
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill(&mut buf[..]);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

pub fn six_digit_code() -> String {
    format!("{:06}", rand::thread_rng().gen_range(0..1_000_000))
}

// ─── Passwords ──────────────────────────────────────────────────────────────

pub fn hash_password(password: &str, salt: &str) -> String {
    hex::encode(Sha256::digest(format!("{salt}{password}").as_bytes()))
}

fn set_password_fields(user: &mut User, password: &str) {
    let salt = hex::encode(rand::thread_rng().gen::<[u8; 16]>());
    user.password_hash = Some(hash_password(password, &salt));
    user.password_salt = Some(salt);
}

pub fn verify_password(user: &User, password: &str) -> bool {
    match (&user.password_hash, &user.password_salt) {
        (Some(h), Some(s)) => &hash_password(password, s) == h,
        _ => false,
    }
}

pub fn check_policy(pool: &Pool, pw: &str) -> CResult<()> {
    let p = pool.password_policy();
    let bad = |m: &str| {
        err(
            "InvalidPasswordException",
            format!("Password does not conform to policy: {m}"),
        )
    };
    if pw.chars().count() < p.min_length {
        return Err(bad("Password not long enough"));
    }
    if p.upper && !pw.chars().any(|c| c.is_uppercase()) {
        return Err(bad("Password must have uppercase characters"));
    }
    if p.lower && !pw.chars().any(|c| c.is_lowercase()) {
        return Err(bad("Password must have lowercase characters"));
    }
    if p.numbers && !pw.chars().any(|c| c.is_ascii_digit()) {
        return Err(bad("Password must have numeric characters"));
    }
    if p.symbols
        && !pw
            .chars()
            .any(|c| !c.is_alphanumeric() && !c.is_whitespace())
    {
        return Err(bad("Password must have symbol characters"));
    }
    Ok(())
}

pub fn generate_password(pool: &Pool) -> String {
    let p = pool.password_policy();
    let mut rng = rand::thread_rng();
    let mut chars: Vec<char> = Vec::new();
    for set in [
        "ABCDEFGHJKLMNPQRSTUVWXYZ",
        "abcdefghijkmnopqrstuvwxyz",
        "23456789",
        "!#$%^&*-_+=",
    ] {
        let b = set.as_bytes();
        for _ in 0..3 {
            chars.push(b[rng.gen_range(0..b.len())] as char);
        }
    }
    while chars.len() < p.min_length.max(12) {
        chars.push(ALNUM[rng.gen_range(0..ALNUM.len())] as char);
    }
    chars.shuffle(&mut rng);
    chars.into_iter().collect()
}

// ─── Pools ──────────────────────────────────────────────────────────────────

pub fn find_pool(c: &Connection, id: &str) -> rusqlite::Result<Option<Pool>> {
    c.query_row(
        &format!(
            "SELECT {} FROM cognito_user_pools WHERE id = ?1",
            Pool::COLS
        ),
        params![id],
        Pool::from_row,
    )
    .optional()
}

pub fn get_pool(c: &Connection, id: &str) -> CResult<Pool> {
    find_pool(c, id)?.ok_or_else(|| {
        err(
            "ResourceNotFoundException",
            format!("User pool {id} does not exist."),
        )
    })
}

pub fn find_pool_by_name(c: &Connection, name: &str) -> rusqlite::Result<Option<Pool>> {
    c.query_row(
        &format!(
            "SELECT {} FROM cognito_user_pools WHERE name = ?1 ORDER BY created LIMIT 1",
            Pool::COLS
        ),
        params![name],
        Pool::from_row,
    )
    .optional()
}

pub fn list_pools(c: &Connection) -> rusqlite::Result<Vec<Pool>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {} FROM cognito_user_pools ORDER BY created, id",
        Pool::COLS
    ))?;
    let rows = stmt.query_map([], Pool::from_row)?;
    rows.collect()
}

pub fn create_pool(c: &Connection, id: Option<String>, name: &str, data: Value) -> CResult<Pool> {
    if name.is_empty() {
        return Err(err("InvalidParameterException", "PoolName is required."));
    }
    if data.get("UsernameAttributes").is_some() && data.get("AliasAttributes").is_some() {
        return Err(err(
            "InvalidParameterException",
            "Only one of the aliasAttributes or usernameAttributes can be set in a User Pool.",
        ));
    }
    let id = id.unwrap_or_else(new_pool_id);
    if find_pool(c, &id)?.is_some() {
        return Err(err(
            "InvalidParameterException",
            format!("User pool {id} already exists."),
        ));
    }
    let now = now_f();
    c.execute(
        "INSERT INTO cognito_user_pools (id, name, data, domain, created, modified)
         VALUES (?1, ?2, ?3, NULL, ?4, ?4)",
        params![id, name, data.to_string(), now],
    )?;
    get_pool(c, &id)
}

pub fn save_pool(c: &Connection, pool: &Pool) -> CResult<()> {
    c.execute(
        "UPDATE cognito_user_pools SET name = ?2, data = ?3, domain = ?4, modified = ?5 WHERE id = ?1",
        params![pool.id, pool.name, pool.data.to_string(), pool.domain, now_f()],
    )?;
    Ok(())
}

pub fn delete_pool(c: &Connection, id: &str) -> CResult<()> {
    let pool = get_pool(c, id)?;
    if pool.data.get("DeletionProtection").and_then(|v| v.as_str()) == Some("ACTIVE") {
        return Err(err(
            "InvalidParameterException",
            "The user pool cannot be deleted because deletion protection is activated.",
        ));
    }
    for table in [
        "cognito_clients",
        "cognito_users",
        "cognito_auth_codes",
        "cognito_sessions",
        "cognito_origins",
        "cognito_refresh_tokens",
        "cognito_challenges",
        "cognito_codes",
    ] {
        c.execute(
            &format!("DELETE FROM {table} WHERE pool_id = ?1"),
            params![id],
        )?;
    }
    c.execute("DELETE FROM cognito_user_pools WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn count_users(c: &Connection, pool_id: &str) -> rusqlite::Result<i64> {
    c.query_row(
        "SELECT COUNT(*) FROM cognito_users WHERE pool_id = ?1",
        params![pool_id],
        |r| r.get(0),
    )
}

pub fn find_pool_by_domain(c: &Connection, domain: &str) -> rusqlite::Result<Option<Pool>> {
    c.query_row(
        &format!(
            "SELECT {} FROM cognito_user_pools WHERE domain = ?1",
            Pool::COLS
        ),
        params![domain],
        Pool::from_row,
    )
    .optional()
}

// ─── Clients ────────────────────────────────────────────────────────────────

/// Client settings accepted by Create/UpdateUserPoolClient (stored verbatim).
pub const CLIENT_FIELDS: &[&str] = &[
    "RefreshTokenValidity",
    "AccessTokenValidity",
    "IdTokenValidity",
    "TokenValidityUnits",
    "ReadAttributes",
    "WriteAttributes",
    "ExplicitAuthFlows",
    "SupportedIdentityProviders",
    "CallbackURLs",
    "LogoutURLs",
    "DefaultRedirectURI",
    "AllowedOAuthFlows",
    "AllowedOAuthScopes",
    "AllowedOAuthFlowsUserPoolClient",
    "AnalyticsConfiguration",
    "PreventUserExistenceErrors",
    "EnableTokenRevocation",
    "EnablePropagateAdditionalUserContextData",
    "AuthSessionValidity",
];

pub fn client_data_from(req: &Value) -> CResult<Value> {
    let mut data = Map::new();
    for f in CLIENT_FIELDS {
        if let Some(v) = req.get(*f) {
            if !v.is_null() {
                data.insert(f.to_string(), v.clone());
            }
        }
    }
    let data = Value::Object(data);
    let callbacks: Vec<String> = data
        .get("CallbackURLs")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if let Some(d) = data.get("DefaultRedirectURI").and_then(|v| v.as_str()) {
        if !callbacks.iter().any(|c| c == d) {
            return Err(err(
                "InvalidParameterException",
                "DefaultRedirectURI should be one of the CallbackURLs.",
            ));
        }
    }
    if data
        .get("AllowedOAuthFlowsUserPoolClient")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        let flows = data.get("AllowedOAuthFlows").and_then(|v| v.as_array());
        if flows.is_none_or(|f| f.is_empty()) {
            return Err(err(
                "InvalidOAuthFlowException",
                "AllowedOAuthFlows are required when AllowedOAuthFlowsUserPoolClient is true.",
            ));
        }
        let code_or_implicit = flows
            .into_iter()
            .flatten()
            .any(|f| f == "code" || f == "implicit");
        if code_or_implicit && callbacks.is_empty() {
            return Err(err(
                "InvalidParameterException",
                "CallbackURLs are required when the code or implicit grant is enabled.",
            ));
        }
    }
    Ok(data)
}

pub fn find_client(c: &Connection, client_id: &str) -> rusqlite::Result<Option<Client>> {
    c.query_row(
        &format!(
            "SELECT {} FROM cognito_clients WHERE client_id = ?1",
            Client::COLS
        ),
        params![client_id],
        Client::from_row,
    )
    .optional()
}

pub fn get_client(c: &Connection, pool_id: Option<&str>, client_id: &str) -> CResult<Client> {
    match find_client(c, client_id)? {
        Some(cl) if pool_id.is_none_or(|p| p == cl.pool_id) => Ok(cl),
        _ => Err(err(
            "ResourceNotFoundException",
            format!("User pool client {client_id} does not exist."),
        )),
    }
}

pub fn list_clients(c: &Connection, pool_id: &str) -> rusqlite::Result<Vec<Client>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {} FROM cognito_clients WHERE pool_id = ?1 ORDER BY created, client_id",
        Client::COLS
    ))?;
    let rows = stmt.query_map(params![pool_id], Client::from_row)?;
    rows.collect()
}

pub fn create_client(
    c: &Connection,
    pool_id: &str,
    client_id: Option<String>,
    name: &str,
    secret: Option<String>,
    data: Value,
) -> CResult<Client> {
    get_pool(c, pool_id)?;
    if name.is_empty() {
        return Err(err("InvalidParameterException", "ClientName is required."));
    }
    let id = client_id.unwrap_or_else(new_client_id);
    if find_client(c, &id)?.is_some() {
        return Err(err(
            "InvalidParameterException",
            format!("Client {id} already exists."),
        ));
    }
    let now = now_f();
    c.execute(
        "INSERT INTO cognito_clients (client_id, pool_id, name, secret, data, created, modified)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![id, pool_id, name, secret, data.to_string(), now],
    )?;
    get_client(c, None, &id)
}

pub fn save_client(c: &Connection, client: &Client) -> CResult<()> {
    c.execute(
        "UPDATE cognito_clients SET name = ?2, secret = ?3, data = ?4, modified = ?5 WHERE client_id = ?1",
        params![client.client_id, client.name, client.secret, client.data.to_string(), now_f()],
    )?;
    Ok(())
}

pub fn delete_client(c: &Connection, pool_id: &str, client_id: &str) -> CResult<()> {
    get_client(c, Some(pool_id), client_id)?;
    c.execute(
        "DELETE FROM cognito_clients WHERE client_id = ?1",
        params![client_id],
    )?;
    c.execute(
        "UPDATE cognito_refresh_tokens SET revoked = 1 WHERE client_id = ?1",
        params![client_id],
    )?;
    Ok(())
}

// ─── Users ──────────────────────────────────────────────────────────────────

fn query_users(
    c: &Connection,
    sql_where: &str,
    args: &[&dyn rusqlite::ToSql],
) -> rusqlite::Result<Vec<User>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {} FROM cognito_users WHERE {sql_where} ORDER BY created, username",
        User::COLS
    ))?;
    let rows = stmt.query_map(args, User::from_row)?;
    rows.collect()
}

pub fn list_users(c: &Connection, pool_id: &str) -> rusqlite::Result<Vec<User>> {
    query_users(c, "pool_id = ?1", &[&pool_id])
}

pub fn user_by_username(
    c: &Connection,
    pool_id: &str,
    username: &str,
) -> rusqlite::Result<Option<User>> {
    Ok(
        query_users(c, "pool_id = ?1 AND username = ?2", &[&pool_id, &username])?
            .into_iter()
            .next(),
    )
}

/// Resolve a login name: username, username attribute (email/phone), verified
/// alias, or `sub`.
pub fn find_user(c: &Connection, pool: &Pool, login: &str) -> rusqlite::Result<Option<User>> {
    let pid = pool.id.as_str();
    let by_name = if pool.case_sensitive() {
        query_users(c, "pool_id = ?1 AND username = ?2", &[&pid, &login])?
    } else {
        query_users(
            c,
            "pool_id = ?1 AND username = ?2 COLLATE NOCASE",
            &[&pid, &login],
        )?
    };
    if let Some(u) = by_name.into_iter().next() {
        return Ok(Some(u));
    }
    let uattrs = pool.username_attributes();
    let aliases = pool.alias_attributes();
    let email_match = "pool_id = ?1 AND lower(json_extract(attributes, '$.email')) = lower(?2)";
    if uattrs.iter().any(|a| a == "email") {
        if let Some(u) = query_users(c, email_match, &[&pid, &login])?
            .into_iter()
            .next()
        {
            return Ok(Some(u));
        }
    }
    if uattrs.iter().any(|a| a == "phone_number") {
        if let Some(u) = query_users(
            c,
            "pool_id = ?1 AND json_extract(attributes, '$.phone_number') = ?2",
            &[&pid, &login],
        )?
        .into_iter()
        .next()
        {
            return Ok(Some(u));
        }
    }
    if aliases.iter().any(|a| a == "email") {
        let w = format!("{email_match} AND json_extract(attributes, '$.email_verified') = 'true'");
        if let Some(u) = query_users(c, &w, &[&pid, &login])?.into_iter().next() {
            return Ok(Some(u));
        }
    }
    if aliases.iter().any(|a| a == "phone_number") {
        if let Some(u) = query_users(
            c,
            "pool_id = ?1 AND json_extract(attributes, '$.phone_number') = ?2 \
             AND json_extract(attributes, '$.phone_number_verified') = 'true'",
            &[&pid, &login],
        )?
        .into_iter()
        .next()
        {
            return Ok(Some(u));
        }
    }
    if aliases.iter().any(|a| a == "preferred_username") {
        if let Some(u) = query_users(
            c,
            "pool_id = ?1 AND lower(json_extract(attributes, '$.preferred_username')) = lower(?2)",
            &[&pid, &login],
        )?
        .into_iter()
        .next()
        {
            return Ok(Some(u));
        }
    }
    Ok(
        query_users(c, "pool_id = ?1 AND sub = ?2", &[&pid, &login])?
            .into_iter()
            .next(),
    )
}

pub fn get_user(c: &Connection, pool: &Pool, login: &str) -> CResult<User> {
    find_user(c, pool, login)?.ok_or_else(|| err("UserNotFoundException", "User does not exist."))
}

pub fn save_user(c: &Connection, user: &mut User) -> CResult<()> {
    user.modified = now_f();
    c.execute(
        "UPDATE cognito_users SET attributes = ?3, password_hash = ?4, password_salt = ?5,
            status = ?6, enabled = ?7, modified = ?8, tokens_valid_after = ?9
         WHERE pool_id = ?1 AND username = ?2",
        params![
            user.pool_id,
            user.username,
            Value::Object(user.attributes.clone()).to_string(),
            user.password_hash,
            user.password_salt,
            user.status,
            user.enabled as i64,
            user.modified,
            user.tokens_valid_after
        ],
    )?;
    Ok(())
}

pub fn delete_user(c: &Connection, user: &User) -> CResult<()> {
    for table in [
        "cognito_users",
        "cognito_auth_codes",
        "cognito_sessions",
        "cognito_origins",
        "cognito_refresh_tokens",
        "cognito_challenges",
        "cognito_codes",
    ] {
        c.execute(
            &format!("DELETE FROM {table} WHERE pool_id = ?1 AND username = ?2"),
            params![user.pool_id, user.username],
        )?;
    }
    Ok(())
}

pub struct NewUser {
    pub login: String,
    pub attributes: Vec<(String, String)>,
    pub password: Option<String>,
    pub status: String,
    pub enabled: bool,
    pub sub: Option<String>,
}

fn email_taken(
    c: &Connection,
    pool_id: &str,
    email: &str,
    except: Option<&str>,
) -> rusqlite::Result<bool> {
    let users = query_users(
        c,
        "pool_id = ?1 AND lower(json_extract(attributes, '$.email')) = lower(?2)",
        &[&pool_id, &email],
    )?;
    Ok(users.iter().any(|u| Some(u.username.as_str()) != except))
}

pub fn create_user(c: &Connection, pool: &Pool, req: NewUser) -> CResult<User> {
    let mut attrs = Map::new();
    for (k, v) in &req.attributes {
        if k != "sub" {
            attrs.insert(k.clone(), json!(v));
        }
    }
    let sub = req
        .sub
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let uattrs = pool.username_attributes();
    let username = if !uattrs.is_empty() {
        if req.login.contains('@') && uattrs.iter().any(|a| a == "email") {
            attrs.insert("email".into(), json!(req.login));
        } else if req.login.starts_with('+') && uattrs.iter().any(|a| a == "phone_number") {
            attrs.insert("phone_number".into(), json!(req.login));
        } else {
            let what = if uattrs.iter().any(|a| a == "email") {
                "an email"
            } else {
                "a phone number"
            };
            return Err(err(
                "InvalidParameterException",
                format!("Username should be {what}."),
            ));
        }
        sub.clone()
    } else {
        if req.login.is_empty() {
            return Err(err("InvalidParameterException", "Username is required."));
        }
        req.login.clone()
    };
    if find_user(c, pool, &req.login)?.is_some()
        || user_by_username(c, &pool.id, &username)?.is_some()
    {
        let msg = if pool.email_is_username() && req.login.contains('@') {
            "An account with the given email already exists."
        } else {
            "User account already exists"
        };
        return Err(err("UsernameExistsException", msg));
    }
    if pool.email_is_username() {
        if let Some(e) = attrs.get("email").and_then(|v| v.as_str()) {
            if email_taken(c, &pool.id, e, None)? {
                return Err(err(
                    "UsernameExistsException",
                    "An account with the given email already exists.",
                ));
            }
        }
    }
    let now = now_f();
    let mut user = User {
        pool_id: pool.id.clone(),
        username,
        sub,
        attributes: attrs,
        password_hash: None,
        password_salt: None,
        status: req.status,
        enabled: req.enabled,
        created: now,
        modified: now,
        tokens_valid_after: 0.0,
    };
    if let Some(pw) = &req.password {
        set_password_fields(&mut user, pw);
    }
    c.execute(
        &format!(
            "INSERT INTO cognito_users ({}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            User::COLS
        ),
        params![
            user.pool_id,
            user.username,
            user.sub,
            Value::Object(user.attributes.clone()).to_string(),
            user.password_hash,
            user.password_salt,
            user.status,
            user.enabled as i64,
            user.created,
            user.modified,
            user.tokens_valid_after
        ],
    )?;
    Ok(user)
}

/// Apply attribute updates (`email` changes reset `email_verified` unless given).
pub fn update_attributes(
    c: &Connection,
    pool: &Pool,
    user: &mut User,
    attrs: &[(String, String)],
) -> CResult<()> {
    let given: Vec<&str> = attrs.iter().map(|(k, _)| k.as_str()).collect();
    for (k, v) in attrs {
        if k == "sub" {
            return Err(err(
                "InvalidParameterException",
                "Cannot modify the non-mutable attribute sub",
            ));
        }
        if k == "email" {
            if pool.email_is_username() && email_taken(c, &pool.id, v, Some(&user.username))? {
                return Err(err(
                    "AliasExistsException",
                    "An account with the given email already exists.",
                ));
            }
            if user.attr("email") != Some(v.as_str()) && !given.contains(&"email_verified") {
                user.attributes
                    .insert("email_verified".into(), json!("false"));
            }
        }
        if k == "phone_number"
            && user.attr("phone_number") != Some(v.as_str())
            && !given.contains(&"phone_number_verified")
        {
            user.attributes
                .insert("phone_number_verified".into(), json!("false"));
        }
        user.attributes.insert(k.clone(), json!(v));
    }
    save_user(c, user)
}

/// Set a password after validating it against the pool policy.
pub fn set_password(
    c: &Connection,
    pool: &Pool,
    user: &mut User,
    pw: &str,
    permanent: bool,
) -> CResult<()> {
    check_policy(pool, pw)?;
    set_password_fields(user, pw);
    user.status = if permanent {
        "CONFIRMED"
    } else {
        "FORCE_CHANGE_PASSWORD"
    }
    .into();
    save_user(c, user)
}

// ─── ListUsers filter ───────────────────────────────────────────────────────

pub struct Filter {
    attr: String,
    prefix: bool,
    value: String,
}

const FILTER_ATTRS: &[&str] = &[
    "username",
    "email",
    "phone_number",
    "name",
    "given_name",
    "family_name",
    "preferred_username",
    "cognito:user_status",
    "status",
    "sub",
];

pub fn parse_filter(f: &str) -> CResult<Option<Filter>> {
    let f = f.trim();
    if f.is_empty() {
        return Ok(None);
    }
    let invalid = || {
        err(
            "InvalidParameterException",
            format!("Error while parsing filter: {f}"),
        )
    };
    let (lhs, rhs, prefix) = if let Some(i) = f.find("^=") {
        (&f[..i], &f[i + 2..], true)
    } else if let Some(i) = f.find('=') {
        (&f[..i], &f[i + 1..], false)
    } else {
        return Err(invalid());
    };
    let attr = lhs.trim().trim_matches('"').to_string();
    let rhs = rhs.trim();
    if rhs.len() < 2 || !rhs.starts_with('"') || !rhs.ends_with('"') {
        return Err(invalid());
    }
    let value = rhs[1..rhs.len() - 1].replace("\\\"", "\"");
    if !FILTER_ATTRS.contains(&attr.as_str()) {
        return Err(err(
            "InvalidParameterException",
            format!("Invalid search attribute: {attr}"),
        ));
    }
    Ok(Some(Filter {
        attr,
        prefix,
        value,
    }))
}

impl Filter {
    pub fn matches(&self, u: &User) -> bool {
        let actual: Option<String> = match self.attr.as_str() {
            "username" => Some(u.username.clone()),
            "status" => Some(if u.enabled { "Enabled" } else { "Disabled" }.into()),
            "cognito:user_status" => Some(u.status.clone()),
            a => u.attr(a).map(|s| s.to_string()),
        };
        let Some(actual) = actual else { return false };
        let case_sensitive = matches!(self.attr.as_str(), "sub" | "status");
        let (a, v) = if case_sensitive {
            (actual, self.value.clone())
        } else {
            (actual.to_lowercase(), self.value.to_lowercase())
        };
        if self.prefix {
            a.starts_with(&v)
        } else {
            a == v
        }
    }
}

// ─── Authentication ─────────────────────────────────────────────────────────

pub enum AuthOutcome {
    Ok(User),
    NewPasswordRequired(User),
}

pub fn authenticate(
    c: &Connection,
    pool: &Pool,
    client: Option<&Client>,
    login: &str,
    password: &str,
) -> CResult<AuthOutcome> {
    let Some(user) = find_user(c, pool, login)? else {
        if client.is_some_and(|cl| cl.prevent_user_existence_errors()) {
            return Err(not_authorized(BAD_CREDENTIALS));
        }
        return Err(err("UserNotFoundException", "User does not exist."));
    };
    if !user.enabled {
        return Err(not_authorized("User is disabled."));
    }
    if !verify_password(&user, password) {
        return Err(not_authorized(BAD_CREDENTIALS));
    }
    match user.status.as_str() {
        "UNCONFIRMED" => Err(err("UserNotConfirmedException", "User is not confirmed.")),
        "RESET_REQUIRED" => Err(err(
            "PasswordResetRequiredException",
            "Password reset required for the user",
        )),
        "FORCE_CHANGE_PASSWORD" => Ok(AuthOutcome::NewPasswordRequired(user)),
        _ => Ok(AuthOutcome::Ok(user)),
    }
}

pub fn create_challenge(
    c: &Connection,
    client: &Client,
    user: &User,
    challenge: &str,
) -> CResult<String> {
    let session = random_token(192);
    let expires = now_f() + (client.auth_session_minutes() * 60) as f64;
    c.execute(
        "INSERT INTO cognito_challenges (session, pool_id, client_id, username, challenge, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![session, client.pool_id, client.client_id, user.username, challenge, expires],
    )?;
    Ok(session)
}

/// Consume a challenge session, returning the user it was issued for.
pub fn take_challenge(
    c: &Connection,
    client: &Client,
    session: &str,
    challenge: &str,
) -> CResult<User> {
    let expired = || not_authorized("Invalid session for the user, session is expired.");
    let row: Option<(String, String, String, f64)> = c
        .query_row(
            "SELECT client_id, username, challenge, expires_at FROM cognito_challenges WHERE session = ?1",
            params![session],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((cid, username, ch, exp)) = row else {
        return Err(expired());
    };
    if cid != client.client_id || exp < now_f() {
        return Err(expired());
    }
    if ch != challenge {
        return Err(err(
            "InvalidParameterException",
            "Invalid challenge name for the session.",
        ));
    }
    c.execute(
        "DELETE FROM cognito_challenges WHERE session = ?1",
        params![session],
    )?;
    user_by_username(c, &client.pool_id, &username)?.ok_or_else(expired)
}

// ─── Tokens ─────────────────────────────────────────────────────────────────

pub struct TokenSet {
    pub id_token: Option<String>,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: i64,
}

pub struct IssueOpts {
    pub scopes: Vec<String>,
    pub nonce: Option<String>,
    pub auth_time: i64,
    pub id_token: bool,
    pub refresh: bool,
    pub origin_jti: Option<String>,
}

pub const ADMIN_SCOPE: &str = "aws.cognito.signin.user.admin";

fn id_claim_value(name: &str, v: &str) -> Value {
    if name.ends_with("_verified") {
        json!(v.eq_ignore_ascii_case("true"))
    } else if name == "updated_at" {
        v.parse::<i64>()
            .map(|n| json!(n))
            .unwrap_or_else(|_| json!(v))
    } else {
        json!(v)
    }
}

pub fn issue_tokens(
    c: &Connection,
    ctx: &Ctx,
    pool: &Pool,
    client: &Client,
    user: &User,
    opts: IssueOpts,
) -> CResult<TokenSet> {
    let now = now_i();
    let origin = match opts.origin_jti {
        Some(o) => o,
        None => {
            let o = Uuid::new_v4().to_string();
            c.execute(
                "INSERT INTO cognito_origins (origin_jti, pool_id, client_id, username, created)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![o, pool.id, client.client_id, user.username, now_f()],
            )?;
            o
        }
    };
    let event_id = Uuid::new_v4().to_string();
    let iss = ctx.issuer(&pool.id);
    let access_ttl = client.ttl(TokenKind::Access, &ctx.cfg);
    let access = json!({
        "sub": user.sub,
        "iss": iss,
        "client_id": client.client_id,
        "origin_jti": origin,
        "event_id": event_id,
        "token_use": "access",
        "scope": opts.scopes.join(" "),
        "auth_time": opts.auth_time,
        "exp": now + access_ttl,
        "iat": now,
        "jti": Uuid::new_v4().to_string(),
        "username": user.username,
        "version": 2,
    });
    let access_token = ctx.keys.sign(&access);

    let id_token = if opts.id_token {
        let mut claims = Map::new();
        for (k, v) in &user.attributes {
            if let Some(s) = v.as_str() {
                claims.insert(k.clone(), id_claim_value(k, s));
            }
        }
        if user.attr("email").is_some() && !claims.contains_key("email_verified") {
            claims.insert("email_verified".into(), json!(false));
        }
        let id_ttl = client.ttl(TokenKind::Id, &ctx.cfg);
        for (k, v) in [
            ("sub", json!(user.sub)),
            ("iss", json!(iss)),
            ("cognito:username", json!(user.username)),
            ("origin_jti", json!(origin)),
            ("aud", json!(client.client_id)),
            ("event_id", json!(event_id)),
            ("token_use", json!("id")),
            ("auth_time", json!(opts.auth_time)),
            ("exp", json!(now + id_ttl)),
            ("iat", json!(now)),
            ("jti", json!(Uuid::new_v4().to_string())),
        ] {
            claims.insert(k.into(), v);
        }
        if let Some(n) = &opts.nonce {
            claims.insert("nonce".into(), json!(n));
        }
        Some(ctx.keys.sign(&Value::Object(claims)))
    } else {
        None
    };

    let refresh_token = if opts.refresh {
        let token = random_token(96);
        let expires = now_f() + client.ttl(TokenKind::Refresh, &ctx.cfg) as f64;
        c.execute(
            "INSERT INTO cognito_refresh_tokens
                (token, pool_id, client_id, username, origin_jti, scope, auth_time, created, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                token,
                pool.id,
                client.client_id,
                user.username,
                origin,
                opts.scopes.join(" "),
                opts.auth_time,
                now_f(),
                expires
            ],
        )?;
        Some(token)
    } else {
        None
    };
    Ok(TokenSet {
        id_token,
        access_token,
        refresh_token,
        expires_in: access_ttl,
    })
}

/// Tokens for a successful API (InitiateAuth-style) authentication.
pub fn issue_api_tokens(
    c: &Connection,
    ctx: &Ctx,
    pool: &Pool,
    client: &Client,
    user: &User,
) -> CResult<TokenSet> {
    issue_tokens(
        c,
        ctx,
        pool,
        client,
        user,
        IssueOpts {
            scopes: vec![ADMIN_SCOPE.into()],
            nonce: None,
            auth_time: now_i(),
            id_token: true,
            refresh: true,
            origin_jti: None,
        },
    )
}

pub fn client_credentials_token(ctx: &Ctx, client: &Client, scopes: &[String]) -> (String, i64) {
    let now = now_i();
    let ttl = client.ttl(TokenKind::Access, &ctx.cfg);
    let claims = json!({
        "sub": client.client_id,
        "token_use": "access",
        "scope": scopes.join(" "),
        "auth_time": now,
        "iss": ctx.issuer(&client.pool_id),
        "exp": now + ttl,
        "iat": now,
        "version": 2,
        "jti": Uuid::new_v4().to_string(),
        "client_id": client.client_id,
    });
    (ctx.keys.sign(&claims), ttl)
}

fn origin_revoked(c: &Connection, origin: &str) -> rusqlite::Result<bool> {
    Ok(c.query_row(
        "SELECT revoked FROM cognito_origins WHERE origin_jti = ?1",
        params![origin],
        |r| r.get::<_, i64>(0),
    )
    .optional()?
    .is_none_or(|r| r != 0))
}

/// Exchange a refresh token for new ID/access tokens (no new refresh token).
pub fn refresh(c: &Connection, ctx: &Ctx, client: &Client, token: &str) -> CResult<TokenSet> {
    let invalid = || not_authorized("Invalid Refresh Token");
    let row: Option<(String, String, String, String, i64, f64, i64)> = c
        .query_row(
            "SELECT client_id, username, origin_jti, scope, auth_time, expires_at, revoked
             FROM cognito_refresh_tokens WHERE token = ?1",
            params![token],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .optional()?;
    let Some((cid, username, origin, scope, auth_time, expires, revoked)) = row else {
        return Err(invalid());
    };
    if cid != client.client_id {
        return Err(invalid());
    }
    if revoked != 0 || origin_revoked(c, &origin)? {
        return Err(not_authorized("Refresh Token has been revoked"));
    }
    if expires < now_f() {
        return Err(not_authorized("Refresh Token has expired"));
    }
    let pool = get_pool(c, &client.pool_id)?;
    let user = user_by_username(c, &pool.id, &username)?.ok_or_else(invalid)?;
    if !user.enabled {
        return Err(not_authorized("User is disabled."));
    }
    let scopes: Vec<String> = scope.split_whitespace().map(String::from).collect();
    let id_token = scopes.iter().any(|s| s == "openid" || s == ADMIN_SCOPE);
    issue_tokens(
        c,
        ctx,
        &pool,
        client,
        &user,
        IssueOpts {
            scopes,
            nonce: None,
            auth_time,
            id_token,
            refresh: false,
            origin_jti: Some(origin),
        },
    )
}

/// Revoke a refresh token (and the access tokens derived from it).
/// Returns `Ok(false)` when the token is unknown.
pub fn revoke_refresh(c: &Connection, client: &Client, token: &str) -> CResult<bool> {
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT client_id, origin_jti FROM cognito_refresh_tokens WHERE token = ?1",
            params![token],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((cid, origin)) = row else {
        return Ok(false);
    };
    if cid != client.client_id {
        return Err(err(
            "UnauthorizedException",
            "Token does not belong to this client.",
        ));
    }
    c.execute(
        "UPDATE cognito_refresh_tokens SET revoked = 1 WHERE token = ?1",
        params![token],
    )?;
    c.execute(
        "UPDATE cognito_origins SET revoked = 1 WHERE origin_jti = ?1",
        params![origin],
    )?;
    Ok(true)
}

/// Validate an access token issued for a user: signature, expiry, issuer,
/// revocation, and user state. Errors carry Cognito's GetUser messages.
pub fn validate_access(c: &Connection, ctx: &Ctx, token: &str) -> CResult<(Pool, User, Value)> {
    let invalid = || not_authorized("Invalid Access Token");
    let claims = ctx.keys.verify(token).ok_or_else(invalid)?;
    if claims.get("token_use").and_then(|v| v.as_str()) != Some("access") {
        return Err(invalid());
    }
    if claims.get("exp").and_then(|v| v.as_i64()).unwrap_or(0) < now_i() {
        return Err(not_authorized("Access Token has expired"));
    }
    let iss = claims
        .get("iss")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let pool_id = iss.rsplit('/').next().unwrap_or_default().to_string();
    if iss != ctx.issuer(&pool_id) {
        return Err(invalid());
    }
    let pool = find_pool(c, &pool_id)?.ok_or_else(invalid)?;
    let username = claims
        .get("username")
        .and_then(|v| v.as_str())
        .ok_or_else(invalid)?;
    let user = user_by_username(c, &pool.id, username)?
        .ok_or_else(|| err("UserNotFoundException", "User does not exist."))?;
    let origin = claims
        .get("origin_jti")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if origin_revoked(c, origin)? {
        return Err(not_authorized("Access Token has been revoked"));
    }
    if !user.enabled {
        return Err(not_authorized("User is disabled."));
    }
    Ok((pool, user, claims))
}

pub fn has_scope(claims: &Value, scope: &str) -> bool {
    claims
        .get("scope")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s.split_whitespace().any(|x| x == scope))
}

/// Revoke every refresh token, access token, and hosted-UI session of a user.
pub fn global_sign_out(c: &Connection, user: &mut User) -> CResult<()> {
    let args = params![user.pool_id, user.username];
    c.execute(
        "UPDATE cognito_origins SET revoked = 1 WHERE pool_id = ?1 AND username = ?2",
        args,
    )?;
    c.execute(
        "UPDATE cognito_refresh_tokens SET revoked = 1 WHERE pool_id = ?1 AND username = ?2",
        args,
    )?;
    c.execute(
        "DELETE FROM cognito_sessions WHERE pool_id = ?1 AND username = ?2",
        args,
    )?;
    user.tokens_valid_after = now_f();
    save_user(c, user)
}

// ─── Hosted UI sessions and authorization codes ─────────────────────────────

pub const SESSION_TTL_SECS: f64 = 3600.0;

pub fn create_session(c: &Connection, user: &User, auth_time: i64) -> CResult<String> {
    let id = random_token(32);
    let now = now_f();
    c.execute(
        "INSERT INTO cognito_sessions (id, pool_id, username, auth_time, created, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id,
            user.pool_id,
            user.username,
            auth_time,
            now,
            now + SESSION_TTL_SECS
        ],
    )?;
    Ok(id)
}

/// Resolve a hosted-UI session to a signed-in, enabled, confirmed user.
pub fn session_user(c: &Connection, pool_id: &str, session: &str) -> CResult<Option<(User, i64)>> {
    let row: Option<(String, i64, f64)> = c
        .query_row(
            "SELECT username, auth_time, expires_at FROM cognito_sessions WHERE id = ?1 AND pool_id = ?2",
            params![session, pool_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((username, auth_time, expires)) = row else {
        return Ok(None);
    };
    if expires < now_f() {
        return Ok(None);
    }
    let Some(user) = user_by_username(c, pool_id, &username)? else {
        return Ok(None);
    };
    if !user.enabled || user.status != "CONFIRMED" {
        return Ok(None);
    }
    Ok(Some((user, auth_time)))
}

pub fn delete_session(c: &Connection, session: &str) -> CResult<()> {
    c.execute(
        "DELETE FROM cognito_sessions WHERE id = ?1",
        params![session],
    )?;
    Ok(())
}

pub struct AuthCode {
    pub client_id: String,
    pub username: String,
    pub redirect_uri: String,
    pub scope: String,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub nonce: Option<String>,
    pub auth_time: i64,
}

pub const CODE_TTL_SECS: f64 = 300.0;

pub fn create_code(c: &Connection, pool_id: &str, code: &AuthCode) -> CResult<String> {
    let id = Uuid::new_v4().to_string();
    c.execute(
        "INSERT INTO cognito_auth_codes (code, pool_id, client_id, username, redirect_uri, scope,
            code_challenge, code_challenge_method, nonce, auth_time, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            id,
            pool_id,
            code.client_id,
            code.username,
            code.redirect_uri,
            code.scope,
            code.code_challenge,
            code.code_challenge_method,
            code.nonce,
            code.auth_time,
            now_f() + CODE_TTL_SECS
        ],
    )?;
    Ok(id)
}

/// Consume an authorization code (single use). `None` if unknown, used or expired.
pub fn take_code(c: &Connection, code: &str) -> CResult<Option<AuthCode>> {
    let row = c
        .query_row(
            "SELECT client_id, username, redirect_uri, scope, code_challenge, code_challenge_method,
                    nonce, auth_time, expires_at, used
             FROM cognito_auth_codes WHERE code = ?1",
            params![code],
            |r| {
                Ok((
                    AuthCode {
                        client_id: r.get(0)?,
                        username: r.get(1)?,
                        redirect_uri: r.get(2)?,
                        scope: r.get(3)?,
                        code_challenge: r.get(4)?,
                        code_challenge_method: r.get(5)?,
                        nonce: r.get(6)?,
                        auth_time: r.get(7)?,
                    },
                    r.get::<_, f64>(8)?,
                    r.get::<_, i64>(9)?,
                ))
            },
        )
        .optional()?;
    let Some((ac, expires, used)) = row else {
        return Ok(None);
    };
    c.execute(
        "UPDATE cognito_auth_codes SET used = 1 WHERE code = ?1",
        params![code],
    )?;
    if used != 0 || expires < now_f() {
        return Ok(None);
    }
    Ok(Some(ac))
}

// ─── Verification codes ─────────────────────────────────────────────────────

pub fn set_code(c: &Connection, user: &User, kind: &str) -> CResult<String> {
    let code = six_digit_code();
    c.execute(
        "INSERT INTO cognito_codes (pool_id, username, kind, code, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(pool_id, username, kind) DO UPDATE SET code = excluded.code, expires_at = excluded.expires_at",
        params![user.pool_id, user.username, kind, code, now_f() + 86400.0],
    )?;
    Ok(code)
}

pub fn check_code(c: &Connection, user: &User, kind: &str, code: &str) -> CResult<()> {
    let row: Option<(String, f64)> = c
        .query_row(
            "SELECT code, expires_at FROM cognito_codes WHERE pool_id = ?1 AND username = ?2 AND kind = ?3",
            params![user.pool_id, user.username, kind],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        Some((stored, exp)) if stored == code.trim() => {
            if exp < now_f() {
                return Err(err(
                    "ExpiredCodeException",
                    "Invalid code provided, please request a code again.",
                ));
            }
            c.execute(
                "DELETE FROM cognito_codes WHERE pool_id = ?1 AND username = ?2 AND kind = ?3",
                params![user.pool_id, user.username, kind],
            )?;
            Ok(())
        }
        _ => Err(err(
            "CodeMismatchException",
            "Invalid verification code provided, please try again.",
        )),
    }
}

// ─── Email ──────────────────────────────────────────────────────────────────

pub const DEFAULT_FROM: &str = "no-reply@verificationemail.com";

fn sender(pool: &Pool) -> String {
    pool.data
        .pointer("/EmailConfiguration/From")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| DEFAULT_FROM.into())
}

pub fn build_email(from: &str, to: &str, subject: &str, body: &str) -> Vec<u8> {
    let ctype = if body.contains('<') {
        "text/html"
    } else {
        "text/plain"
    };
    format!(
        "From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\nDate: {date}\r\nMessage-ID: <{id}@cognito.cloudtwin.local>\r\n\
MIME-Version: 1.0\r\nContent-Type: {ctype}; charset=UTF-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n{body}\r\n",
        date = chrono::Utc::now().to_rfc2822(),
        id = Uuid::new_v4(),
    )
    .into_bytes()
}

/// Invitation email for AdminCreateUser (`{username}` / `{####}` placeholders).
pub fn invite_email(pool: &Pool, user: &User, login: &str, temp_password: &str) -> Option<Vec<u8>> {
    let to = user.attr("email")?;
    let tmpl = pool
        .data
        .pointer("/AdminCreateUserConfig/InviteMessageTemplate");
    let subject = tmpl
        .and_then(|t| t.get("EmailSubject"))
        .and_then(|v| v.as_str())
        .unwrap_or("Your temporary password");
    let message = tmpl
        .and_then(|t| t.get("EmailMessage"))
        .and_then(|v| v.as_str())
        .unwrap_or("Your username is {username} and temporary password is {####}.");
    let body = message
        .replace("{username}", login)
        .replace("{####}", temp_password);
    Some(build_email(&sender(pool), to, subject, &body))
}

/// Verification / password-reset code email.
pub fn code_email(pool: &Pool, user: &User, code: &str) -> Option<Vec<u8>> {
    let to = user.attr("email")?;
    let tmpl = pool.data.get("VerificationMessageTemplate");
    let subject = tmpl
        .and_then(|t| t.get("EmailSubject"))
        .or_else(|| pool.data.get("EmailVerificationSubject"))
        .and_then(|v| v.as_str())
        .unwrap_or("Your verification code");
    let message = tmpl
        .and_then(|t| t.get("EmailMessage"))
        .or_else(|| pool.data.get("EmailVerificationMessage"))
        .and_then(|v| v.as_str())
        .unwrap_or("Your verification code is {####}.");
    let body = message
        .replace("{####}", code)
        .replace("{username}", &user.username);
    Some(build_email(&sender(pool), to, subject, &body))
}

/// `a***@e***` style masking used in CodeDeliveryDetails.
pub fn mask_email(email: &str) -> String {
    match email.split_once('@') {
        Some((l, d)) => format!(
            "{}***@{}***",
            l.chars().next().unwrap_or('*'),
            d.chars().next().unwrap_or('*')
        ),
        None => "***".into(),
    }
}

pub fn code_delivery(user: &User) -> Value {
    match user.attr("email") {
        Some(e) => {
            json!({"Destination": mask_email(e), "DeliveryMedium": "EMAIL", "AttributeName": "email"})
        }
        None => json!({}),
    }
}

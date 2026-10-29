//! Cognito user pools: admin JSON API (`AWSCognitoIdentityProviderService.*`)
//! plus OIDC discovery, JWKS, hosted UI and `/oauth2/*` endpoints.

mod config;
mod handlers_api;
mod handlers_oidc;
mod models;
pub mod schema;
mod service;
mod tokens;

#[cfg(test)]
mod tests;

pub use config::CognitoConfig;

use std::sync::Arc;

use axum::{response::Response, Router};
use serde_json::{json, Map, Value};

use crate::AppState;
use service::{self as svc, err, CResult, NewUser};

pub async fn handle_json(state: &Arc<AppState>, target: &str, body: Value) -> Response {
    handlers_api::handle(state, target, body).await
}

/// OIDC discovery, JWKS, and hosted-UI routes.
pub fn router() -> Router<Arc<AppState>> {
    handlers_oidc::router()
}

/// Provision user pools, clients, and users (idempotent):
///
/// ```yaml
/// cognito:
///   user_pools:
///     - id: us-east-1_localpool        # optional; otherwise matched by name
///       name: orkystra
///       username_attributes: [email]   # optional
///       domain: orkystra-local          # optional
///       clients:
///         - client_id: fixedclientid    # optional; otherwise matched by name
///           name: web
///           secret: false               # false | true | "explicit-secret"
///           callback_urls: [http://localhost:3000/callback]
///           logout_urls: [http://localhost:3000/]
///           scopes: [openid, email, profile]
///           flows: [code]
///       users:
///         - email: dev@example.com
///           password: Passw0rd!
///           attributes: { name: Dev User }
/// ```
pub async fn seed(state: &Arc<AppState>, doc: &Value) -> anyhow::Result<()> {
    for pool_doc in doc
        .get("user_pools")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        let d = pool_doc.clone();
        svc::run(state, move |c| seed_pool(c, &d))
            .await
            .map_err(|e| anyhow::anyhow!("{}: {}", e.code, e.msg))?;
    }
    Ok(())
}

fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str()).filter(|x| !x.is_empty())
}

fn strings(v: &Value, k: &str) -> Option<Vec<String>> {
    v.get(k).and_then(|x| x.as_array()).map(|a| {
        a.iter()
            .filter_map(|s| s.as_str().map(String::from))
            .collect()
    })
}

fn seed_pool(c: &rusqlite::Connection, d: &Value) -> CResult<()> {
    let id = s(d, "id").map(String::from);
    let name = s(d, "name")
        .or(id.as_deref())
        .ok_or_else(|| err("InvalidParameterException", "user pool needs a name or id"))?
        .to_string();
    let existing = match &id {
        Some(id) => svc::find_pool(c, id)?,
        None => svc::find_pool_by_name(c, &name)?,
    };
    let mut pool = match existing {
        Some(p) => p,
        None => {
            let mut data = Map::new();
            for (yaml, api) in [
                ("username_attributes", "UsernameAttributes"),
                ("alias_attributes", "AliasAttributes"),
                ("auto_verified_attributes", "AutoVerifiedAttributes"),
            ] {
                if let Some(v) = strings(d, yaml) {
                    data.insert(api.into(), json!(v));
                }
            }
            data.entry("AutoVerifiedAttributes")
                .or_insert_with(|| json!(["email"]));
            data.insert(
                "UsernameConfiguration".into(),
                json!({"CaseSensitive": false}),
            );
            if let Some(p) = d.get("password_policy").filter(|v| v.is_object()) {
                data.insert("Policies".into(), json!({"PasswordPolicy": p}));
            }
            svc::create_pool(c, id, &name, Value::Object(data))?
        }
    };
    if let Some(domain) = s(d, "domain") {
        if pool.domain.is_none() && svc::find_pool_by_domain(c, domain)?.is_none() {
            pool.domain = Some(domain.to_string());
            svc::save_pool(c, &pool)?;
        }
    }
    for cd in d
        .get("clients")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        seed_client(c, &pool, cd)?;
    }
    for ud in d
        .get("users")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        seed_user(c, &pool, ud)?;
    }
    Ok(())
}

fn seed_client(c: &rusqlite::Connection, pool: &models::Pool, cd: &Value) -> CResult<()> {
    let name = s(cd, "name").unwrap_or("client").to_string();
    let mut data = Map::new();
    data.insert(
        "CallbackURLs".into(),
        json!(strings(cd, "callback_urls").unwrap_or_default()),
    );
    data.insert(
        "LogoutURLs".into(),
        json!(strings(cd, "logout_urls").unwrap_or_default()),
    );
    let scopes = strings(cd, "scopes")
        .unwrap_or_else(|| vec!["openid".into(), "email".into(), "profile".into()]);
    data.insert("AllowedOAuthScopes".into(), json!(scopes));
    data.insert(
        "AllowedOAuthFlows".into(),
        json!(strings(cd, "flows").unwrap_or_else(|| vec!["code".into()])),
    );
    data.insert("AllowedOAuthFlowsUserPoolClient".into(), json!(true));
    data.insert("SupportedIdentityProviders".into(), json!(["COGNITO"]));
    data.insert(
        "ExplicitAuthFlows".into(),
        json!(strings(cd, "explicit_auth_flows").unwrap_or_else(|| vec![
            "ALLOW_USER_PASSWORD_AUTH".into(),
            "ALLOW_ADMIN_USER_PASSWORD_AUTH".into(),
            "ALLOW_REFRESH_TOKEN_AUTH".into(),
            "ALLOW_USER_SRP_AUTH".into(),
        ])),
    );
    if let Some(d) = s(cd, "default_redirect_uri") {
        data.insert("DefaultRedirectURI".into(), json!(d));
    }
    let mut units = Map::new();
    for (k, field, unit) in [
        (
            "access_token_ttl_seconds",
            "AccessTokenValidity",
            "AccessToken",
        ),
        ("id_token_ttl_seconds", "IdTokenValidity", "IdToken"),
        (
            "refresh_token_ttl_seconds",
            "RefreshTokenValidity",
            "RefreshToken",
        ),
    ] {
        if let Some(v) = cd.get(k).and_then(|v| v.as_i64()) {
            data.insert(field.into(), json!(v));
            units.insert(unit.into(), json!("seconds"));
        }
    }
    if !units.is_empty() {
        data.insert("TokenValidityUnits".into(), Value::Object(units));
    }
    let data = Value::Object(data);

    let existing = match s(cd, "client_id") {
        Some(cid) => match svc::find_client(c, cid)? {
            Some(cl) if cl.pool_id != pool.id => {
                return Err(err(
                    "InvalidParameterException",
                    format!("client {cid} belongs to another user pool"),
                ))
            }
            other => other,
        },
        None => svc::list_clients(c, &pool.id)?
            .into_iter()
            .find(|cl| cl.name == name),
    };
    let secret_spec = cd.get("secret").cloned().unwrap_or(Value::Bool(false));
    match existing {
        Some(mut cl) => {
            cl.data = data;
            match &secret_spec {
                Value::String(sec) if !sec.is_empty() => cl.secret = Some(sec.clone()),
                Value::Bool(true) if cl.secret.is_none() => {
                    cl.secret = Some(svc::new_client_secret())
                }
                _ => {}
            }
            svc::save_client(c, &cl)
        }
        None => {
            let secret = match &secret_spec {
                Value::String(sec) if !sec.is_empty() => Some(sec.clone()),
                Value::Bool(true) => Some(svc::new_client_secret()),
                _ => None,
            };
            svc::create_client(
                c,
                &pool.id,
                s(cd, "client_id").map(String::from),
                &name,
                secret,
                data,
            )
            .map(|_| ())
        }
    }
}

fn seed_user(c: &rusqlite::Connection, pool: &models::Pool, ud: &Value) -> CResult<()> {
    let email = s(ud, "email");
    let username = s(ud, "username");
    let login = if pool.username_attributes().is_empty() {
        username.or(email)
    } else {
        email.or(username)
    }
    .ok_or_else(|| {
        err(
            "InvalidParameterException",
            "seed user needs a username or email",
        )
    })?
    .to_string();
    let sub = s(ud, "sub").map(String::from);
    if svc::find_user(c, pool, &login)?.is_some() {
        return Ok(());
    }
    if let Some(sub) = &sub {
        if svc::find_user(c, pool, sub)?.is_some() {
            return Ok(());
        }
    }
    let mut attrs: Vec<(String, String)> = Vec::new();
    if let Some(e) = email {
        attrs.push(("email".into(), e.to_string()));
        let verified = ud
            .get("email_verified")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        attrs.push(("email_verified".into(), verified.to_string()));
    }
    for (k, v) in ud
        .get("attributes")
        .and_then(|v| v.as_object())
        .into_iter()
        .flatten()
    {
        let v = match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        attrs.retain(|(n, _)| n != k);
        attrs.push((k.clone(), v));
    }
    let password = s(ud, "password").map(String::from);
    let status = s(ud, "status").map(String::from).unwrap_or_else(|| {
        if password.is_some() {
            "CONFIRMED"
        } else {
            "FORCE_CHANGE_PASSWORD"
        }
        .into()
    });
    svc::create_user(
        c,
        pool,
        NewUser {
            login,
            attributes: attrs,
            password: Some(password.unwrap_or_else(|| svc::generate_password(pool))),
            status,
            enabled: ud.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
            sub,
        },
    )
    .map(|_| ())
}

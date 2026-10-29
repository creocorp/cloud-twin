//! `AWSCognitoIdentityProviderService.*` JSON 1.1 protocol handlers.

use std::sync::Arc;

use axum::{http::StatusCode, response::Response};
use serde_json::{json, Map, Value};

use super::models::{Client, Pool, User};
use super::service::{self as svc, err, not_authorized, run, AuthOutcome, CResult, Ctx, NewUser};
use crate::proto::{json_error_response, json_ok};
use crate::AppState;

const READ_ONLY: &[&str] = &[
    "DescribeUserPool",
    "ListUserPools",
    "DescribeUserPoolClient",
    "ListUserPoolClients",
    "DescribeUserPoolDomain",
    "AdminGetUser",
    "ListUsers",
    "GetUser",
];

pub async fn handle(state: &Arc<AppState>, target: &str, body: Value) -> Response {
    let op = target
        .strip_prefix("AWSCognitoIdentityProviderService.")
        .unwrap_or(target);
    let body = if body.is_object() { body } else { json!({}) };
    let res = dispatch(state, op, &body).await;
    match res {
        Ok(v) => {
            if !READ_ONLY.contains(&op) {
                let mut payload = Map::new();
                for k in ["UserPoolId", "ClientId", "Username", "PoolName", "ClientName", "Domain", "AuthFlow"] {
                    if let Some(v) = body.get(k) {
                        payload.insert(k.into(), v.clone());
                    }
                }
                for k in ["UserPool", "UserPoolClient"] {
                    if let Some(id) = v.get(k).and_then(|p| p.get("Id").or_else(|| p.get("ClientId"))) {
                        payload.insert(format!("{k}Id"), id.clone());
                    }
                }
                crate::telemetry::emit(
                    &state.db,
                    "aws",
                    "cognito",
                    &to_snake(op),
                    &Value::Object(payload).to_string(),
                )
                .await;
            }
            json_ok(v)
        }
        Err(e) => {
            let status = if e.code == "InternalErrorException" {
                StatusCode::INTERNAL_SERVER_ERROR
            } else {
                StatusCode::BAD_REQUEST
            };
            json_error_response(status, e.code, &e.msg)
        }
    }
}

fn to_snake(op: &str) -> String {
    let mut out = String::new();
    for (i, ch) in op.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

async fn dispatch(state: &Arc<AppState>, op: &str, b: &Value) -> CResult<Value> {
    match op {
        "CreateUserPool" => create_user_pool(state, b).await,
        "DescribeUserPool" => describe_user_pool(state, b).await,
        "ListUserPools" => list_user_pools(state, b).await,
        "UpdateUserPool" => update_user_pool(state, b).await,
        "DeleteUserPool" => {
            let id = req_str(b, "UserPoolId")?;
            run(state, move |c| svc::delete_pool(c, &id)).await?;
            Ok(json!({}))
        }
        "CreateUserPoolClient" => create_client(state, b).await,
        "DescribeUserPoolClient" => {
            let (pid, cid) = (req_str(b, "UserPoolId")?, req_str(b, "ClientId")?);
            let cl = run(state, move |c| svc::get_client(c, Some(&pid), &cid)).await?;
            Ok(json!({"UserPoolClient": cl.describe()}))
        }
        "ListUserPoolClients" => list_clients(state, b).await,
        "UpdateUserPoolClient" => update_client(state, b).await,
        "DeleteUserPoolClient" => {
            let (pid, cid) = (req_str(b, "UserPoolId")?, req_str(b, "ClientId")?);
            run(state, move |c| svc::delete_client(c, &pid, &cid)).await?;
            Ok(json!({}))
        }
        "CreateUserPoolDomain" => create_domain(state, b).await,
        "DescribeUserPoolDomain" => describe_domain(state, b).await,
        "DeleteUserPoolDomain" => delete_domain(state, b).await,
        "AdminCreateUser" => admin_create_user(state, b).await,
        "AdminGetUser" => {
            let user = pool_user(state, b).await?;
            Ok(user.get_shape())
        }
        "ListUsers" => list_users(state, b).await,
        "AdminDisableUser" | "AdminEnableUser" => {
            let enable = op == "AdminEnableUser";
            with_user(state, b, move |c, _pool, user| {
                user.enabled = enable;
                if !enable {
                    c.execute(
                        "DELETE FROM cognito_sessions WHERE pool_id = ?1 AND username = ?2",
                        rusqlite::params![user.pool_id, user.username],
                    )?;
                }
                svc::save_user(c, user)
            })
            .await
        }
        "AdminDeleteUser" => with_user(state, b, |c, _p, u| svc::delete_user(c, u)).await,
        "AdminSetUserPassword" => {
            let pw = req_str(b, "Password")?;
            let permanent = b.get("Permanent").and_then(|v| v.as_bool()).unwrap_or(false);
            with_user(state, b, move |c, pool, user| {
                svc::set_password(c, pool, user, &pw, permanent)
            })
            .await
        }
        "AdminUserGlobalSignOut" => with_user(state, b, |c, _p, u| svc::global_sign_out(c, u)).await,
        "AdminUpdateUserAttributes" => {
            let attrs = attr_list(b, "UserAttributes");
            with_user(state, b, move |c, pool, user| svc::update_attributes(c, pool, user, &attrs)).await
        }
        "AdminDeleteUserAttributes" => {
            let names: Vec<String> = str_list(b, "UserAttributeNames");
            with_user(state, b, move |c, _pool, user| {
                for n in &names {
                    if n == "sub" {
                        return Err(err("InvalidParameterException", "Cannot delete the attribute sub"));
                    }
                    user.attributes.remove(n);
                }
                svc::save_user(c, user)
            })
            .await
        }
        "AdminConfirmSignUp" => {
            with_user(state, b, confirm_user).await
        }
        "AdminResetUserPassword" => admin_reset_password(state, b).await,
        "AdminInitiateAuth" | "InitiateAuth" => initiate_auth(state, op == "AdminInitiateAuth", b).await,
        "RespondToAuthChallenge" | "AdminRespondToAuthChallenge" => respond_to_challenge(state, b).await,
        "GetUser" => {
            let token = req_str(b, "AccessToken")?;
            let ctx = Ctx::load(state).await?;
            let user = run(state, move |c| access_user(c, &ctx, &token)).await?;
            Ok(json!({
                "Username": user.username,
                "UserAttributes": user.attribute_list(None),
                "MFAOptions": [],
                "UserMFASettingList": [],
            }))
        }
        "GlobalSignOut" => {
            let token = req_str(b, "AccessToken")?;
            let ctx = Ctx::load(state).await?;
            run(state, move |c| {
                let mut user = access_user(c, &ctx, &token)?;
                svc::global_sign_out(c, &mut user)
            })
            .await?;
            Ok(json!({}))
        }
        "ChangePassword" => {
            let token = req_str(b, "AccessToken")?;
            let prev = req_str(b, "PreviousPassword")?;
            let proposed = req_str(b, "ProposedPassword")?;
            let ctx = Ctx::load(state).await?;
            run(state, move |c| {
                let mut user = access_user(c, &ctx, &token)?;
                if !svc::verify_password(&user, &prev) {
                    return Err(not_authorized(svc::BAD_CREDENTIALS));
                }
                let pool = svc::get_pool(c, &user.pool_id)?;
                svc::set_password(c, &pool, &mut user, &proposed, true)
            })
            .await?;
            Ok(json!({}))
        }
        "RevokeToken" => {
            let token = req_str(b, "Token")?;
            let cid = req_str(b, "ClientId")?;
            let secret = opt_str(b, "ClientSecret");
            run(state, move |c| {
                let client = svc::get_client(c, None, &cid)?;
                if let Some(s) = &client.secret {
                    if secret.as_deref() != Some(s.as_str()) {
                        return Err(not_authorized("Client secret is invalid."));
                    }
                }
                svc::revoke_refresh(c, &client, &token)
            })
            .await?;
            Ok(json!({}))
        }
        "SignUp" => sign_up(state, b).await,
        "ConfirmSignUp" => {
            let code = req_str(b, "ConfirmationCode")?;
            with_client_user(state, b, move |c, pool, user| {
                if user.status != "UNCONFIRMED" {
                    return Err(not_authorized(&format!(
                        "User cannot be confirmed. Current status is {}",
                        user.status
                    )));
                }
                svc::check_code(c, user, "signup", &code)?;
                confirm_user(c, pool, user)
            })
            .await
        }
        "ResendConfirmationCode" => send_code(state, b, "signup").await,
        "ForgotPassword" => send_code(state, b, "reset").await,
        "ConfirmForgotPassword" => {
            let code = req_str(b, "ConfirmationCode")?;
            let pw = req_str(b, "Password")?;
            with_client_user(state, b, move |c, pool, user| {
                svc::check_code(c, user, "reset", &code)?;
                svc::set_password(c, pool, user, &pw, true)?;
                if user.attr("email").is_some() {
                    user.attributes.insert("email_verified".into(), json!("true"));
                    svc::save_user(c, user)?;
                }
                Ok(())
            })
            .await
        }
        _ => Err(err(
            "InvalidAction",
            format!("The action {op} is not valid for this web service."),
        )),
    }
}

// ─── Request helpers ────────────────────────────────────────────────────────

fn opt_str(b: &Value, k: &str) -> Option<String> {
    b.get(k).and_then(|v| v.as_str()).map(String::from)
}

fn req_str(b: &Value, k: &str) -> CResult<String> {
    opt_str(b, k).ok_or_else(|| {
        let mut chars = k.chars();
        let lower: String = chars
            .next()
            .map(|c| c.to_lowercase().chain(chars).collect())
            .unwrap_or_default();
        err(
            "InvalidParameterException",
            format!("1 validation error detected: Value null at '{lower}' failed to satisfy constraint: Member must not be null"),
        )
    })
}

fn str_list(b: &Value, k: &str) -> Vec<String> {
    b.get(k)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

fn attr_list(b: &Value, k: &str) -> Vec<(String, String)> {
    b.get(k)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let n = x.get("Name")?.as_str()?;
                    let v = x.get("Value").and_then(|v| v.as_str()).unwrap_or_default();
                    Some((n.to_string(), v.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn send_emails(state: &AppState, emails: Vec<Vec<u8>>) {
    for raw in emails {
        if let Err(e) = crate::mailbox::service::capture_raw(&state.db, &raw, "cognito").await {
            tracing::warn!("cognito: failed to capture email: {e}");
        }
    }
}

async fn pool_user(state: &AppState, b: &Value) -> CResult<User> {
    let pid = req_str(b, "UserPoolId")?;
    let username = req_str(b, "Username")?;
    run(state, move |c| {
        let pool = svc::get_pool(c, &pid)?;
        svc::get_user(c, &pool, &username)
    })
    .await
}

/// Admin operation on `{UserPoolId, Username}` returning `{}`.
async fn with_user(
    state: &AppState,
    b: &Value,
    f: impl FnOnce(&rusqlite::Connection, &Pool, &mut User) -> CResult<()> + Send + 'static,
) -> CResult<Value> {
    let pid = req_str(b, "UserPoolId")?;
    let username = req_str(b, "Username")?;
    run(state, move |c| {
        let pool = svc::get_pool(c, &pid)?;
        let mut user = svc::get_user(c, &pool, &username)?;
        f(c, &pool, &mut user)
    })
    .await?;
    Ok(json!({}))
}

/// Public (client-scoped) operation on `{ClientId, Username}` returning `{}`.
async fn with_client_user(
    state: &AppState,
    b: &Value,
    f: impl FnOnce(&rusqlite::Connection, &Pool, &mut User) -> CResult<()> + Send + 'static,
) -> CResult<Value> {
    let cid = req_str(b, "ClientId")?;
    let username = req_str(b, "Username")?;
    run(state, move |c| {
        let client = svc::get_client(c, None, &cid)?;
        let pool = svc::get_pool(c, &client.pool_id)?;
        let mut user = svc::get_user(c, &pool, &username)?;
        f(c, &pool, &mut user)
    })
    .await?;
    Ok(json!({}))
}

fn confirm_user(c: &rusqlite::Connection, pool: &Pool, user: &mut User) -> CResult<()> {
    if user.status != "UNCONFIRMED" {
        return Err(not_authorized(&format!(
            "User cannot be confirmed. Current status is {}",
            user.status
        )));
    }
    user.status = "CONFIRMED".into();
    if pool.auto_verified().iter().any(|a| a == "email") && user.attr("email").is_some() {
        user.attributes.insert("email_verified".into(), json!("true"));
    }
    svc::save_user(c, user)
}

fn access_user(c: &rusqlite::Connection, ctx: &Ctx, token: &str) -> CResult<User> {
    let (_pool, user, claims) = svc::validate_access(c, ctx, token)?;
    if !svc::has_scope(&claims, svc::ADMIN_SCOPE) {
        return Err(not_authorized("Access Token does not have required scopes"));
    }
    Ok(user)
}

fn paginate<T>(items: Vec<T>, token: Option<&str>, limit: usize) -> CResult<(Vec<T>, Option<String>)> {
    let start = match token {
        Some(t) => t
            .parse::<usize>()
            .map_err(|_| err("InvalidParameterException", "Invalid pagination token."))?,
        None => 0,
    };
    let total = items.len();
    let page: Vec<T> = items.into_iter().skip(start).take(limit).collect();
    let next = (start + limit < total).then(|| (start + limit).to_string());
    Ok((page, next))
}

// ─── Pools ──────────────────────────────────────────────────────────────────

async fn create_user_pool(state: &AppState, b: &Value) -> CResult<Value> {
    let name = req_str(b, "PoolName")?;
    let mut data = b.as_object().cloned().unwrap_or_default();
    data.remove("PoolName");
    data.entry("UsernameConfiguration")
        .or_insert_with(|| json!({"CaseSensitive": false}));
    run(state, move |c| {
        let pool = svc::create_pool(c, None, &name, Value::Object(data))?;
        Ok(json!({"UserPool": pool.describe(0)}))
    })
    .await
}

async fn describe_user_pool(state: &AppState, b: &Value) -> CResult<Value> {
    let id = req_str(b, "UserPoolId")?;
    run(state, move |c| {
        let pool = svc::get_pool(c, &id)?;
        let n = svc::count_users(c, &id)?;
        Ok(json!({"UserPool": pool.describe(n)}))
    })
    .await
}

async fn list_user_pools(state: &AppState, b: &Value) -> CResult<Value> {
    let max = b
        .get("MaxResults")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            err(
                "InvalidParameterException",
                "1 validation error detected: Value null at 'maxResults' failed to satisfy constraint: Member must not be null",
            )
        })?;
    if !(1..=60).contains(&max) {
        return Err(err(
            "InvalidParameterException",
            "1 validation error detected: Value at 'maxResults' failed to satisfy constraint: Member must have value between 1 and 60",
        ));
    }
    let token = opt_str(b, "NextToken");
    let pools = run(state, |c| Ok(svc::list_pools(c)?)).await?;
    let (page, next) = paginate(pools, token.as_deref(), max as usize)?;
    let mut out = json!({"UserPools": page.iter().map(|p| p.summary()).collect::<Vec<_>>()});
    if let Some(n) = next {
        out["NextToken"] = json!(n);
    }
    Ok(out)
}

async fn update_user_pool(state: &AppState, b: &Value) -> CResult<Value> {
    let id = req_str(b, "UserPoolId")?;
    let updates = b.as_object().cloned().unwrap_or_default();
    run(state, move |c| {
        let mut pool = svc::get_pool(c, &id)?;
        let mut data = pool.data.as_object().cloned().unwrap_or_default();
        for (k, v) in updates {
            if matches!(k.as_str(), "UserPoolId" | "UsernameAttributes" | "AliasAttributes" | "PoolName") {
                continue;
            }
            data.insert(k, v);
        }
        pool.data = Value::Object(data);
        svc::save_pool(c, &pool)
    })
    .await?;
    Ok(json!({}))
}

// ─── Clients ────────────────────────────────────────────────────────────────

async fn create_client(state: &AppState, b: &Value) -> CResult<Value> {
    let pid = req_str(b, "UserPoolId")?;
    let name = req_str(b, "ClientName")?;
    let secret = b
        .get("GenerateSecret")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        .then(svc::new_client_secret);
    let data = svc::client_data_from(b)?;
    run(state, move |c| {
        let cl = svc::create_client(c, &pid, None, &name, secret, data)?;
        Ok(json!({"UserPoolClient": cl.describe()}))
    })
    .await
}

async fn list_clients(state: &AppState, b: &Value) -> CResult<Value> {
    let pid = req_str(b, "UserPoolId")?;
    let max = b.get("MaxResults").and_then(|v| v.as_u64()).unwrap_or(60).clamp(1, 60) as usize;
    let token = opt_str(b, "NextToken");
    let clients: Vec<Client> = run(state, move |c| {
        svc::get_pool(c, &pid)?;
        Ok(svc::list_clients(c, &pid)?)
    })
    .await?;
    let (page, next) = paginate(clients, token.as_deref(), max)?;
    let mut out = json!({"UserPoolClients": page.iter().map(|c| c.summary()).collect::<Vec<_>>()});
    if let Some(n) = next {
        out["NextToken"] = json!(n);
    }
    Ok(out)
}

async fn update_client(state: &AppState, b: &Value) -> CResult<Value> {
    let pid = req_str(b, "UserPoolId")?;
    let cid = req_str(b, "ClientId")?;
    let name = opt_str(b, "ClientName");
    let data = svc::client_data_from(b)?;
    run(state, move |c| {
        let mut cl = svc::get_client(c, Some(&pid), &cid)?;
        if let Some(n) = name {
            cl.name = n;
        }
        cl.data = data;
        svc::save_client(c, &cl)?;
        let cl = svc::get_client(c, None, &cid)?;
        Ok(json!({"UserPoolClient": cl.describe()}))
    })
    .await
}

// ─── Domains ────────────────────────────────────────────────────────────────

async fn create_domain(state: &AppState, b: &Value) -> CResult<Value> {
    let pid = req_str(b, "UserPoolId")?;
    let domain = req_str(b, "Domain")?;
    let custom = b.get("CustomDomainConfig").is_some();
    run(state, move |c| {
        let mut pool = svc::get_pool(c, &pid)?;
        if let Some(other) = svc::find_pool_by_domain(c, &domain)? {
            if other.id != pool.id {
                return Err(err("InvalidParameterException", "Domain already associated with another user pool."));
            }
        }
        if pool.domain.as_deref().is_some_and(|d| d != domain) {
            return Err(err(
                "InvalidParameterException",
                format!("User pool {pid} already has a domain configured."),
            ));
        }
        pool.domain = Some(domain.clone());
        svc::save_pool(c, &pool)?;
        Ok(if custom {
            json!({"CloudFrontDomain": format!("{domain}.cloudfront.net")})
        } else {
            json!({})
        })
    })
    .await
}

async fn describe_domain(state: &AppState, b: &Value) -> CResult<Value> {
    let domain = req_str(b, "Domain")?;
    let pool = run(state, move |c| Ok(svc::find_pool_by_domain(c, &domain)?)).await?;
    Ok(match pool {
        Some(p) => json!({"DomainDescription": {
            "UserPoolId": p.id,
            "AWSAccountId": crate::aws::ACCOUNT_ID,
            "Domain": p.domain,
            "S3Bucket": "aws-cognito-prod-cloudtwin-assets",
            "CloudFrontDistribution": format!("{}.cloudfront.net", p.domain.clone().unwrap_or_default()),
            "Version": "20220101000000",
            "Status": "ACTIVE",
        }}),
        None => json!({"DomainDescription": {}}),
    })
}

async fn delete_domain(state: &AppState, b: &Value) -> CResult<Value> {
    let pid = req_str(b, "UserPoolId")?;
    let domain = req_str(b, "Domain")?;
    run(state, move |c| {
        let mut pool = svc::get_pool(c, &pid)?;
        if pool.domain.as_deref() != Some(domain.as_str()) {
            return Err(err("InvalidParameterException", "No such domain or user pool exists."));
        }
        pool.domain = None;
        svc::save_pool(c, &pool)
    })
    .await?;
    Ok(json!({}))
}

// ─── Users ──────────────────────────────────────────────────────────────────

async fn admin_create_user(state: &AppState, b: &Value) -> CResult<Value> {
    let pid = req_str(b, "UserPoolId")?;
    let login = req_str(b, "Username")?;
    let attrs = attr_list(b, "UserAttributes");
    let temp = opt_str(b, "TemporaryPassword");
    let action = opt_str(b, "MessageAction");
    let email_medium = str_list(b, "DesiredDeliveryMediums").iter().any(|m| m == "EMAIL");
    if let Some(a) = &action {
        if a != "SUPPRESS" && a != "RESEND" {
            return Err(err("InvalidParameterException", format!("Invalid MessageAction: {a}")));
        }
    }
    let send = email_medium && action.as_deref() != Some("SUPPRESS");
    let (shape, emails) = run(state, move |c| {
        let pool = svc::get_pool(c, &pid)?;
        let password = match temp {
            Some(t) => {
                svc::check_policy(&pool, &t)?;
                t
            }
            None => svc::generate_password(&pool),
        };
        let user = if action.as_deref() == Some("RESEND") {
            let mut user = svc::get_user(c, &pool, &login)?;
            if user.status != "FORCE_CHANGE_PASSWORD" {
                return Err(err(
                    "UnsupportedUserStateException",
                    "Resend not possible. User is not in FORCE_CHANGE_PASSWORD state.",
                ));
            }
            svc::set_password(c, &pool, &mut user, &password, false)?;
            user
        } else {
            svc::create_user(
                c,
                &pool,
                NewUser {
                    login: login.clone(),
                    attributes: attrs,
                    password: Some(password.clone()),
                    status: "FORCE_CHANGE_PASSWORD".into(),
                    enabled: true,
                    sub: None,
                },
            )?
        };
        let mut emails = Vec::new();
        if send {
            match svc::invite_email(&pool, &user, &login, &password) {
                Some(m) => emails.push(m),
                None => {
                    return Err(err(
                        "InvalidParameterException",
                        "User has no attribute matching desired delivery mediums",
                    ))
                }
            }
        }
        Ok((user.list_shape(None), emails))
    })
    .await?;
    send_emails(state, emails).await;
    Ok(json!({"User": shape}))
}

async fn list_users(state: &AppState, b: &Value) -> CResult<Value> {
    let pid = req_str(b, "UserPoolId")?;
    let filter = svc::parse_filter(&opt_str(b, "Filter").unwrap_or_default())?;
    let limit = b.get("Limit").and_then(|v| v.as_u64()).unwrap_or(60);
    if !(1..=60).contains(&limit) {
        return Err(err(
            "InvalidParameterException",
            "1 validation error detected: Value at 'limit' failed to satisfy constraint: Member must have value less than or equal to 60",
        ));
    }
    let token = opt_str(b, "PaginationToken");
    let only: Option<Vec<String>> = b.get("AttributesToGet").filter(|v| v.is_array()).map(|_| str_list(b, "AttributesToGet"));
    let users = run(state, move |c| {
        svc::get_pool(c, &pid)?;
        Ok(svc::list_users(c, &pid)?)
    })
    .await?;
    let users: Vec<User> = users
        .into_iter()
        .filter(|u| filter.as_ref().is_none_or(|f| f.matches(u)))
        .collect();
    let (page, next) = paginate(users, token.as_deref(), limit as usize)?;
    let mut out = json!({
        "Users": page.iter().map(|u| u.list_shape(only.as_deref())).collect::<Vec<_>>()
    });
    if let Some(n) = next {
        out["PaginationToken"] = json!(n);
    }
    Ok(out)
}

async fn admin_reset_password(state: &AppState, b: &Value) -> CResult<Value> {
    let pid = req_str(b, "UserPoolId")?;
    let username = req_str(b, "Username")?;
    let emails = run(state, move |c| {
        let pool = svc::get_pool(c, &pid)?;
        let mut user = svc::get_user(c, &pool, &username)?;
        if !user.enabled {
            return Err(not_authorized("User is disabled."));
        }
        if user.status != "UNCONFIRMED" {
            user.status = "RESET_REQUIRED".into();
        }
        svc::save_user(c, &mut user)?;
        let code = svc::set_code(c, &user, "reset")?;
        Ok(svc::code_email(&pool, &user, &code).into_iter().collect::<Vec<_>>())
    })
    .await?;
    send_emails(state, emails).await;
    Ok(json!({}))
}

async fn sign_up(state: &AppState, b: &Value) -> CResult<Value> {
    let cid = req_str(b, "ClientId")?;
    let login = req_str(b, "Username")?;
    let pw = req_str(b, "Password")?;
    let attrs = attr_list(b, "UserAttributes");
    let (sub, delivery, emails) = run(state, move |c| {
        let client = svc::get_client(c, None, &cid)?;
        let pool = svc::get_pool(c, &client.pool_id)?;
        if pool.admin_create_only() {
            return Err(not_authorized("SignUp is not permitted for this user pool"));
        }
        svc::check_policy(&pool, &pw)?;
        let user = svc::create_user(
            c,
            &pool,
            NewUser {
                login,
                attributes: attrs,
                password: Some(pw),
                status: "UNCONFIRMED".into(),
                enabled: true,
                sub: None,
            },
        )?;
        let code = svc::set_code(c, &user, "signup")?;
        let emails: Vec<Vec<u8>> = svc::code_email(&pool, &user, &code).into_iter().collect();
        Ok((user.sub.clone(), svc::code_delivery(&user), emails))
    })
    .await?;
    send_emails(state, emails).await;
    Ok(json!({"UserConfirmed": false, "CodeDeliveryDetails": delivery, "UserSub": sub}))
}

async fn send_code(state: &AppState, b: &Value, kind: &'static str) -> CResult<Value> {
    let cid = req_str(b, "ClientId")?;
    let username = req_str(b, "Username")?;
    let (delivery, emails) = run(state, move |c| {
        let client = svc::get_client(c, None, &cid)?;
        let pool = svc::get_pool(c, &client.pool_id)?;
        let Some(user) = svc::find_user(c, &pool, &username)? else {
            if client.prevent_user_existence_errors() {
                return Ok((json!({"Destination": "u***@e***", "DeliveryMedium": "EMAIL", "AttributeName": "email"}), vec![]));
            }
            return Err(err("UserNotFoundException", "Username/client id combination not found."));
        };
        if !user.enabled {
            return Err(not_authorized("User is disabled."));
        }
        if kind == "signup" && user.status != "UNCONFIRMED" {
            return Err(err("InvalidParameterException", "User is already confirmed."));
        }
        let code = svc::set_code(c, &user, kind)?;
        let emails: Vec<Vec<u8>> = svc::code_email(&pool, &user, &code).into_iter().collect();
        Ok((svc::code_delivery(&user), emails))
    })
    .await?;
    send_emails(state, emails).await;
    Ok(json!({"CodeDeliveryDetails": delivery}))
}

// ─── Authentication ─────────────────────────────────────────────────────────

fn auth_result(ts: svc::TokenSet) -> Value {
    let mut r = json!({
        "AccessToken": ts.access_token,
        "ExpiresIn": ts.expires_in,
        "TokenType": "Bearer",
    });
    if let Some(id) = ts.id_token {
        r["IdToken"] = json!(id);
    }
    if let Some(rt) = ts.refresh_token {
        r["RefreshToken"] = json!(rt);
    }
    json!({"ChallengeParameters": {}, "AuthenticationResult": r})
}

fn new_password_challenge(c: &rusqlite::Connection, client: &Client, user: &User) -> CResult<Value> {
    let session = svc::create_challenge(c, client, user, "NEW_PASSWORD_REQUIRED")?;
    let attrs: Map<String, Value> = user.attributes.clone();
    Ok(json!({
        "ChallengeName": "NEW_PASSWORD_REQUIRED",
        "Session": session,
        "ChallengeParameters": {
            "USER_ID_FOR_SRP": user.username,
            "requiredAttributes": "[]",
            "userAttributes": Value::Object(attrs).to_string(),
        },
    }))
}

async fn initiate_auth(state: &AppState, admin: bool, b: &Value) -> CResult<Value> {
    let cid = req_str(b, "ClientId")?;
    let pool_id = if admin { Some(req_str(b, "UserPoolId")?) } else { None };
    let flow = req_str(b, "AuthFlow")?;
    let params = b.get("AuthParameters").cloned().unwrap_or_else(|| json!({}));
    let param = |k: &str| params.get(k).and_then(|v| v.as_str()).map(String::from);
    let missing = |k: &str| err("InvalidParameterException", format!("Missing required parameter {k}"));
    let ctx = Ctx::load(state).await?;
    match flow.as_str() {
        "REFRESH_TOKEN_AUTH" | "REFRESH_TOKEN" => {
            let token = param("REFRESH_TOKEN").ok_or_else(|| missing("REFRESH_TOKEN"))?;
            run(state, move |c| {
                let client = svc::get_client(c, pool_id.as_deref(), &cid)?;
                if !client.allows_flow("ALLOW_REFRESH_TOKEN_AUTH", "REFRESH_TOKEN_AUTH") {
                    return Err(err("InvalidParameterException", "Refresh Token auth flow not enabled for this client"));
                }
                Ok(auth_result(svc::refresh(c, &ctx, &client, &token)?))
            })
            .await
        }
        "ADMIN_USER_PASSWORD_AUTH" | "ADMIN_NO_SRP_AUTH" | "USER_PASSWORD_AUTH" => {
            let is_admin_flow = flow != "USER_PASSWORD_AUTH";
            if is_admin_flow != admin {
                return Err(err("InvalidParameterException", format!("Initiate Auth method not supported: {flow}")));
            }
            let username = param("USERNAME").ok_or_else(|| missing("USERNAME"))?;
            let password = param("PASSWORD").ok_or_else(|| missing("PASSWORD"))?;
            run(state, move |c| {
                let client = svc::get_client(c, pool_id.as_deref(), &cid)?;
                let (allow, legacy) = if is_admin_flow {
                    ("ALLOW_ADMIN_USER_PASSWORD_AUTH", "ADMIN_NO_SRP_AUTH")
                } else {
                    ("ALLOW_USER_PASSWORD_AUTH", "USER_PASSWORD_AUTH")
                };
                if !client.allows_flow(allow, legacy) {
                    return Err(err(
                        "InvalidParameterException",
                        format!("{} flow not enabled for this client", if is_admin_flow { "ADMIN_USER_PASSWORD_AUTH" } else { "USER_PASSWORD_AUTH" }),
                    ));
                }
                let pool = svc::get_pool(c, &client.pool_id)?;
                match svc::authenticate(c, &pool, Some(&client), &username, &password)? {
                    AuthOutcome::Ok(user) => Ok(auth_result(svc::issue_api_tokens(c, &ctx, &pool, &client, &user)?)),
                    AuthOutcome::NewPasswordRequired(user) => new_password_challenge(c, &client, &user),
                }
            })
            .await
        }
        "USER_SRP_AUTH" | "CUSTOM_AUTH" | "USER_AUTH" => Err(err(
            "InvalidParameterException",
            format!("{flow} is not supported by CloudTwin; use USER_PASSWORD_AUTH or ADMIN_USER_PASSWORD_AUTH"),
        )),
        _ => Err(err(
            "InvalidParameterException",
            format!("1 validation error detected: Value '{flow}' at 'authFlow' failed to satisfy constraint: Member must satisfy enum value set"),
        )),
    }
}

async fn respond_to_challenge(state: &AppState, b: &Value) -> CResult<Value> {
    let cid = req_str(b, "ClientId")?;
    let pool_id = opt_str(b, "UserPoolId");
    let name = req_str(b, "ChallengeName")?;
    let session = req_str(b, "Session")?;
    let responses = b.get("ChallengeResponses").cloned().unwrap_or_else(|| json!({}));
    if name != "NEW_PASSWORD_REQUIRED" {
        return Err(err("InvalidParameterException", format!("Unsupported challenge: {name}")));
    }
    let new_pw = responses
        .get("NEW_PASSWORD")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| err("InvalidParameterException", "Missing required parameter NEW_PASSWORD"))?;
    let attrs: Vec<(String, String)> = responses
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| {
            let n = k.strip_prefix("userAttributes.")?;
            Some((n.to_string(), v.as_str()?.to_string()))
        })
        .collect();
    let ctx = Ctx::load(state).await?;
    run(state, move |c| {
        let client = svc::get_client(c, pool_id.as_deref(), &cid)?;
        let pool = svc::get_pool(c, &client.pool_id)?;
        let mut user = svc::take_challenge(c, &client, &session, &name)?;
        if !user.enabled {
            return Err(not_authorized("User is disabled."));
        }
        svc::set_password(c, &pool, &mut user, &new_pw, true)?;
        if !attrs.is_empty() {
            svc::update_attributes(c, &pool, &mut user, &attrs)?;
        }
        Ok(auth_result(svc::issue_api_tokens(c, &ctx, &pool, &client, &user)?))
    })
    .await
}

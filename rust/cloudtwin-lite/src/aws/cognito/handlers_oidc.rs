//! OIDC discovery, JWKS, hosted UI (`/login`, `/logout`) and `/oauth2/*` endpoints.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::models::{now_i, Client, Pool, User};
use super::service::{self as svc, run, AuthOutcome, CogError, Ctx, IssueOpts};
use crate::proto::xml_escape as esc;
use crate::AppState;

type Params = HashMap<String, String>;

/// Pool-id path parameters are named `:bucket` so they share matchit's
/// parameter slot with S3's `/:bucket/*key` routes.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/:bucket/.well-known/openid-configuration", get(discovery))
        .route("/:bucket/.well-known/jwks.json", get(jwks))
        .route("/oauth2/authorize", get(authorize))
        .route("/login", get(authorize).post(login_post))
        .route("/oauth2/token", post(token))
        .route("/oauth2/userInfo", get(userinfo).post(userinfo))
        .route("/oauth2/userinfo", get(userinfo).post(userinfo))
        .route("/oauth2/revoke", post(revoke))
        .route("/logout", get(logout))
}

// ─── Small helpers ──────────────────────────────────────────────────────────

fn form(body: &Bytes) -> Params {
    serde_urlencoded::from_bytes(body).unwrap_or_default()
}

fn p<'a>(q: &'a Params, k: &str) -> Option<&'a str> {
    q.get(k).map(|s| s.as_str()).filter(|s| !s.is_empty())
}

fn oauth_error(status: StatusCode, error: &str, desc: Option<&str>) -> Response {
    let mut body = json!({"error": error});
    if let Some(d) = desc {
        body["error_description"] = json!(d);
    }
    (status, Json(body)).into_response()
}

fn internal(e: CogError) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "server_error", "error_description": e.msg})))
        .into_response()
}

fn redirect(location: &str) -> Response {
    let mut r = StatusCode::FOUND.into_response();
    if let Ok(v) = HeaderValue::from_str(location) {
        r.headers_mut().insert(header::LOCATION, v);
    }
    r
}

fn with_params(uri: &str, params: &[(&str, &str)], fragment: bool) -> String {
    let qs = serde_urlencoded::to_string(params).unwrap_or_default();
    if fragment {
        format!("{uri}#{qs}")
    } else if uri.contains('?') {
        format!("{uri}&{qs}")
    } else {
        format!("{uri}?{qs}")
    }
}

fn cookie_name(pool_id: &str) -> String {
    format!("cognito_session_{pool_id}")
}

fn cookies(headers: &HeaderMap) -> HashMap<String, String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| {
            let (k, v) = kv.trim().split_once('=')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

fn set_cookie(resp: &mut Response, value: String) {
    if let Ok(v) = HeaderValue::from_str(&value) {
        resp.headers_mut().append(header::SET_COOKIE, v);
    }
}

const CSS: &str = "*{box-sizing:border-box}body{margin:0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;background:#f2f4f7;color:#1d2433}\
.card{max-width:380px;margin:72px auto;background:#fff;border-radius:10px;box-shadow:0 4px 24px rgba(16,24,40,.08);padding:32px}\
h1{font-size:20px;margin:0 0 4px}.sub{color:#667085;font-size:13px;margin:0 0 24px}label{display:block;font-size:13px;font-weight:600;margin:14px 0 6px}\
input[type=text],input[type=password],input[type=email]{width:100%;padding:10px 12px;border:1px solid #d0d5dd;border-radius:6px;font-size:15px}\
input:focus{outline:none;border-color:#2563eb;box-shadow:0 0 0 3px rgba(37,99,235,.15)}button{width:100%;margin-top:22px;padding:11px;border:0;border-radius:6px;background:#2563eb;color:#fff;font-size:15px;font-weight:600;cursor:pointer}\
button:hover{background:#1d4ed8}.error{background:#fef3f2;border:1px solid #fecdca;color:#b42318;padding:10px 12px;border-radius:6px;font-size:13px;margin-bottom:8px}\
.foot{text-align:center;color:#98a2b3;font-size:12px;margin-top:20px}";

fn page(status: StatusCode, title: &str, body: &str) -> Response {
    let html = format!(
        "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>{t}</title><style>{CSS}</style></head><body><div class=\"card\">{body}</div>\
<div class=\"foot\">CloudTwin · local Cognito hosted UI</div></body></html>",
        t = esc(title)
    );
    let mut r = (status, html).into_response();
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

fn error_page(title: &str, msg: &str) -> Response {
    page(
        StatusCode::BAD_REQUEST,
        title,
        &format!("<h1>{}</h1><p class=\"sub\">An error was encountered with the requested page.</p><div class=\"error\">{}</div>", esc(title), esc(msg)),
    )
}

fn json_no_store(value: Value) -> Response {
    let mut r = Json(value).into_response();
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

// ─── Discovery / JWKS ───────────────────────────────────────────────────────

async fn pool_exists(state: &AppState, id: String) -> Result<bool, Response> {
    run(state, move |c| Ok(svc::find_pool(c, &id)?.is_some()))
        .await
        .map_err(internal)
}

fn not_found(pool_id: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"message": format!("User pool {pool_id} does not exist.")})),
    )
        .into_response()
}

async fn discovery(State(state): State<Arc<AppState>>, Path(pool_id): Path<String>) -> Response {
    match pool_exists(&state, pool_id.clone()).await {
        Err(r) => return r,
        Ok(false) => return not_found(&pool_id),
        Ok(true) => {}
    }
    let ctx = match Ctx::load(&state).await {
        Ok(c) => c,
        Err(e) => return internal(e),
    };
    let issuer = ctx.issuer(&pool_id);
    let base = &ctx.base_url;
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{base}/oauth2/authorize"),
        "token_endpoint": format!("{base}/oauth2/token"),
        "userinfo_endpoint": format!("{base}/oauth2/userInfo"),
        "revocation_endpoint": format!("{base}/oauth2/revoke"),
        "end_session_endpoint": format!("{base}/logout"),
        "jwks_uri": format!("{issuer}/.well-known/jwks.json"),
        "response_types_supported": ["code", "token"],
        "scopes_supported": ["openid", "email", "phone", "profile", svc::ADMIN_SCOPE],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
        "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post"],
        "code_challenge_methods_supported": ["S256", "plain"],
        "grant_types_supported": ["authorization_code", "refresh_token", "client_credentials", "implicit"],
    }))
    .into_response()
}

async fn jwks(State(state): State<Arc<AppState>>, Path(pool_id): Path<String>) -> Response {
    match pool_exists(&state, pool_id.clone()).await {
        Err(r) => return r,
        Ok(false) => return not_found(&pool_id),
        Ok(true) => {}
    }
    match Ctx::load(&state).await {
        Ok(ctx) => Json(ctx.keys.jwks()).into_response(),
        Err(e) => internal(e),
    }
}

// ─── Authorize request ──────────────────────────────────────────────────────

struct AuthReq {
    client: Client,
    pool: Pool,
    redirect_uri: String,
    state: Option<String>,
    scopes: Vec<String>,
    response_type: String,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    nonce: Option<String>,
}

impl AuthReq {
    fn params(&self) -> Vec<(&'static str, String)> {
        let mut v = vec![
            ("client_id", self.client.client_id.clone()),
            ("redirect_uri", self.redirect_uri.clone()),
            ("response_type", self.response_type.clone()),
            ("scope", self.scopes.join(" ")),
        ];
        for (k, val) in [
            ("state", &self.state),
            ("code_challenge", &self.code_challenge),
            ("code_challenge_method", &self.code_challenge_method),
            ("nonce", &self.nonce),
        ] {
            if let Some(x) = val {
                v.push((k, x.clone()));
            }
        }
        v
    }

    fn hidden_fields(&self) -> String {
        self.params()
            .iter()
            .map(|(k, v)| format!("<input type=\"hidden\" name=\"{k}\" value=\"{}\">", esc(v)))
            .collect()
    }

    fn error_redirect(&self, error: &str, desc: &str) -> Response {
        let mut params = vec![("error", error), ("error_description", desc)];
        if let Some(s) = &self.state {
            params.push(("state", s));
        }
        redirect(&with_params(&self.redirect_uri, &params, self.response_type == "token"))
    }
}

/// Validate authorize parameters. Errors before the redirect URI is trusted
/// render an error page; later errors redirect back with `error=`.
async fn prepare(state: &AppState, q: &Params) -> Result<AuthReq, Response> {
    let Some(client_id) = p(q, "client_id").map(String::from) else {
        return Err(error_page("Invalid request", "Required String parameter 'client_id' is not present"));
    };
    let found = run(state, move |c| {
        let Some(client) = svc::find_client(c, &client_id)? else { return Ok(None) };
        let pool = svc::get_pool(c, &client.pool_id)?;
        Ok(Some((client, pool)))
    })
    .await
    .map_err(internal)?;
    let Some((client, pool)) = found else {
        return Err(error_page("Invalid request", "Client does not exist."));
    };
    let redirect_uri = match p(q, "redirect_uri") {
        Some(r) => r.to_string(),
        None => match client.data.get("DefaultRedirectURI").and_then(|v| v.as_str()) {
            Some(d) => d.to_string(),
            None => return Err(error_page("Invalid request", "redirect_mismatch")),
        },
    };
    if !client.list("CallbackURLs").contains(&redirect_uri) {
        return Err(error_page("Invalid request", "redirect_mismatch"));
    }
    let allowed = client.list("AllowedOAuthScopes");
    let mut req = AuthReq {
        redirect_uri,
        state: p(q, "state").map(String::from),
        scopes: match p(q, "scope") {
            Some(s) => s.split_whitespace().map(String::from).collect(),
            None => allowed.clone(),
        },
        response_type: p(q, "response_type").unwrap_or_default().to_string(),
        code_challenge: p(q, "code_challenge").map(String::from),
        code_challenge_method: p(q, "code_challenge_method").map(String::from),
        nonce: p(q, "nonce").map(String::from),
        client,
        pool,
    };
    let flow = match req.response_type.as_str() {
        "code" => "code",
        "token" => "implicit",
        _ => {
            req.response_type = "code".into();
            return Err(req.error_redirect("unsupported_response_type", "response_type must be code or token"));
        }
    };
    if !req.client.oauth_enabled() || !req.client.list("AllowedOAuthFlows").iter().any(|f| f == flow) {
        return Err(req.error_redirect("unauthorized_client", "Client is not enabled for this OAuth flow"));
    }
    if req.scopes.is_empty() || req.scopes.iter().any(|s| !allowed.contains(s)) {
        return Err(req.error_redirect("invalid_scope", "Requested scope is not allowed for this client"));
    }
    if let Some(m) = &req.code_challenge_method {
        if m != "S256" && m != "plain" {
            return Err(req.error_redirect("invalid_request", "Unsupported code_challenge_method"));
        }
        if req.code_challenge.is_none() {
            return Err(req.error_redirect("invalid_request", "code_challenge is required"));
        }
    }
    if req.code_challenge.is_some() && req.code_challenge_method.is_none() {
        req.code_challenge_method = Some("plain".into());
    }
    Ok(req)
}

/// Issue the code (or implicit tokens) and redirect back to the client.
async fn complete(state: &AppState, req: &AuthReq, user: User, auth_time: i64) -> Response {
    let pool = req.pool.clone();
    let client = req.client.clone();
    let scopes = req.scopes.clone();
    let nonce = req.nonce.clone();
    let st = req.state.clone();
    if req.response_type == "token" {
        let ctx = match Ctx::load(state).await {
            Ok(c) => c,
            Err(e) => return internal(e),
        };
        let res = run(state, move |c| {
            let id_token = scopes.iter().any(|s| s == "openid");
            svc::issue_tokens(c, &ctx, &pool, &client, &user, IssueOpts {
                scopes,
                nonce,
                auth_time,
                id_token,
                refresh: false,
                origin_jti: None,
            })
        })
        .await;
        return match res {
            Ok(ts) => {
                let exp = ts.expires_in.to_string();
                let mut params: Vec<(&str, &str)> = vec![("access_token", &ts.access_token)];
                if let Some(id) = &ts.id_token {
                    params.push(("id_token", id));
                }
                params.push(("token_type", "Bearer"));
                params.push(("expires_in", &exp));
                if let Some(s) = &st {
                    params.push(("state", s));
                }
                redirect(&with_params(&req.redirect_uri, &params, true))
            }
            Err(e) => internal(e),
        };
    }
    let code = svc::AuthCode {
        client_id: client.client_id.clone(),
        username: user.username.clone(),
        redirect_uri: req.redirect_uri.clone(),
        scope: scopes.join(" "),
        code_challenge: req.code_challenge.clone(),
        code_challenge_method: req.code_challenge_method.clone(),
        nonce,
        auth_time,
    };
    match run(state, move |c| svc::create_code(c, &pool.id, &code)).await {
        Ok(code) => {
            let mut params: Vec<(&str, &str)> = vec![("code", &code)];
            if let Some(s) = &st {
                params.push(("state", s));
            }
            redirect(&with_params(&req.redirect_uri, &params, false))
        }
        Err(e) => internal(e),
    }
}

fn login_page(req: &AuthReq, error: Option<&str>, username: &str) -> Response {
    let err_html = error
        .map(|e| format!("<div class=\"error\">{}</div>", esc(e)))
        .unwrap_or_default();
    let label = if req.pool.email_is_username() { "Email" } else { "Username" };
    let body = format!(
        "<h1>Sign in</h1><p class=\"sub\">to {client}</p>{err_html}\
<form method=\"post\" action=\"/login\" autocomplete=\"on\">{hidden}\
<label for=\"username\">{label}</label><input id=\"username\" name=\"username\" type=\"text\" value=\"{user}\" autocapitalize=\"none\" autocomplete=\"username\" required autofocus>\
<label for=\"password\">Password</label><input id=\"password\" name=\"password\" type=\"password\" autocomplete=\"current-password\" required>\
<button type=\"submit\" name=\"signInSubmitButton\">Sign in</button></form>",
        client = esc(&req.client.name),
        hidden = req.hidden_fields(),
        user = esc(username),
    );
    page(if error.is_some() { StatusCode::BAD_REQUEST } else { StatusCode::OK }, "Signin", &body)
}

fn new_password_page(req: &AuthReq, session: &str, error: Option<&str>) -> Response {
    let err_html = error
        .map(|e| format!("<div class=\"error\">{}</div>", esc(e)))
        .unwrap_or_default();
    let body = format!(
        "<h1>Change password</h1><p class=\"sub\">Please enter a new password.</p>{err_html}\
<form method=\"post\" action=\"/login\">{hidden}<input type=\"hidden\" name=\"challenge_session\" value=\"{session}\">\
<label for=\"new_password\">New password</label><input id=\"new_password\" name=\"new_password\" type=\"password\" autocomplete=\"new-password\" required autofocus>\
<label for=\"confirm_password\">Confirm new password</label><input id=\"confirm_password\" name=\"confirm_password\" type=\"password\" autocomplete=\"new-password\" required>\
<button type=\"submit\">Send</button></form>",
        hidden = req.hidden_fields(),
        session = esc(session),
    );
    page(if error.is_some() { StatusCode::BAD_REQUEST } else { StatusCode::OK }, "Change password", &body)
}

// ─── /oauth2/authorize, /login ──────────────────────────────────────────────

async fn authorize(State(state): State<Arc<AppState>>, headers: HeaderMap, Query(q): Query<Params>) -> Response {
    let req = match prepare(&state, &q).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let force_login = p(&q, "prompt") == Some("login");
    if !force_login {
        if let Some(sid) = cookies(&headers).get(&cookie_name(&req.pool.id)).cloned() {
            let pid = req.pool.id.clone();
            match run(&state, move |c| svc::session_user(c, &pid, &sid)).await {
                Ok(Some((user, auth_time))) => return complete(&state, &req, user, auth_time).await,
                Ok(None) => {}
                Err(e) => return internal(e),
            }
        }
    }
    login_page(&req, None, "")
}

async fn login_post(State(state): State<Arc<AppState>>, body: Bytes) -> Response {
    let f = form(&body);
    let req = match prepare(&state, &f).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let client = req.client.clone();
    let pool = req.pool.clone();

    if let Some(session) = p(&f, "challenge_session").map(String::from) {
        let new_pw = p(&f, "new_password").unwrap_or_default().to_string();
        if p(&f, "confirm_password").unwrap_or_default() != new_pw {
            return new_password_page(&req, &session, Some("Passwords do not match"));
        }
        let sess = session.clone();
        let res = run(&state, move |c| {
            let mut user = svc::take_challenge(c, &client, &sess, "NEW_PASSWORD_REQUIRED")?;
            if !user.enabled {
                return Err(svc::not_authorized("User is disabled."));
            }
            svc::set_password(c, &pool, &mut user, &new_pw, true)?;
            Ok(user)
        })
        .await;
        return match res {
            Ok(user) => finish_login(&state, &req, user).await,
            Err(e) if e.code == "InvalidPasswordException" => new_password_page(&req, &session, Some(&e.msg)),
            Err(e) if e.code == "InternalErrorException" => internal(e),
            Err(e) => login_page(&req, Some(&e.msg), ""),
        };
    }

    let username = p(&f, "username").unwrap_or_default().trim().to_string();
    let password = p(&f, "password").unwrap_or_default().to_string();
    let uname = username.clone();
    let res = run(&state, move |c| {
        match svc::authenticate(c, &pool, Some(&client), &uname, &password)? {
            AuthOutcome::Ok(user) => Ok((user, None)),
            AuthOutcome::NewPasswordRequired(user) => {
                let s = svc::create_challenge(c, &client, &user, "NEW_PASSWORD_REQUIRED")?;
                Ok((user, Some(s)))
            }
        }
    })
    .await;
    match res {
        Ok((user, None)) => finish_login(&state, &req, user).await,
        Ok((_, Some(session))) => new_password_page(&req, &session, None),
        Err(e) if e.code == "InternalErrorException" => internal(e),
        Err(e) => {
            let msg = if e.code == "UserNotFoundException" { svc::BAD_CREDENTIALS } else { e.msg.as_str() };
            login_page(&req, Some(msg), &username)
        }
    }
}

async fn finish_login(state: &AppState, req: &AuthReq, user: User) -> Response {
    let auth_time = now_i();
    let u = user.clone();
    let sid = match run(state, move |c| svc::create_session(c, &u, auth_time)).await {
        Ok(s) => s,
        Err(e) => return internal(e),
    };
    let mut resp = complete(state, req, user, auth_time).await;
    set_cookie(
        &mut resp,
        format!("{}={sid}; Path=/; HttpOnly; SameSite=Lax", cookie_name(&req.pool.id)),
    );
    resp
}

// ─── /oauth2/token ──────────────────────────────────────────────────────────

/// Resolve and authenticate the calling client (HTTP Basic or body credentials).
async fn client_auth(state: &AppState, headers: &HeaderMap, f: &Params) -> Result<Client, Response> {
    let invalid = || oauth_error(StatusCode::UNAUTHORIZED, "invalid_client", None);
    let basic = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic ").or_else(|| v.strip_prefix("basic ")))
        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b.trim()).ok())
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| {
            let (id, secret) = s.split_once(':')?;
            let dec = |x: &str| {
                percent_encoding::percent_decode_str(&x.replace('+', " "))
                    .decode_utf8_lossy()
                    .to_string()
            };
            Some((dec(id), dec(secret)))
        });
    let (id, secret) = match basic {
        Some((id, secret)) => (id, Some(secret).filter(|s| !s.is_empty())),
        None => match p(f, "client_id") {
            Some(id) => (id.to_string(), p(f, "client_secret").map(String::from)),
            None => return Err(invalid()),
        },
    };
    let client = run(state, move |c| Ok(svc::find_client(c, &id)?))
        .await
        .map_err(internal)?
        .ok_or_else(invalid)?;
    if let Some(expected) = &client.secret {
        if secret.as_deref() != Some(expected.as_str()) {
            return Err(invalid());
        }
    }
    Ok(client)
}

fn verify_pkce(challenge: &str, method: &str, verifier: &str) -> bool {
    match method {
        "S256" => {
            let digest = Sha256::digest(verifier.as_bytes());
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest) == challenge
        }
        _ => verifier == challenge,
    }
}

fn token_response(ts: svc::TokenSet) -> Response {
    let mut out = json!({
        "access_token": ts.access_token,
        "expires_in": ts.expires_in,
        "token_type": "Bearer",
    });
    if let Some(id) = ts.id_token {
        out["id_token"] = json!(id);
    }
    if let Some(rt) = ts.refresh_token {
        out["refresh_token"] = json!(rt);
    }
    json_no_store(out)
}

async fn token(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    let f = form(&body);
    let bad = |e: &str, d: &str| oauth_error(StatusCode::BAD_REQUEST, e, Some(d));
    let Some(grant) = p(&f, "grant_type").map(String::from) else {
        return bad("invalid_request", "grant_type is required");
    };
    if !matches!(grant.as_str(), "authorization_code" | "refresh_token" | "client_credentials") {
        return bad("unsupported_grant_type", "Unsupported grant_type");
    }
    let client = match client_auth(&state, &headers, &f).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    let ctx = match Ctx::load(&state).await {
        Ok(c) => c,
        Err(e) => return internal(e),
    };
    match grant.as_str() {
        "authorization_code" => {
            let (Some(code), Some(redirect_uri)) = (p(&f, "code").map(String::from), p(&f, "redirect_uri").map(String::from))
            else {
                return bad("invalid_request", "code and redirect_uri are required");
            };
            if !client.oauth_enabled() || !client.list("AllowedOAuthFlows").iter().any(|f| f == "code") {
                return bad("unauthorized_client", "Client is not enabled for the code grant");
            }
            let verifier = p(&f, "code_verifier").map(String::from);
            // Codes are consumed even when the exchange fails, so the outer
            // transaction must commit: failures travel in the inner Result.
            let res = run(&state, move |c| {
                let Some(ac) = svc::take_code(c, &code)? else {
                    return Ok(Err("invalid_grant"));
                };
                if ac.client_id != client.client_id || ac.redirect_uri != redirect_uri {
                    return Ok(Err("invalid_grant"));
                }
                if let Some(ch) = &ac.code_challenge {
                    let method = ac.code_challenge_method.as_deref().unwrap_or("plain");
                    if !verifier.as_deref().is_some_and(|v| verify_pkce(ch, method, v)) {
                        return Ok(Err("invalid_grant"));
                    }
                }
                let pool = svc::get_pool(c, &client.pool_id)?;
                let Some(user) = svc::user_by_username(c, &pool.id, &ac.username)? else {
                    return Ok(Err("invalid_grant"));
                };
                if !user.enabled {
                    return Ok(Err("invalid_grant"));
                }
                let scopes: Vec<String> = ac.scope.split_whitespace().map(String::from).collect();
                let id_token = scopes.iter().any(|s| s == "openid");
                Ok(Ok(svc::issue_tokens(c, &ctx, &pool, &client, &user, IssueOpts {
                    scopes,
                    nonce: ac.nonce,
                    auth_time: ac.auth_time,
                    id_token,
                    refresh: true,
                    origin_jti: None,
                })?))
            })
            .await;
            match res {
                Ok(Ok(ts)) => token_response(ts),
                Ok(Err(e)) => oauth_error(StatusCode::BAD_REQUEST, e, None),
                Err(e) => internal(e),
            }
        }
        "refresh_token" => {
            let Some(rt) = p(&f, "refresh_token").map(String::from) else {
                return bad("invalid_request", "refresh_token is required");
            };
            match run(&state, move |c| svc::refresh(c, &ctx, &client, &rt)).await {
                Ok(ts) => token_response(ts),
                Err(e) if e.code == "InternalErrorException" => internal(e),
                Err(_) => oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", None),
            }
        }
        _ => {
            if client.secret.is_none() {
                return oauth_error(StatusCode::UNAUTHORIZED, "invalid_client", None);
            }
            if !client.oauth_enabled() || !client.list("AllowedOAuthFlows").iter().any(|f| f == "client_credentials") {
                return bad("unauthorized_client", "Client is not enabled for client_credentials");
            }
            let allowed: Vec<String> = client
                .list("AllowedOAuthScopes")
                .into_iter()
                .filter(|s| s.contains('/'))
                .collect();
            let scopes: Vec<String> = match p(&f, "scope") {
                Some(s) => s.split_whitespace().map(String::from).collect(),
                None => allowed.clone(),
            };
            if scopes.is_empty() || scopes.iter().any(|s| !allowed.contains(s)) {
                return bad("invalid_scope", "Requested scope is not allowed for this client");
            }
            let (token, ttl) = svc::client_credentials_token(&ctx, &client, &scopes);
            json_no_store(json!({"access_token": token, "expires_in": ttl, "token_type": "Bearer"}))
        }
    }
}

// ─── /oauth2/userInfo ───────────────────────────────────────────────────────

async fn userinfo(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let unauthorized = |desc: &str| {
        let mut r = oauth_error(StatusCode::UNAUTHORIZED, "invalid_token", Some(desc));
        r.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Bearer error=\"invalid_token\""),
        );
        r
    };
    let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
        .map(|s| s.trim().to_string())
    else {
        return unauthorized("Access token is missing");
    };
    let ctx = match Ctx::load(&state).await {
        Ok(c) => c,
        Err(e) => return internal(e),
    };
    match run(&state, move |c| svc::validate_access(c, &ctx, &token)).await {
        Ok((_pool, user, _claims)) => {
            let mut out = serde_json::Map::new();
            out.insert("sub".into(), json!(user.sub));
            for (k, v) in &user.attributes {
                out.insert(k.clone(), v.clone());
            }
            out.insert("username".into(), json!(user.username));
            json_no_store(Value::Object(out))
        }
        Err(e) if e.code == "InternalErrorException" => internal(e),
        Err(e) => unauthorized(&e.msg),
    }
}

// ─── /oauth2/revoke ─────────────────────────────────────────────────────────

async fn revoke(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    let f = form(&body);
    let Some(tok) = p(&f, "token").map(String::from) else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_request", Some("token is required"));
    };
    let client = match client_auth(&state, &headers, &f).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    if tok.split('.').count() == 3 {
        return oauth_error(StatusCode::BAD_REQUEST, "unsupported_token_type", None);
    }
    match run(&state, move |c| svc::revoke_refresh(c, &client, &tok)).await {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) if e.code == "InternalErrorException" => internal(e),
        Err(_) => oauth_error(StatusCode::BAD_REQUEST, "unauthorized_client", None),
    }
}

// ─── /logout ────────────────────────────────────────────────────────────────

async fn logout(State(state): State<Arc<AppState>>, headers: HeaderMap, Query(q): Query<Params>) -> Response {
    let Some(client_id) = p(&q, "client_id").map(String::from) else {
        return error_page("Invalid request", "Required String parameter 'client_id' is not present");
    };
    let client = match run(&state, move |c| Ok(svc::find_client(c, &client_id)?)).await {
        Ok(Some(c)) => c,
        Ok(None) => return error_page("Invalid request", "Client does not exist."),
        Err(e) => return internal(e),
    };
    let target = if let Some(uri) = p(&q, "logout_uri") {
        if !client.list("LogoutURLs").iter().any(|u| u == uri) {
            return error_page("Invalid request", "Invalid logout_uri: it must be one of the client's sign-out URLs.");
        }
        uri.to_string()
    } else if p(&q, "redirect_uri").is_some() && p(&q, "response_type").is_some() {
        let params: Vec<(&str, &str)> = q
            .iter()
            .filter(|(k, _)| k.as_str() != "logout_uri")
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        format!("/oauth2/authorize?{}", serde_urlencoded::to_string(params).unwrap_or_default())
    } else {
        return error_page("Invalid request", "Required String parameter 'logout_uri' is not present");
    };
    let name = cookie_name(&client.pool_id);
    if let Some(sid) = cookies(&headers).get(&name).cloned() {
        if let Err(e) = run(&state, move |c| svc::delete_session(c, &sid)).await {
            return internal(e);
        }
    }
    let mut resp = redirect(&target);
    set_cookie(&mut resp, format!("{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"));
    resp
}

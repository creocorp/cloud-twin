use std::collections::HashMap;

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use base64::Engine;
use rsa::pkcs1v15::{Signature, VerifyingKey};
use rsa::signature::Verifier;
use rsa::{BigUint, RsaPublicKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::testutil::{aws_json, raw, rest, TestApp, TestResponse};

const POOL: &str = "us-east-1_localpool";
const CLIENT: &str = "fixedclientid";
const CB: &str = "http://localhost:3000/callback";

async fn op(t: &TestApp, name: &str, body: Value) -> TestResponse {
    t.call(aws_json(
        &format!("AWSCognitoIdentityProviderService.{name}"),
        body,
    ))
    .await
}

async fn ok(t: &TestApp, name: &str, body: Value) -> Value {
    let r = op(t, name, body).await;
    assert_eq!(r.status, 200, "{name}: {}", r.text());
    r.json()
}

async fn fail(t: &TestApp, name: &str, body: Value) -> (String, String) {
    let r = op(t, name, body).await;
    assert_eq!(
        r.status,
        400,
        "{name} unexpectedly returned {}: {}",
        r.status,
        r.text()
    );
    let j = r.json();
    (
        j["__type"].as_str().unwrap().to_string(),
        j["message"].as_str().unwrap().to_string(),
    )
}

async fn seed(t: &TestApp, doc: Value) {
    let r = t
        .call(rest(
            "POST",
            "/_cloudtwin/seed",
            Some(json!({"cognito": doc})),
        ))
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
}

fn standard_seed() -> Value {
    json!({"user_pools": [{
        "id": POOL,
        "name": "orkystra",
        "username_attributes": ["email"],
        "domain": "orkystra-local",
        "clients": [{
            "client_id": CLIENT,
            "name": "web",
            "callback_urls": [CB],
            "logout_urls": ["http://localhost:3000/"],
        }],
        "users": [{
            "email": "dev@example.com",
            "password": "Passw0rd!",
            "attributes": {"name": "Dev User", "given_name": "Dev"},
        }],
    }]})
}

fn form_req(
    uri: &str,
    pairs: &[(&str, &str)],
    extra: &[(&str, &str)],
) -> axum::http::Request<axum::body::Body> {
    let mut headers = vec![("content-type", "application/x-www-form-urlencoded")];
    headers.extend_from_slice(extra);
    raw(
        "POST",
        uri,
        &headers,
        serde_urlencoded::to_string(pairs).unwrap(),
    )
}

fn get(uri: &str, headers: &[(&str, &str)]) -> axum::http::Request<axum::body::Body> {
    raw("GET", uri, headers, Vec::new())
}

fn claims(token: &str) -> Value {
    let part = token.split('.').nth(1).unwrap();
    serde_json::from_slice(&B64URL.decode(part).unwrap()).unwrap()
}

/// Verify an RS256 JWT with the JWKS published by the server.
fn verify_with_jwks(jwks: &Value, token: &str) -> Value {
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3);
    let header: Value = serde_json::from_slice(&B64URL.decode(parts[0]).unwrap()).unwrap();
    assert_eq!(header["alg"], "RS256");
    let key = jwks["keys"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["kid"] == header["kid"])
        .expect("kid present in JWKS");
    let n = BigUint::from_bytes_be(&B64URL.decode(key["n"].as_str().unwrap()).unwrap());
    let e = BigUint::from_bytes_be(&B64URL.decode(key["e"].as_str().unwrap()).unwrap());
    let vk = VerifyingKey::<Sha256>::new(RsaPublicKey::new(n, e).unwrap());
    let sig = Signature::try_from(B64URL.decode(parts[2]).unwrap().as_slice()).unwrap();
    vk.verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig)
        .expect("signature verifies");
    claims(token)
}

fn query_of(location: &str) -> HashMap<String, String> {
    let q = location.split_once('?').map(|(_, q)| q).unwrap_or("");
    serde_urlencoded::from_str(q).unwrap()
}

fn hidden_value(html: &str, name: &str) -> String {
    let marker = format!("name=\"{name}\" value=\"");
    let start = html.find(&marker).expect("hidden field") + marker.len();
    let end = html[start..].find('"').unwrap();
    html[start..start + end].to_string()
}

fn session_cookie(r: &TestResponse) -> String {
    let c = r.header("set-cookie").expect("set-cookie");
    assert!(c.contains("HttpOnly") && c.contains("Path=/"), "{c}");
    c.split(';').next().unwrap().to_string()
}

fn pkce() -> (String, String) {
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_string();
    let challenge = B64URL.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn authorize_uri(challenge: &str) -> String {
    format!(
        "/oauth2/authorize?{}",
        serde_urlencoded::to_string([
            ("response_type", "code"),
            ("client_id", CLIENT),
            ("redirect_uri", CB),
            ("state", "xyz"),
            ("scope", "openid email profile"),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
            ("nonce", "n-0S6"),
        ])
        .unwrap()
    )
}

const HIDDEN: [&str; 8] = [
    "client_id",
    "redirect_uri",
    "response_type",
    "scope",
    "state",
    "code_challenge",
    "code_challenge_method",
    "nonce",
];

async fn login_form(t: &TestApp, uri: &str, username: &str, password: &str) -> TestResponse {
    let page = t.call(get(uri, &[])).await;
    assert_eq!(page.status, 200, "{}", page.text());
    let html = page.text();
    let mut pairs: Vec<(String, String)> = Vec::new();
    for name in HIDDEN {
        if html.contains(&format!("name=\"{name}\"")) {
            pairs.push((name.into(), hidden_value(&html, name)));
        }
    }
    pairs.push(("username".into(), username.into()));
    pairs.push(("password".into(), password.into()));
    let refs: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    t.call(form_req("/login", &refs, &[])).await
}

fn urlenc(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

#[tokio::test]
async fn discovery_and_jwks_shape() {
    let t = TestApp::new().await;
    seed(&t, standard_seed()).await;
    let r = t
        .call(get(
            &format!("/{POOL}/.well-known/openid-configuration"),
            &[],
        ))
        .await;
    assert_eq!(r.status, 200);
    let d = r.json();
    let iss = format!("http://localhost:4793/{POOL}");
    assert_eq!(d["issuer"], iss);
    assert_eq!(
        d["authorization_endpoint"],
        "http://localhost:4793/oauth2/authorize"
    );
    assert_eq!(d["token_endpoint"], "http://localhost:4793/oauth2/token");
    assert_eq!(
        d["userinfo_endpoint"],
        "http://localhost:4793/oauth2/userInfo"
    );
    assert_eq!(
        d["revocation_endpoint"],
        "http://localhost:4793/oauth2/revoke"
    );
    assert_eq!(d["end_session_endpoint"], "http://localhost:4793/logout");
    assert_eq!(d["jwks_uri"], format!("{iss}/.well-known/jwks.json"));
    assert_eq!(
        d["code_challenge_methods_supported"],
        json!(["S256", "plain"])
    );
    assert_eq!(d["id_token_signing_alg_values_supported"], json!(["RS256"]));
    assert_eq!(d["response_types_supported"], json!(["code", "token"]));

    let j = t
        .call(get(&format!("/{POOL}/.well-known/jwks.json"), &[]))
        .await
        .json();
    let k = &j["keys"][0];
    assert_eq!(k["alg"], "RS256");
    assert_eq!(k["kty"], "RSA");
    assert_eq!(k["use"], "sig");
    assert_eq!(k["e"], "AQAB");
    assert!(k["n"].as_str().unwrap().len() > 300);

    assert_eq!(
        t.call(get("/us-east-1_missing/.well-known/jwks.json", &[]))
            .await
            .status,
        404
    );
    assert_eq!(
        t.call(get(
            "/us-east-1_missing/.well-known/openid-configuration",
            &[]
        ))
        .await
        .status,
        404
    );
}

#[tokio::test]
async fn issuer_base_url_override() {
    let t =
        TestApp::with_config(|c| c.cognito.issuer_base_url = Some("http://cloudtwin:4793/".into()))
            .await;
    seed(&t, standard_seed()).await;
    let d = t
        .call(get(
            &format!("/{POOL}/.well-known/openid-configuration"),
            &[],
        ))
        .await
        .json();
    assert_eq!(d["issuer"], format!("http://cloudtwin:4793/{POOL}"));
    let r = ok(
        &t,
        "InitiateAuth",
        json!({"ClientId": CLIENT, "AuthFlow": "USER_PASSWORD_AUTH",
        "AuthParameters": {"USERNAME": "dev@example.com", "PASSWORD": "Passw0rd!"}}),
    )
    .await;
    assert_eq!(
        claims(r["AuthenticationResult"]["IdToken"].as_str().unwrap())["iss"],
        d["issuer"]
    );
}

#[tokio::test]
async fn hosted_ui_code_pkce_full_flow() {
    let t = TestApp::new().await;
    seed(&t, standard_seed()).await;
    let jwks = t
        .call(get(&format!("/{POOL}/.well-known/jwks.json"), &[]))
        .await
        .json();
    let (verifier, challenge) = pkce();
    let uri = authorize_uri(&challenge);

    // Login page and a wrong password.
    let page = t.call(get(&uri, &[])).await;
    assert_eq!(page.status, 200);
    assert!(page
        .header("content-type")
        .unwrap()
        .starts_with("text/html"));
    assert!(page.text().contains("name=\"password\""));
    let bad = login_form(&t, &uri, "dev@example.com", "wrong").await;
    assert_eq!(bad.status, 400);
    assert!(bad.text().contains("Incorrect username or password."));
    assert!(bad.header("location").is_none());

    // Correct credentials → session cookie + redirect with code and state.
    let r = login_form(&t, &uri, "DEV@example.com", "Passw0rd!").await;
    assert_eq!(r.status, 302, "{}", r.text());
    let cookie = session_cookie(&r);
    let loc = r.header("location").unwrap();
    assert!(loc.starts_with(&format!("{CB}?")), "{loc}");
    let q = query_of(&loc);
    assert_eq!(q["state"], "xyz");
    let code = q["code"].clone();

    // Wrong verifier fails (and burns the code).
    let wrong = t
        .call(form_req(
            "/oauth2/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", CLIENT),
                ("code", &code),
                ("redirect_uri", CB),
                ("code_verifier", "nope"),
            ],
            &[],
        ))
        .await;
    assert_eq!(wrong.status, 400);
    assert_eq!(wrong.json()["error"], "invalid_grant");

    // Existing session → immediate redirect with a fresh code.
    let r = t.call(get(&uri, &[("cookie", &cookie)])).await;
    assert_eq!(r.status, 302);
    let code = query_of(&r.header("location").unwrap())["code"].clone();

    let exchange = |code: String| {
        form_req(
            "/oauth2/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", CLIENT),
                ("code", &code),
                ("redirect_uri", CB),
                ("code_verifier", &verifier),
            ],
            &[],
        )
    };
    let r = t.call(exchange(code.clone())).await;
    assert_eq!(r.status, 200, "{}", r.text());
    let tok = r.json();
    assert_eq!(tok["token_type"], "Bearer");
    assert_eq!(tok["expires_in"], 3600);
    let id_token = tok["id_token"].as_str().unwrap().to_string();
    let access_token = tok["access_token"].as_str().unwrap().to_string();
    let refresh_token = tok["refresh_token"].as_str().unwrap().to_string();

    // Single use.
    let again = t.call(exchange(code)).await;
    assert_eq!(again.json()["error"], "invalid_grant");

    // Claims and signatures.
    let iss = format!("http://localhost:4793/{POOL}");
    let idc = verify_with_jwks(&jwks, &id_token);
    assert_eq!(idc["iss"], iss);
    assert_eq!(idc["aud"], CLIENT);
    assert_eq!(idc["token_use"], "id");
    assert_eq!(idc["email"], "dev@example.com");
    assert_eq!(idc["email_verified"], true);
    assert_eq!(idc["name"], "Dev User");
    assert_eq!(idc["nonce"], "n-0S6");
    assert_eq!(idc["cognito:username"], idc["sub"]);
    assert_eq!(idc["sub"].as_str().unwrap().len(), 36);
    assert_eq!(
        idc["exp"].as_i64().unwrap() - idc["iat"].as_i64().unwrap(),
        3600
    );
    for k in ["origin_jti", "event_id", "auth_time", "jti"] {
        assert!(idc.get(k).is_some(), "id token missing {k}");
    }
    let ac = verify_with_jwks(&jwks, &access_token);
    assert_eq!(ac["token_use"], "access");
    assert_eq!(ac["client_id"], CLIENT);
    assert!(ac.get("aud").is_none());
    assert_eq!(ac["scope"], "openid email profile");
    assert_eq!(ac["username"], idc["sub"]);
    assert_eq!(ac["version"], 2);
    assert_eq!(ac["origin_jti"], idc["origin_jti"]);

    // userInfo
    let ui = t
        .call(get(
            "/oauth2/userInfo",
            &[("authorization", &format!("Bearer {access_token}"))],
        ))
        .await;
    assert_eq!(ui.status, 200, "{}", ui.text());
    let ui = ui.json();
    assert_eq!(ui["email"], "dev@example.com");
    assert_eq!(ui["email_verified"], "true");
    assert_eq!(ui["sub"], idc["sub"]);
    assert_eq!(ui["username"], idc["sub"]);
    let bad = t
        .call(get(
            "/oauth2/userInfo",
            &[("authorization", "Bearer garbage")],
        ))
        .await;
    assert_eq!(bad.status, 401);
    assert_eq!(bad.json()["error"], "invalid_token");

    // Refresh: new id/access tokens, no new refresh token.
    let r = t
        .call(form_req(
            "/oauth2/token",
            &[
                ("grant_type", "refresh_token"),
                ("client_id", CLIENT),
                ("refresh_token", &refresh_token),
            ],
            &[],
        ))
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
    let rt = r.json();
    assert!(rt.get("refresh_token").is_none());
    let new_access = rt["access_token"].as_str().unwrap().to_string();
    assert_eq!(
        verify_with_jwks(&jwks, rt["id_token"].as_str().unwrap())["origin_jti"],
        idc["origin_jti"]
    );

    // Revoke → refresh fails, derived access tokens rejected.
    let r = t
        .call(form_req(
            "/oauth2/revoke",
            &[("token", &refresh_token), ("client_id", CLIENT)],
            &[],
        ))
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
    let r = t
        .call(form_req(
            "/oauth2/token",
            &[
                ("grant_type", "refresh_token"),
                ("client_id", CLIENT),
                ("refresh_token", &refresh_token),
            ],
            &[],
        ))
        .await;
    assert_eq!(r.json()["error"], "invalid_grant");
    for at in [&access_token, &new_access] {
        let ui = t
            .call(get(
                "/oauth2/userInfo",
                &[("authorization", &format!("Bearer {at}"))],
            ))
            .await;
        assert_eq!(ui.status, 401);
    }

    // Logout: bad logout_uri → error page; good → cookie cleared + redirect.
    let r = t
        .call(get(
            &format!("/logout?client_id={CLIENT}&logout_uri=http://evil.example/"),
            &[("cookie", &cookie)],
        ))
        .await;
    assert_eq!(r.status, 400);
    assert!(r.header("location").is_none());
    let r = t
        .call(get(
            &format!(
                "/logout?client_id={CLIENT}&logout_uri={}",
                urlenc("http://localhost:3000/")
            ),
            &[("cookie", &cookie)],
        ))
        .await;
    assert_eq!(r.status, 302);
    assert_eq!(r.header("location").unwrap(), "http://localhost:3000/");
    assert!(r.header("set-cookie").unwrap().contains("Max-Age=0"));
    // The old session no longer short-circuits the login page.
    let r = t.call(get(&uri, &[("cookie", &cookie)])).await;
    assert_eq!(r.status, 200);
    assert!(r.text().contains("Sign in"));
}

#[tokio::test]
async fn authorize_validation_and_logout_redirect_variant() {
    let t = TestApp::new().await;
    seed(&t, standard_seed()).await;
    // Unknown redirect URI → error page, never a redirect.
    let r = t
        .call(get(
            &format!(
                "/oauth2/authorize?response_type=code&client_id={CLIENT}&redirect_uri=http://evil/"
            ),
            &[],
        ))
        .await;
    assert_eq!(r.status, 400);
    assert!(r.header("location").is_none());
    assert!(r.text().contains("redirect_mismatch"));
    // Unknown client
    assert_eq!(
        t.call(get(
            "/oauth2/authorize?response_type=code&client_id=nope&redirect_uri=x",
            &[]
        ))
        .await
        .status,
        400
    );
    // Disallowed scope → redirect with error
    let r = t
        .call(get(&format!(
            "/oauth2/authorize?response_type=code&client_id={CLIENT}&redirect_uri={}&scope=phone&state=s1",
            urlenc(CB)
        ), &[]))
        .await;
    assert_eq!(r.status, 302);
    let q = query_of(&r.header("location").unwrap());
    assert_eq!(q["error"], "invalid_scope");
    assert_eq!(q["state"], "s1");
    // logout with redirect_uri + response_type → back to authorize
    let r = t
        .call(get(
            &format!(
                "/logout?client_id={CLIENT}&redirect_uri={}&response_type=code",
                urlenc(CB)
            ),
            &[],
        ))
        .await;
    assert_eq!(r.status, 302);
    assert!(r
        .header("location")
        .unwrap()
        .starts_with("/oauth2/authorize?"));
    // Token endpoint errors
    let r = t
        .call(form_req(
            "/oauth2/token",
            &[("grant_type", "password")],
            &[],
        ))
        .await;
    assert_eq!(r.json()["error"], "unsupported_grant_type");
    let r = t
        .call(form_req(
            "/oauth2/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", "nope"),
                ("code", "x"),
                ("redirect_uri", CB),
            ],
            &[],
        ))
        .await;
    assert_eq!(r.status, 401);
    assert_eq!(r.json()["error"], "invalid_client");
}

#[tokio::test]
async fn confidential_client_auth_and_client_credentials() {
    let t = TestApp::new().await;
    seed(&t, json!({"user_pools": [{
        "id": POOL, "name": "p",
        "clients": [{"client_id": "svc", "name": "svc", "secret": "s3cret",
                     "callback_urls": [CB], "flows": ["code", "client_credentials"],
                     "scopes": ["openid", "api/read", "api/write"], "access_token_ttl_seconds": 600}],
        "users": [{"username": "alice", "email": "alice@example.com", "password": "Passw0rd!"}],
    }]}))
    .await;
    let uri = format!(
        "/oauth2/authorize?response_type=code&client_id=svc&redirect_uri={}&scope=openid",
        urlenc(CB)
    );
    let r = login_form(&t, &uri, "alice", "Passw0rd!").await;
    assert_eq!(r.status, 302, "{}", r.text());
    let code = query_of(&r.header("location").unwrap())["code"].clone();
    let body = [
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", CB),
        ("client_id", "svc"),
    ];
    let r = t.call(form_req("/oauth2/token", &body, &[])).await;
    assert_eq!(r.status, 401, "missing secret must fail");
    let basic = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("svc:s3cret")
    );
    let r = t
        .call(form_req(
            "/oauth2/token",
            &body,
            &[("authorization", &basic)],
        ))
        .await;
    // The code was not consumed by the unauthenticated attempt.
    assert_eq!(r.status, 200, "{}", r.text());
    let tok = r.json();
    assert_eq!(tok["expires_in"], 600);
    assert_eq!(
        claims(tok["id_token"].as_str().unwrap())["cognito:username"],
        "alice"
    );

    let r = t
        .call(form_req(
            "/oauth2/token",
            &[
                ("grant_type", "client_credentials"),
                ("client_id", "svc"),
                ("client_secret", "s3cret"),
            ],
            &[],
        ))
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
    let j = r.json();
    assert!(j.get("refresh_token").is_none() && j.get("id_token").is_none());
    let c = claims(j["access_token"].as_str().unwrap());
    assert_eq!(c["scope"], "api/read api/write");
    assert_eq!(c["sub"], "svc");
    let r = t
        .call(form_req(
            "/oauth2/token",
            &[("grant_type", "client_credentials"), ("scope", "openid")],
            &[("authorization", &basic)],
        ))
        .await;
    assert_eq!(r.json()["error"], "invalid_scope");
}

#[tokio::test]
async fn hosted_ui_force_change_password() {
    let t = TestApp::new().await;
    seed(&t, standard_seed()).await;
    ok(
        &t,
        "AdminCreateUser",
        json!({"UserPoolId": POOL, "Username": "new@example.com",
        "TemporaryPassword": "TempPassw0rd!", "MessageAction": "SUPPRESS"}),
    )
    .await;
    let (_, challenge) = pkce();
    let uri = authorize_uri(&challenge);
    let r = login_form(&t, &uri, "new@example.com", "TempPassw0rd!").await;
    assert_eq!(r.status, 200, "{}", r.text());
    let html = r.text();
    assert!(html.contains("new_password"));
    let session = hidden_value(&html, "challenge_session");
    let mut pairs: Vec<(String, String)> = HIDDEN
        .iter()
        .map(|n| (n.to_string(), hidden_value(&html, n)))
        .collect();
    pairs.push(("challenge_session".into(), session));
    let mut weak = pairs.clone();
    weak.push(("new_password".into(), "short".into()));
    weak.push(("confirm_password".into(), "short".into()));
    let refs: Vec<(&str, &str)> = weak.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let r = t.call(form_req("/login", &refs, &[])).await;
    assert_eq!(r.status, 400);
    assert!(r.text().contains("Password does not conform to policy"));
    pairs.push(("new_password".into(), "BrandNewPassw0rd!".into()));
    pairs.push(("confirm_password".into(), "BrandNewPassw0rd!".into()));
    let refs: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let r = t.call(form_req("/login", &refs, &[])).await;
    assert_eq!(r.status, 302, "{}", r.text());
    assert!(query_of(&r.header("location").unwrap()).contains_key("code"));
    let u = ok(
        &t,
        "AdminGetUser",
        json!({"UserPoolId": POOL, "Username": "new@example.com"}),
    )
    .await;
    assert_eq!(u["UserStatus"], "CONFIRMED");
}

#[tokio::test]
async fn admin_create_user_invite_email_and_new_password_challenge() {
    let t = TestApp::new().await;
    let pool = ok(
        &t,
        "CreateUserPool",
        json!({
            "PoolName": "invites",
            "UsernameAttributes": ["email"],
            "AutoVerifiedAttributes": ["email"],
            "AdminCreateUserConfig": {"InviteMessageTemplate": {
                "EmailSubject": "Welcome to Orkystra",
                "EmailMessage": "Hello {username}, your code is {####}",
            }},
        }),
    )
    .await;
    let pid = pool["UserPool"]["Id"].as_str().unwrap().to_string();
    assert!(pid.starts_with("us-east-1_") && pid.len() == 19, "{pid}");
    assert_eq!(
        pool["UserPool"]["Arn"],
        format!("arn:aws:cognito-idp:us-east-1:000000000000:userpool/{pid}")
    );
    let client = ok(
        &t,
        "CreateUserPoolClient",
        json!({"UserPoolId": pid, "ClientName": "app",
        "ExplicitAuthFlows": ["ALLOW_ADMIN_USER_PASSWORD_AUTH", "ALLOW_REFRESH_TOKEN_AUTH"]}),
    )
    .await;
    let cid = client["UserPoolClient"]["ClientId"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(cid.len(), 26);
    assert!(client["UserPoolClient"].get("ClientSecret").is_none());

    let (code, _) = fail(
        &t,
        "AdminCreateUser",
        json!({"UserPoolId": pid, "Username": "notanemail"}),
    )
    .await;
    assert_eq!(code, "InvalidParameterException");

    let r = ok(&t, "AdminCreateUser", json!({
        "UserPoolId": pid, "Username": "invitee@example.com",
        "UserAttributes": [{"Name": "email_verified", "Value": "true"}, {"Name": "name", "Value": "Invitee"}],
        "DesiredDeliveryMediums": ["EMAIL"],
    }))
    .await;
    let user = &r["User"];
    assert_eq!(user["UserStatus"], "FORCE_CHANGE_PASSWORD");
    assert_eq!(user["Enabled"], true);
    assert!(user["UserCreateDate"].is_number());
    let sub = user["Username"].as_str().unwrap().to_string();
    assert_eq!(sub.len(), 36);
    assert!(user["Attributes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["Name"] == "sub" && a["Value"] == sub));

    let (subject, body, to): (String, String, String) = t
        .state
        .db
        .call(|c| {
            c.query_row(
                "SELECT subject, text_body, to_addrs FROM mailbox_messages WHERE source = 'cognito'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
        })
        .await
        .unwrap();
    assert_eq!(subject, "Welcome to Orkystra");
    assert!(to.contains("invitee@example.com"));
    assert!(
        body.starts_with("Hello invitee@example.com, your code is "),
        "{body}"
    );
    let temp = body.trim().rsplit(' ').next().unwrap().to_string();

    let (code, _) = fail(
        &t,
        "AdminCreateUser",
        json!({"UserPoolId": pid, "Username": "INVITEE@example.com"}),
    )
    .await;
    assert_eq!(code, "UsernameExistsException");
    // Lookup by email or sub
    ok(
        &t,
        "AdminGetUser",
        json!({"UserPoolId": pid, "Username": "invitee@example.com"}),
    )
    .await;
    let g = ok(
        &t,
        "AdminGetUser",
        json!({"UserPoolId": pid, "Username": sub}),
    )
    .await;
    assert!(g["UserAttributes"].is_array());

    // USER_PASSWORD_AUTH not enabled on this client.
    let (code, _) = fail(
        &t,
        "InitiateAuth",
        json!({"ClientId": cid, "AuthFlow": "USER_PASSWORD_AUTH",
        "AuthParameters": {"USERNAME": "invitee@example.com", "PASSWORD": temp}}),
    )
    .await;
    assert_eq!(code, "InvalidParameterException");
    let (code, msg) = fail(
        &t,
        "AdminInitiateAuth",
        json!({"UserPoolId": pid, "ClientId": cid, "AuthFlow": "ADMIN_USER_PASSWORD_AUTH",
        "AuthParameters": {"USERNAME": "invitee@example.com", "PASSWORD": "wrong"}}),
    )
    .await;
    assert_eq!(
        (code.as_str(), msg.as_str()),
        ("NotAuthorizedException", "Incorrect username or password.")
    );

    let ch = ok(
        &t,
        "AdminInitiateAuth",
        json!({"UserPoolId": pid, "ClientId": cid, "AuthFlow": "ADMIN_USER_PASSWORD_AUTH",
        "AuthParameters": {"USERNAME": "invitee@example.com", "PASSWORD": temp}}),
    )
    .await;
    assert_eq!(ch["ChallengeName"], "NEW_PASSWORD_REQUIRED");
    let session = ch["Session"].as_str().unwrap().to_string();
    assert_eq!(ch["ChallengeParameters"]["USER_ID_FOR_SRP"], sub);

    let (code, _) = fail(
        &t,
        "AdminRespondToAuthChallenge",
        json!({"UserPoolId": pid, "ClientId": cid,
        "ChallengeName": "NEW_PASSWORD_REQUIRED", "Session": session,
        "ChallengeResponses": {"USERNAME": "invitee@example.com", "NEW_PASSWORD": "weak"}}),
    )
    .await;
    assert_eq!(code, "InvalidPasswordException");
    let res = ok(&t, "AdminRespondToAuthChallenge", json!({"UserPoolId": pid, "ClientId": cid,
        "ChallengeName": "NEW_PASSWORD_REQUIRED", "Session": session,
        "ChallengeResponses": {"USERNAME": "invitee@example.com", "NEW_PASSWORD": "N3wPassword!", "userAttributes.family_name": "Smith"}})).await;
    let ar = &res["AuthenticationResult"];
    assert!(ar["IdToken"].is_string() && ar["RefreshToken"].is_string());
    let ac = claims(ar["AccessToken"].as_str().unwrap());
    assert_eq!(ac["scope"], "aws.cognito.signin.user.admin");
    let me = ok(&t, "GetUser", json!({"AccessToken": ar["AccessToken"]})).await;
    assert_eq!(me["Username"], sub);
    assert!(me["UserAttributes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["Name"] == "family_name" && a["Value"] == "Smith"));
    let u = ok(
        &t,
        "AdminGetUser",
        json!({"UserPoolId": pid, "Username": sub}),
    )
    .await;
    assert_eq!(u["UserStatus"], "CONFIRMED");
    // Session is single use.
    let (code, _) = fail(&t, "RespondToAuthChallenge", json!({"ClientId": cid, "ChallengeName": "NEW_PASSWORD_REQUIRED",
        "Session": session, "ChallengeResponses": {"USERNAME": sub, "NEW_PASSWORD": "N3wPassword!"}})).await;
    assert_eq!(code, "NotAuthorizedException");

    // Refresh via REFRESH_TOKEN_AUTH
    let r = ok(
        &t,
        "AdminInitiateAuth",
        json!({"UserPoolId": pid, "ClientId": cid, "AuthFlow": "REFRESH_TOKEN_AUTH",
        "AuthParameters": {"REFRESH_TOKEN": ar["RefreshToken"]}}),
    )
    .await;
    assert!(r["AuthenticationResult"].get("RefreshToken").is_none());
    assert!(r["AuthenticationResult"]["IdToken"].is_string());

    // AdminSetUserPassword non-permanent → FORCE_CHANGE_PASSWORD
    ok(
        &t,
        "AdminSetUserPassword",
        json!({"UserPoolId": pid, "Username": sub, "Password": "Another1!pw", "Permanent": false}),
    )
    .await;
    let u = ok(
        &t,
        "AdminGetUser",
        json!({"UserPoolId": pid, "Username": sub}),
    )
    .await;
    assert_eq!(u["UserStatus"], "FORCE_CHANGE_PASSWORD");
    ok(
        &t,
        "AdminSetUserPassword",
        json!({"UserPoolId": pid, "Username": sub, "Password": "Another1!pw", "Permanent": true}),
    )
    .await;
    let u = ok(
        &t,
        "AdminGetUser",
        json!({"UserPoolId": pid, "Username": sub}),
    )
    .await;
    assert_eq!(u["UserStatus"], "CONFIRMED");
}

#[tokio::test]
async fn list_users_filters_and_disable_enable() {
    let t = TestApp::new().await;
    seed(&t, json!({"user_pools": [{"id": POOL, "name": "p", "clients": [{"client_id": CLIENT, "name": "web", "callback_urls": [CB]}],
        "users": [
            {"username": "alice", "email": "alice@example.com", "password": "Passw0rd!", "attributes": {"given_name": "Alice"}},
            {"username": "albert", "email": "albert@corp.example", "password": "Passw0rd!"},
            {"username": "bob", "email": "bob@example.com", "password": "Passw0rd!"},
        ]}]}))
    .await;
    let names = |v: &Value| -> Vec<String> {
        v["Users"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["Username"].as_str().unwrap().to_string())
            .collect()
    };
    let all = ok(&t, "ListUsers", json!({"UserPoolId": POOL})).await;
    assert_eq!(names(&all).len(), 3);
    let r = ok(
        &t,
        "ListUsers",
        json!({"UserPoolId": POOL, "Filter": "email = \"bob@example.com\""}),
    )
    .await;
    assert_eq!(names(&r), vec!["bob"]);
    let r = ok(
        &t,
        "ListUsers",
        json!({"UserPoolId": POOL, "Filter": "username ^= \"al\""}),
    )
    .await;
    let mut al = names(&r);
    al.sort();
    assert_eq!(al, vec!["albert", "alice"]);
    let r = ok(&t, "ListUsers", json!({"UserPoolId": POOL, "Filter": "given_name = \"Alice\"", "AttributesToGet": ["email"]})).await;
    assert_eq!(
        r["Users"][0]["Attributes"],
        json!([{"Name": "email", "Value": "alice@example.com"}])
    );
    let r = ok(&t, "ListUsers", json!({"UserPoolId": POOL, "Limit": 2})).await;
    assert_eq!(names(&r).len(), 2);
    let r2 = ok(
        &t,
        "ListUsers",
        json!({"UserPoolId": POOL, "Limit": 2, "PaginationToken": r["PaginationToken"]}),
    )
    .await;
    assert_eq!(names(&r2).len(), 1);
    let mut seen = [names(&r), names(&r2)].concat();
    seen.sort();
    assert_eq!(seen, vec!["albert", "alice", "bob"]);
    assert!(r2.get("PaginationToken").is_none());
    let (code, _) = fail(
        &t,
        "ListUsers",
        json!({"UserPoolId": POOL, "Filter": "nickname = \"x\""}),
    )
    .await;
    assert_eq!(code, "InvalidParameterException");

    let login = json!({"ClientId": CLIENT, "AuthFlow": "USER_PASSWORD_AUTH",
        "AuthParameters": {"USERNAME": "bob", "PASSWORD": "Passw0rd!"}});
    ok(&t, "InitiateAuth", login.clone()).await;
    ok(
        &t,
        "AdminDisableUser",
        json!({"UserPoolId": POOL, "Username": "bob"}),
    )
    .await;
    let r = ok(
        &t,
        "ListUsers",
        json!({"UserPoolId": POOL, "Filter": "status = \"Disabled\""}),
    )
    .await;
    assert_eq!(names(&r), vec!["bob"]);
    let (code, msg) = fail(&t, "InitiateAuth", login.clone()).await;
    assert_eq!(
        (code.as_str(), msg.as_str()),
        ("NotAuthorizedException", "User is disabled.")
    );
    ok(
        &t,
        "AdminEnableUser",
        json!({"UserPoolId": POOL, "Username": "bob"}),
    )
    .await;
    ok(&t, "InitiateAuth", login).await;

    ok(&t, "AdminUpdateUserAttributes", json!({"UserPoolId": POOL, "Username": "bob",
        "UserAttributes": [{"Name": "name", "Value": "Bobby"}, {"Name": "email", "Value": "bobby@example.com"}]})).await;
    let u = ok(
        &t,
        "AdminGetUser",
        json!({"UserPoolId": POOL, "Username": "bob"}),
    )
    .await;
    let attrs: HashMap<String, String> = u["UserAttributes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["Name"].as_str().unwrap().into(),
                a["Value"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(attrs["name"], "Bobby");
    assert_eq!(attrs["email_verified"], "false");
    ok(
        &t,
        "AdminDeleteUserAttributes",
        json!({"UserPoolId": POOL, "Username": "bob", "UserAttributeNames": ["name"]}),
    )
    .await;
    let u = ok(
        &t,
        "AdminGetUser",
        json!({"UserPoolId": POOL, "Username": "bob"}),
    )
    .await;
    assert!(!u["UserAttributes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["Name"] == "name"));
    ok(
        &t,
        "AdminDeleteUser",
        json!({"UserPoolId": POOL, "Username": "bob"}),
    )
    .await;
    let (code, _) = fail(
        &t,
        "AdminGetUser",
        json!({"UserPoolId": POOL, "Username": "bob"}),
    )
    .await;
    assert_eq!(code, "UserNotFoundException");
}

#[tokio::test]
async fn global_sign_out_invalidates_tokens() {
    let t = TestApp::new().await;
    seed(&t, standard_seed()).await;
    let login = json!({"ClientId": CLIENT, "AuthFlow": "USER_PASSWORD_AUTH",
        "AuthParameters": {"USERNAME": "dev@example.com", "PASSWORD": "Passw0rd!"}});
    let r = ok(&t, "InitiateAuth", login.clone()).await;
    let at = r["AuthenticationResult"]["AccessToken"]
        .as_str()
        .unwrap()
        .to_string();
    let rt = r["AuthenticationResult"]["RefreshToken"]
        .as_str()
        .unwrap()
        .to_string();
    ok(&t, "GetUser", json!({"AccessToken": at})).await;
    let ui = t
        .call(get(
            "/oauth2/userInfo",
            &[("authorization", &format!("Bearer {at}"))],
        ))
        .await;
    assert_eq!(ui.status, 200);

    ok(
        &t,
        "AdminUserGlobalSignOut",
        json!({"UserPoolId": POOL, "Username": "dev@example.com"}),
    )
    .await;
    let (code, msg) = fail(&t, "GetUser", json!({"AccessToken": at})).await;
    assert_eq!(
        (code.as_str(), msg.as_str()),
        ("NotAuthorizedException", "Access Token has been revoked")
    );
    let ui = t
        .call(get(
            "/oauth2/userInfo",
            &[("authorization", &format!("Bearer {at}"))],
        ))
        .await;
    assert_eq!(ui.status, 401);
    let (code, _) = fail(
        &t,
        "InitiateAuth",
        json!({"ClientId": CLIENT, "AuthFlow": "REFRESH_TOKEN_AUTH",
        "AuthParameters": {"REFRESH_TOKEN": rt}}),
    )
    .await;
    assert_eq!(code, "NotAuthorizedException");

    // A fresh sign-in works immediately, then GlobalSignOut via access token.
    let r = ok(&t, "InitiateAuth", login).await;
    let at2 = r["AuthenticationResult"]["AccessToken"]
        .as_str()
        .unwrap()
        .to_string();
    ok(&t, "GetUser", json!({"AccessToken": at2})).await;
    ok(&t, "GlobalSignOut", json!({"AccessToken": at2})).await;
    fail(&t, "GetUser", json!({"AccessToken": at2})).await;

    // RevokeToken API
    let r = ok(
        &t,
        "InitiateAuth",
        json!({"ClientId": CLIENT, "AuthFlow": "USER_PASSWORD_AUTH",
        "AuthParameters": {"USERNAME": "dev@example.com", "PASSWORD": "Passw0rd!"}}),
    )
    .await;
    let rt = r["AuthenticationResult"]["RefreshToken"]
        .as_str()
        .unwrap()
        .to_string();
    ok(&t, "RevokeToken", json!({"Token": rt, "ClientId": CLIENT})).await;
    fail(
        &t,
        "InitiateAuth",
        json!({"ClientId": CLIENT, "AuthFlow": "REFRESH_TOKEN_AUTH",
        "AuthParameters": {"REFRESH_TOKEN": rt}}),
    )
    .await;
}

#[tokio::test]
async fn seed_is_idempotent_and_key_survives_reset() {
    let t = TestApp::new().await;
    seed(&t, standard_seed()).await;
    let jwks1 = t
        .call(get(&format!("/{POOL}/.well-known/jwks.json"), &[]))
        .await
        .json();
    let sub1 = ok(
        &t,
        "AdminGetUser",
        json!({"UserPoolId": POOL, "Username": "dev@example.com"}),
    )
    .await["Username"]
        .clone();
    let mut doc = standard_seed();
    doc["user_pools"][0]["clients"][0]["callback_urls"] = json!([CB, "http://localhost:5173/cb"]);
    seed(&t, doc.clone()).await;
    seed(&t, doc).await;
    let pools = ok(&t, "ListUserPools", json!({"MaxResults": 10})).await;
    assert_eq!(pools["UserPools"].as_array().unwrap().len(), 1);
    let clients = ok(&t, "ListUserPoolClients", json!({"UserPoolId": POOL})).await;
    assert_eq!(
        clients["UserPoolClients"],
        json!([{"ClientId": CLIENT, "UserPoolId": POOL, "ClientName": "web"}])
    );
    let c = ok(
        &t,
        "DescribeUserPoolClient",
        json!({"UserPoolId": POOL, "ClientId": CLIENT}),
    )
    .await;
    assert_eq!(
        c["UserPoolClient"]["CallbackURLs"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        c["UserPoolClient"]["AllowedOAuthScopes"],
        json!(["openid", "email", "profile"])
    );
    let users = ok(&t, "ListUsers", json!({"UserPoolId": POOL})).await;
    assert_eq!(users["Users"].as_array().unwrap().len(), 1);
    assert_eq!(users["Users"][0]["Username"], sub1);
    let d = ok(
        &t,
        "DescribeUserPoolDomain",
        json!({"Domain": "orkystra-local"}),
    )
    .await;
    assert_eq!(d["DomainDescription"]["UserPoolId"], POOL);
    let (code, _) = fail(&t, "ListUserPools", json!({})).await;
    assert_eq!(code, "InvalidParameterException");

    let r = t
        .call(rest(
            "POST",
            "/_cloudtwin/reset",
            Some(json!({"services": ["cognito"]})),
        ))
        .await;
    assert_eq!(r.status, 200);
    let pools = ok(&t, "ListUserPools", json!({"MaxResults": 10})).await;
    assert_eq!(pools["UserPools"], json!([]));
    seed(&t, standard_seed()).await;
    let jwks2 = t
        .call(get(&format!("/{POOL}/.well-known/jwks.json"), &[]))
        .await
        .json();
    assert_eq!(jwks1, jwks2);
}

#[tokio::test]
async fn sign_up_confirm_and_forgot_password() {
    let t = TestApp::new().await;
    seed(&t, standard_seed()).await;
    let r = ok(
        &t,
        "SignUp",
        json!({"ClientId": CLIENT, "Username": "self@example.com", "Password": "Passw0rd!"}),
    )
    .await;
    assert_eq!(r["UserConfirmed"], false);
    assert_eq!(r["CodeDeliveryDetails"]["DeliveryMedium"], "EMAIL");
    let latest_code = |t: &TestApp| {
        let db = t.state.db.clone();
        async move {
            let body: String = db
                .call(|c| {
                    c.query_row(
                        "SELECT text_body FROM mailbox_messages ORDER BY id DESC LIMIT 1",
                        [],
                        |r| r.get(0),
                    )
                })
                .await
                .unwrap();
            body.trim()
                .trim_end_matches('.')
                .rsplit(' ')
                .next()
                .unwrap()
                .to_string()
        }
    };
    let code = latest_code(&t).await;
    assert_eq!(code.len(), 6);
    let login = json!({"ClientId": CLIENT, "AuthFlow": "USER_PASSWORD_AUTH",
        "AuthParameters": {"USERNAME": "self@example.com", "PASSWORD": "Passw0rd!"}});
    let (c, _) = fail(&t, "InitiateAuth", login.clone()).await;
    assert_eq!(c, "UserNotConfirmedException");
    let (c, _) = fail(
        &t,
        "ConfirmSignUp",
        json!({"ClientId": CLIENT, "Username": "self@example.com", "ConfirmationCode": "000000x"}),
    )
    .await;
    assert_eq!(c, "CodeMismatchException");
    ok(
        &t,
        "ConfirmSignUp",
        json!({"ClientId": CLIENT, "Username": "self@example.com", "ConfirmationCode": code}),
    )
    .await;
    ok(&t, "InitiateAuth", login).await;

    ok(
        &t,
        "AdminResetUserPassword",
        json!({"UserPoolId": POOL, "Username": "self@example.com"}),
    )
    .await;
    let (c, _) = fail(
        &t,
        "InitiateAuth",
        json!({"ClientId": CLIENT, "AuthFlow": "USER_PASSWORD_AUTH",
        "AuthParameters": {"USERNAME": "self@example.com", "PASSWORD": "Passw0rd!"}}),
    )
    .await;
    assert_eq!(c, "PasswordResetRequiredException");
    ok(
        &t,
        "ForgotPassword",
        json!({"ClientId": CLIENT, "Username": "self@example.com"}),
    )
    .await;
    let code = latest_code(&t).await;
    ok(
        &t,
        "ConfirmForgotPassword",
        json!({"ClientId": CLIENT, "Username": "self@example.com",
        "ConfirmationCode": code, "Password": "Different1!"}),
    )
    .await;
    ok(
        &t,
        "InitiateAuth",
        json!({"ClientId": CLIENT, "AuthFlow": "USER_PASSWORD_AUTH",
        "AuthParameters": {"USERNAME": "self@example.com", "PASSWORD": "Different1!"}}),
    )
    .await;
}

#[tokio::test]
async fn pool_and_client_crud() {
    let t = TestApp::new().await;
    let p = ok(
        &t,
        "CreateUserPool",
        json!({"PoolName": "crud", "AliasAttributes": ["email"]}),
    )
    .await;
    let pid = p["UserPool"]["Id"].as_str().unwrap().to_string();
    assert_eq!(
        p["UserPool"]["Policies"]["PasswordPolicy"]["MinimumLength"],
        8
    );
    ok(
        &t,
        "UpdateUserPool",
        json!({"UserPoolId": pid, "AutoVerifiedAttributes": ["email"]}),
    )
    .await;
    let d = ok(&t, "DescribeUserPool", json!({"UserPoolId": pid})).await;
    assert_eq!(d["UserPool"]["AutoVerifiedAttributes"], json!(["email"]));
    let c = ok(&t, "CreateUserPoolClient", json!({"UserPoolId": pid, "ClientName": "conf", "GenerateSecret": true,
        "AllowedOAuthFlowsUserPoolClient": true, "AllowedOAuthFlows": ["code"], "AllowedOAuthScopes": ["openid"],
        "CallbackURLs": [CB], "AccessTokenValidity": 5, "TokenValidityUnits": {"AccessToken": "minutes"}})).await;
    let uc = &c["UserPoolClient"];
    assert_eq!(uc["ClientSecret"].as_str().unwrap().len(), 51);
    let cid = uc["ClientId"].as_str().unwrap().to_string();
    let u = ok(&t, "UpdateUserPoolClient", json!({"UserPoolId": pid, "ClientId": cid, "ClientName": "renamed",
        "AllowedOAuthFlowsUserPoolClient": true, "AllowedOAuthFlows": ["code"], "AllowedOAuthScopes": ["openid", "email"],
        "CallbackURLs": [CB]})).await;
    assert_eq!(u["UserPoolClient"]["ClientName"], "renamed");
    assert_eq!(u["UserPoolClient"]["ClientSecret"], uc["ClientSecret"]);
    // Alias email login requires a verified email.
    ok(&t, "AdminCreateUser", json!({"UserPoolId": pid, "Username": "carol", "TemporaryPassword": "Passw0rd!",
        "MessageAction": "SUPPRESS", "UserAttributes": [{"Name": "email", "Value": "carol@example.com"}, {"Name": "email_verified", "Value": "true"}]})).await;
    let g = ok(
        &t,
        "AdminGetUser",
        json!({"UserPoolId": pid, "Username": "carol@example.com"}),
    )
    .await;
    assert_eq!(g["Username"], "carol");
    ok(
        &t,
        "CreateUserPoolDomain",
        json!({"UserPoolId": pid, "Domain": "crud-domain"}),
    )
    .await;
    let (code, _) = fail(
        &t,
        "CreateUserPoolDomain",
        json!({"UserPoolId": "us-east-1_zzz", "Domain": "crud-domain"}),
    )
    .await;
    assert_eq!(code, "ResourceNotFoundException");
    ok(
        &t,
        "DeleteUserPoolDomain",
        json!({"UserPoolId": pid, "Domain": "crud-domain"}),
    )
    .await;
    assert_eq!(
        ok(
            &t,
            "DescribeUserPoolDomain",
            json!({"Domain": "crud-domain"})
        )
        .await["DomainDescription"],
        json!({})
    );
    ok(
        &t,
        "DeleteUserPoolClient",
        json!({"UserPoolId": pid, "ClientId": cid}),
    )
    .await;
    let (code, _) = fail(
        &t,
        "DescribeUserPoolClient",
        json!({"UserPoolId": pid, "ClientId": cid}),
    )
    .await;
    assert_eq!(code, "ResourceNotFoundException");
    ok(&t, "DeleteUserPool", json!({"UserPoolId": pid})).await;
    let (code, _) = fail(&t, "DescribeUserPool", json!({"UserPoolId": pid})).await;
    assert_eq!(code, "ResourceNotFoundException");
    let (code, _) = fail(&t, "NoSuchOperation", json!({})).await;
    assert_eq!(code, "InvalidAction");
}

#[tokio::test]
async fn s3_routes_still_work_alongside_cognito() {
    let t = TestApp::new().await;
    seed(&t, standard_seed()).await;
    assert_eq!(t.call(rest("PUT", "/plain-bucket", None)).await.status, 200);
    let r = t
        .call(raw(
            "PUT",
            "/plain-bucket/dir/file.txt",
            &[("content-type", "text/plain")],
            "hello",
        ))
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
    let r = t.call(get("/plain-bucket/dir/file.txt", &[])).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.text(), "hello");
    let r = t.call(get("/plain-bucket", &[])).await;
    assert_eq!(r.status, 200);
    assert!(r.text().contains("dir/file.txt"));
    assert_eq!(t.call(get("/", &[])).await.status, 200);
    assert_eq!(
        t.call(raw("DELETE", "/plain-bucket/dir/file.txt", &[], Vec::new()))
            .await
            .status,
        204
    );
    // Cognito routes keep working afterwards.
    assert_eq!(
        t.call(get(&format!("/{POOL}/.well-known/jwks.json"), &[]))
            .await
            .status,
        200
    );
}

const APP: &str = "http://localhost:5173";

fn preflight(uri: &str, origin: &str, method: &str) -> axum::http::Request<axum::body::Body> {
    raw(
        "OPTIONS",
        uri,
        &[
            ("origin", origin),
            ("access-control-request-method", method),
            (
                "access-control-request-headers",
                "authorization,content-type",
            ),
        ],
        Vec::new(),
    )
}

#[tokio::test]
async fn browser_cors_on_token_userinfo_revoke_and_discovery() {
    let t = TestApp::new().await;
    seed(&t, standard_seed()).await;

    for (uri, method) in [
        ("/oauth2/token", "POST"),
        ("/oauth2/userInfo", "GET"),
        ("/oauth2/revoke", "POST"),
        (
            &format!("/{POOL}/.well-known/openid-configuration") as &str,
            "GET",
        ),
        (&format!("/{POOL}/.well-known/jwks.json") as &str, "GET"),
    ] {
        let r = t.call(preflight(uri, APP, method)).await;
        assert_eq!(r.status, 200, "preflight {uri}: {}", r.text());
        assert_eq!(
            r.header("access-control-allow-origin").as_deref(),
            Some(APP),
            "{uri}"
        );
        assert_eq!(
            r.header("access-control-allow-credentials").as_deref(),
            Some("true")
        );
        let methods = r.header("access-control-allow-methods").unwrap_or_default();
        assert!(methods.contains(method), "{uri}: {methods}");
        let headers = r.header("access-control-allow-headers").unwrap_or_default();
        assert!(
            headers.contains("authorization") && headers.contains("content-type"),
            "{headers}"
        );
    }

    // Discovery fetched from the page.
    let d = t
        .call(get(
            &format!("/{POOL}/.well-known/openid-configuration"),
            &[("origin", APP)],
        ))
        .await;
    assert_eq!(d.status, 200);
    assert_eq!(
        d.header("access-control-allow-origin").as_deref(),
        Some(APP)
    );

    // Code exchange from the browser, the case in the gap report.
    let (verifier, challenge) = pkce();
    let r = login_form(
        &t,
        &authorize_uri(&challenge),
        "dev@example.com",
        "Passw0rd!",
    )
    .await;
    let code = query_of(&r.header("location").unwrap())["code"].clone();
    let tok = t
        .call(form_req(
            "/oauth2/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", CLIENT),
                ("code", &code),
                ("redirect_uri", CB),
                ("code_verifier", &verifier),
            ],
            &[("origin", APP)],
        ))
        .await;
    assert_eq!(tok.status, 200, "{}", tok.text());
    assert_eq!(
        tok.header("access-control-allow-origin").as_deref(),
        Some(APP)
    );
    let body = tok.json();
    let access = body["access_token"].as_str().unwrap();
    let refresh = body["refresh_token"].as_str().unwrap();

    // OAuth errors must be readable by the page too.
    let bad = t
        .call(form_req(
            "/oauth2/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", CLIENT),
                ("code", "nope"),
            ],
            &[("origin", APP)],
        ))
        .await;
    assert_eq!(bad.status, 400);
    assert_eq!(
        bad.header("access-control-allow-origin").as_deref(),
        Some(APP)
    );

    let auth = format!("Bearer {access}");
    let ui = t
        .call(get(
            "/oauth2/userInfo",
            &[("origin", APP), ("authorization", &auth)],
        ))
        .await;
    assert_eq!(ui.status, 200, "{}", ui.text());
    assert_eq!(
        ui.header("access-control-allow-origin").as_deref(),
        Some(APP)
    );

    let rv = t
        .call(form_req(
            "/oauth2/revoke",
            &[("token", refresh), ("client_id", CLIENT)],
            &[("origin", APP)],
        ))
        .await;
    assert_eq!(rv.status, 200, "{}", rv.text());
    assert_eq!(
        rv.header("access-control-allow-origin").as_deref(),
        Some(APP)
    );

    // Hosted-UI pages are navigations and stay CORS-free.
    let page = t
        .call(get(&authorize_uri(&challenge), &[("origin", APP)]))
        .await;
    assert!(page.header("access-control-allow-origin").is_none());
}

#[tokio::test]
async fn cors_allowed_origins_restricts_browser_access() {
    let t = TestApp::with_config(|c| {
        c.cognito.cors_allowed_origins = vec![APP.to_string()];
    })
    .await;
    seed(&t, standard_seed()).await;

    let ok = t.call(preflight("/oauth2/token", APP, "POST")).await;
    assert_eq!(
        ok.header("access-control-allow-origin").as_deref(),
        Some(APP)
    );

    let evil = "http://evil.example";
    let r = t.call(preflight("/oauth2/token", evil, "POST")).await;
    assert!(r.header("access-control-allow-origin").is_none());
    let r = t
        .call(get(
            &format!("/{POOL}/.well-known/jwks.json"),
            &[("origin", evil)],
        ))
        .await;
    assert_eq!(r.status, 200);
    assert!(r.header("access-control-allow-origin").is_none());
}

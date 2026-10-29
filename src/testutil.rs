//! In-process test harness: build the real app on an in-memory database and
//! drive it with `tower::ServiceExt::oneshot`.

use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    http::{HeaderMap, Request},
    Router,
};
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::config::Config;
use crate::AppState;

pub struct TestApp {
    pub state: Arc<AppState>,
    pub router: Router,
}

pub struct TestResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl TestResponse {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("invalid JSON ({e}): {}", self.text()))
    }

    pub fn header(&self, name: &str) -> Option<String> {
        self.headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }
}

impl TestApp {
    pub async fn new() -> Self {
        Self::with_config(|_| {}).await
    }

    pub async fn with_config(f: impl FnOnce(&mut Config)) -> Self {
        let mut cfg = Config::test_defaults();
        f(&mut cfg);
        let state = AppState::new(cfg).await.expect("state");
        let router = crate::app::build_router(state.clone());
        TestApp { state, router }
    }

    pub async fn call(&self, req: Request<Body>) -> TestResponse {
        let resp = self.router.clone().oneshot(req).await.expect("response");
        let status = resp.status().as_u16();
        let headers = resp.headers().clone();
        let body = resp
            .into_body()
            .collect()
            .await
            .map(|c| c.to_bytes())
            .unwrap_or_default();
        TestResponse {
            status,
            headers,
            body,
        }
    }
}

/// AWS JSON-protocol request (`X-Amz-Target`).
pub fn aws_json(target: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/")
        .header("content-type", "application/x-amz-json-1.1")
        .header("x-amz-target", target)
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// AWS Query-protocol request (form-urlencoded).
pub fn aws_query(params: &[(&str, &str)]) -> Request<Body> {
    let body = serde_urlencoded::to_string(params).unwrap();
    Request::builder()
        .method("POST")
        .uri("/")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap()
}

/// JSON REST request (optionally with a body).
pub fn rest(method: &str, uri: &str, body: Option<serde_json::Value>) -> Request<Body> {
    let b = Request::builder().method(method).uri(uri);
    match body {
        Some(v) => b
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    }
}

/// Arbitrary request with headers and raw body bytes.
pub fn raw(
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    body: impl Into<Bytes>,
) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    b.body(Body::from(body.into())).unwrap()
}

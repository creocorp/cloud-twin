//! Pub/Sub tests driven through HTTP, shaped like the REST transport requests.

use serde_json::json;

use crate::testutil::{rest, TestApp, TestResponse};

const T: &str = "/v1/projects/proj/topics/t";
const S: &str = "/v1/projects/proj/subscriptions/s";

async fn setup(t: &TestApp) {
    assert_eq!(t.call(rest("PUT", T, Some(json!({})))).await.status, 200);
    let r = t
        .call(rest(
            "PUT",
            S,
            Some(
                json!({"name": "projects/proj/subscriptions/s", "topic": "projects/proj/topics/t"}),
            ),
        ))
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
}

async fn publish(t: &TestApp, data: &str) -> TestResponse {
    t.call(rest(
        "POST",
        &format!("{T}:publish"),
        Some(json!({"messages": [{"data": data, "attributes": {"k": "v"}}]})),
    ))
    .await
}

async fn pull(t: &TestApp) -> serde_json::Value {
    let r = t
        .call(rest(
            "POST",
            &format!("{S}:pull"),
            Some(json!({"maxMessages": 10})),
        ))
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
    r.json()
}

#[tokio::test]
async fn projects_paths_crud_and_listing() {
    let t = TestApp::new().await;
    setup(&t).await;
    assert_eq!(t.call(rest("PUT", T, Some(json!({})))).await.status, 409);

    let r = t.call(rest("GET", "/v1/projects/proj/topics", None)).await;
    assert_eq!(
        r.json()["topics"],
        json!([{"name": "projects/proj/topics/t"}])
    );
    let r = t
        .call(rest("GET", "/v1/projects/proj/subscriptions", None))
        .await;
    assert_eq!(
        r.json()["subscriptions"][0]["topic"],
        "projects/proj/topics/t"
    );
    assert_eq!(r.json()["subscriptions"][0]["ackDeadlineSeconds"], 10);
    let r = t
        .call(rest("GET", &format!("{T}/subscriptions"), None))
        .await;
    assert_eq!(
        r.json()["subscriptions"],
        json!(["projects/proj/subscriptions/s"])
    );

    // Projects are isolated.
    let r = t.call(rest("GET", "/v1/projects/other/topics", None)).await;
    assert_eq!(r.json()["topics"], json!([]));

    assert_eq!(t.call(rest("DELETE", S, None)).await.status, 200);
    assert_eq!(t.call(rest("DELETE", S, None)).await.status, 404);
    assert_eq!(t.call(rest("DELETE", T, None)).await.status, 200);
    let r = t.call(rest("GET", T, None)).await;
    assert_eq!(r.status, 404);
    assert_eq!(r.json()["error"]["status"], "NOT_FOUND");
}

#[tokio::test]
async fn publish_pull_ack_with_leases() {
    let t = TestApp::new().await;
    setup(&t).await;
    let r = publish(&t, "aGk=").await;
    assert_eq!(r.status, 200, "{}", r.text());
    let id = r.json()["messageIds"][0].as_str().unwrap().to_string();

    let v = pull(&t).await;
    let msg = &v["receivedMessages"][0];
    assert_eq!(msg["message"]["data"], "aGk=");
    assert_eq!(msg["message"]["messageId"], id);
    assert_eq!(msg["message"]["attributes"], json!({"k": "v"}));
    assert!(msg["message"]["publishTime"]
        .as_str()
        .unwrap()
        .ends_with('Z'));
    let ack_id = msg["ackId"].as_str().unwrap().to_string();

    // Leased: not redelivered until the deadline passes or is reset.
    assert_eq!(pull(&t).await["receivedMessages"], json!([]));
    let r = t
        .call(rest(
            "POST",
            &format!("{S}:modifyAckDeadline"),
            Some(json!({"ackIds": [ack_id], "ackDeadlineSeconds": 0})),
        ))
        .await;
    assert_eq!(r.status, 200);
    let v = pull(&t).await;
    assert_eq!(v["receivedMessages"].as_array().unwrap().len(), 1);

    let r = t
        .call(rest(
            "POST",
            &format!("{S}:acknowledge"),
            Some(json!({"ackIds": [ack_id]})),
        ))
        .await;
    assert_eq!(r.status, 200);
    t.call(rest(
        "POST",
        &format!("{S}:modifyAckDeadline"),
        Some(json!({"ackIds": [ack_id], "ackDeadlineSeconds": 0})),
    ))
    .await;
    assert_eq!(pull(&t).await["receivedMessages"], json!([]));
}

#[tokio::test]
async fn publish_and_subscribe_errors() {
    let t = TestApp::new().await;
    let r = t
        .call(rest(
            "POST",
            "/v1/projects/proj/topics/nope:publish",
            Some(json!({"messages": [{"data": "eA=="}]})),
        ))
        .await;
    assert_eq!(r.status, 404);
    let r = t
        .call(rest(
            "PUT",
            S,
            Some(json!({"topic": "projects/proj/topics/nope"})),
        ))
        .await;
    assert_eq!(r.status, 404);
    setup(&t).await;
    let r = t
        .call(rest(
            "POST",
            &format!("{T}:publish"),
            Some(json!({"messages": [{}]})),
        ))
        .await;
    assert_eq!(r.status, 400);
    let r = t
        .call(rest(
            "POST",
            "/v1/projects/proj/subscriptions/nope:pull",
            Some(json!({})),
        ))
        .await;
    assert_eq!(r.status, 404);
    let r = t
        .call(rest("POST", &format!("{S}:bogus"), Some(json!({}))))
        .await;
    assert_eq!(r.status, 404);
}

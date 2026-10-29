//! Service Bus JSON API tests (root `/{namespace}/queues|topics` alias).

use serde_json::Value;

use crate::testutil::{raw, TestApp, TestResponse};

async fn call(t: &TestApp, method: &str, path: &str, body: &[u8]) -> TestResponse {
    let uri = format!("/{}{path}", t.state.cfg.azure_namespace);
    t.call(raw(method, &uri, &[], body.to_vec())).await
}

fn messages(r: &TestResponse) -> Vec<Value> {
    r.json()["messages"].as_array().cloned().unwrap()
}

#[tokio::test]
async fn queue_crud() {
    let t = TestApp::new().await;
    let r = call(&t, "PUT", "/queues/q1", b"").await;
    assert_eq!(r.status, 201);
    assert_eq!(r.json()["name"], "q1");
    let r = call(&t, "GET", "/queues", b"").await;
    assert_eq!(r.json()["queues"][0]["name"], "q1");
    assert_eq!(call(&t, "GET", "/queues/q1", b"").await.status, 200);
    assert_eq!(call(&t, "DELETE", "/queues/q1", b"").await.status, 204);
    assert_eq!(call(&t, "GET", "/queues/q1", b"").await.status, 404);
    assert_eq!(call(&t, "DELETE", "/queues/q1", b"").await.status, 404);
    assert_eq!(
        call(&t, "POST", "/queues/q1/messages", b"x").await.status,
        404
    );
}

#[tokio::test]
async fn send_receive_complete_abandon_deadletter() {
    let t = TestApp::new().await;
    call(&t, "PUT", "/queues/q", b"").await;
    for i in 0..5 {
        let r = call(&t, "POST", "/queues/q/messages", format!("m{i}").as_bytes()).await;
        assert_eq!(r.status, 201);
        assert_eq!(r.json()["state"], "active");
    }
    let got = messages(&call(&t, "GET", "/queues/q/messages", b"").await);
    assert_eq!(got.len(), 1, "default limit is 1");
    assert_eq!(got[0]["body"], "m0");
    assert_eq!(got[0]["state"], "locked");
    let lock0 = got[0]["lock_token"].as_str().unwrap().to_string();

    let got = messages(&call(&t, "GET", "/queues/q/messages?limit=3", b"").await);
    assert_eq!(got.len(), 3);
    assert_eq!(got[0]["body"], "m1");

    // Abandon makes m0 receivable again; complete removes it.
    let r = call(
        &t,
        "POST",
        &format!("/queues/q/messages/{lock0}/abandon"),
        b"",
    )
    .await;
    assert_eq!(r.status, 200);
    let got = messages(&call(&t, "GET", "/queues/q/messages?limit=10", b"").await);
    let bodies: Vec<_> = got.iter().map(|m| m["body"].as_str().unwrap()).collect();
    assert_eq!(bodies, vec!["m0", "m4"]);
    assert_eq!(
        call(&t, "DELETE", &format!("/queues/q/messages/{lock0}"), b"")
            .await
            .status,
        204
    );
    assert_eq!(
        call(&t, "DELETE", &format!("/queues/q/messages/{lock0}"), b"")
            .await
            .status,
        404
    );

    let lock4 = got[1]["lock_token"].as_str().unwrap();
    let r = call(
        &t,
        "POST",
        &format!("/queues/q/messages/{lock4}/deadletter"),
        b"",
    )
    .await;
    assert_eq!(r.status, 200);
    assert!(messages(&call(&t, "GET", "/queues/q/messages?limit=10", b"").await).is_empty());
    // Only locked messages can be settled this way.
    let r = call(
        &t,
        "POST",
        &format!("/queues/q/messages/{lock4}/abandon"),
        b"",
    )
    .await;
    assert_eq!(r.status, 404);
    assert_eq!(
        call(
            &t,
            "POST",
            &format!("/queues/q/messages/{lock4}/bogus"),
            b""
        )
        .await
        .status,
        404
    );
    assert_eq!(
        call(&t, "GET", "/queues/q/messages?limit=0", b"")
            .await
            .status,
        400
    );
}

#[tokio::test]
async fn topic_fanout_and_subscription_settle() {
    let t = TestApp::new().await;
    assert_eq!(
        call(&t, "PUT", "/topics/t/subscriptions/s1", b"")
            .await
            .status,
        404
    );
    assert_eq!(call(&t, "PUT", "/topics/t", b"").await.status, 201);
    call(&t, "PUT", "/topics/t/subscriptions/s1", b"").await;
    call(&t, "PUT", "/topics/t/subscriptions/s2", b"").await;
    let r = call(&t, "GET", "/topics/t/subscriptions", b"").await;
    assert_eq!(r.json()["subscriptions"].as_array().unwrap().len(), 2);

    let r = call(&t, "POST", "/topics/t/messages", b"hi").await;
    assert_eq!(r.status, 201);
    assert_eq!(r.json()["fan_out"], 2);

    let m1 = messages(&call(&t, "GET", "/topics/t/subscriptions/s1/messages", b"").await);
    let m2 = messages(&call(&t, "GET", "/topics/t/subscriptions/s2/messages", b"").await);
    assert_eq!(m1[0]["body"], "hi");
    assert_eq!(m2[0]["body"], "hi");
    assert_eq!(m1[0]["message_id"], m2[0]["message_id"]);

    let lock = m1[0]["lock_token"].as_str().unwrap();
    let r = call(
        &t,
        "DELETE",
        &format!("/topics/t/subscriptions/s1/messages/{lock}"),
        b"",
    )
    .await;
    assert_eq!(r.status, 204);
    assert!(
        messages(&call(&t, "GET", "/topics/t/subscriptions/s1/messages", b"").await).is_empty()
    );

    assert_eq!(call(&t, "DELETE", "/topics/t", b"").await.status, 204);
    assert_eq!(
        call(&t, "GET", "/topics/t/subscriptions/s2/messages", b"")
            .await
            .status,
        404
    );
    let left: i64 = t
        .state
        .db
        .call(|c| c.query_row("SELECT COUNT(*) FROM asb_messages", [], |r| r.get(0)))
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn other_namespace_is_404() {
    let t = TestApp::new().await;
    let r = t
        .call(raw("PUT", "/azure/not-my-ns/queues/q", &[], Vec::new()))
        .await;
    assert_eq!(r.status, 404);
}

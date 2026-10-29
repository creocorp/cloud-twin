use std::time::Duration;

use axum::{body::Bytes, http::HeaderMap, routing::post, Router};
use serde_json::{json, Value};

use crate::testutil::{aws_json, aws_query, TestApp, TestResponse};
use crate::xml::Element;

async fn sns(t: &TestApp, params: &[(&str, &str)]) -> TestResponse {
    t.call(aws_query(params)).await
}

/// Call an action and return its `<ActionResult>` element.
async fn ok(t: &TestApp, params: &[(&str, &str)]) -> Element {
    let r = sns(t, params).await;
    assert_eq!(r.status, 200, "{params:?}: {}", r.text());
    let root = crate::xml::parse(&r.body).unwrap();
    let action = params.iter().find(|(k, _)| *k == "Action").unwrap().1;
    root.child(&format!("{action}Result"))
        .cloned()
        .unwrap_or_default()
}

fn error_code(r: &TestResponse) -> String {
    crate::xml::parse(&r.body)
        .unwrap()
        .path(&["Error", "Code"])
        .unwrap()
        .text
        .clone()
}

async fn create_topic(t: &TestApp, name: &str) -> String {
    ok(t, &[("Action", "CreateTopic"), ("Name", name)])
        .await
        .child_text("TopicArn")
        .unwrap()
}

async fn create_queue(t: &TestApp, name: &str) -> String {
    let r = t
        .call(aws_json(
            "AmazonSQS.CreateQueue",
            json!({"QueueName": name}),
        ))
        .await;
    r.json()["QueueUrl"].as_str().unwrap().to_string()
}

fn queue_arn(name: &str) -> String {
    format!("arn:aws:sqs:us-east-1:000000000000:{name}")
}

async fn drain(t: &TestApp, url: &str) -> Vec<Value> {
    let r = t
        .call(aws_json(
            "AmazonSQS.ReceiveMessage",
            json!({"QueueUrl": url, "MaxNumberOfMessages": 10, "MessageAttributeNames": ["All"]}),
        ))
        .await;
    r.json()["Messages"].as_array().cloned().unwrap_or_default()
}

async fn subscribe(
    t: &TestApp,
    topic: &str,
    protocol: &str,
    endpoint: &str,
    attrs: &[(&str, &str)],
) -> String {
    let mut params = vec![
        ("Action".to_string(), "Subscribe".to_string()),
        ("TopicArn".into(), topic.into()),
        ("Protocol".into(), protocol.into()),
        ("Endpoint".into(), endpoint.into()),
        ("ReturnSubscriptionArn".into(), "true".into()),
    ];
    for (i, (k, v)) in attrs.iter().enumerate() {
        params.push((format!("Attributes.entry.{}.key", i + 1), k.to_string()));
        params.push((format!("Attributes.entry.{}.value", i + 1), v.to_string()));
    }
    let refs: Vec<(&str, &str)> = params
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    ok(t, &refs).await.child_text("SubscriptionArn").unwrap()
}

fn attr_entries(el: &Element) -> std::collections::BTreeMap<String, String> {
    el.child("Attributes")
        .map(|a| {
            a.children_named("entry")
                .map(|e| {
                    (
                        e.child_text("key").unwrap(),
                        e.child("value").map(|v| v.text.clone()).unwrap_or_default(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn sqs_delivery_uses_notification_envelope() {
    let t = TestApp::new().await;
    let topic = create_topic(&t, "orders").await;
    let url = create_queue(&t, "orders-q").await;
    subscribe(&t, &topic, "sqs", &queue_arn("orders-q"), &[]).await;
    let mid = ok(
        &t,
        &[
            ("Action", "Publish"),
            ("TopicArn", &topic),
            ("Message", "hello"),
            ("Subject", "Greeting"),
            ("MessageAttributes.entry.1.Name", "color"),
            ("MessageAttributes.entry.1.Value.DataType", "String"),
            ("MessageAttributes.entry.1.Value.StringValue", "red"),
        ],
    )
    .await
    .child_text("MessageId")
    .unwrap();

    let msgs = drain(&t, &url).await;
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].get("MessageAttributes").is_none());
    let env: Value = serde_json::from_str(msgs[0]["Body"].as_str().unwrap()).unwrap();
    assert_eq!(env["Type"], "Notification");
    assert_eq!(env["MessageId"], mid);
    assert_eq!(env["TopicArn"], topic);
    assert_eq!(env["Subject"], "Greeting");
    assert_eq!(env["Message"], "hello");
    assert_eq!(env["SignatureVersion"], "1");
    assert!(env["Signature"].is_string() && env["SigningCertURL"].is_string());
    assert!(env["UnsubscribeURL"]
        .as_str()
        .unwrap()
        .contains("Action=Unsubscribe&SubscriptionArn=arn:aws:sns"));
    let ts = env["Timestamp"].as_str().unwrap();
    assert!(
        chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.3fZ").is_ok(),
        "{ts}"
    );
    assert_eq!(
        env["MessageAttributes"],
        json!({"color": {"Type": "String", "Value": "red"}})
    );

    // No subject / attributes → keys absent.
    ok(
        &t,
        &[
            ("Action", "Publish"),
            ("TargetArn", &topic),
            ("Message", "plain"),
        ],
    )
    .await;
    let env: Value =
        serde_json::from_str(drain(&t, &url).await[0]["Body"].as_str().unwrap()).unwrap();
    assert!(env.get("Subject").is_none() && env.get("MessageAttributes").is_none());
}

#[tokio::test]
async fn raw_delivery_and_filter_policies() {
    let t = TestApp::new().await;
    let topic = create_topic(&t, "events").await;
    let raw_url = create_queue(&t, "raw").await;
    let red_url = create_queue(&t, "red").await;
    let big_url = create_queue(&t, "big").await;
    let body_url = create_queue(&t, "body").await;
    subscribe(
        &t,
        &topic,
        "sqs",
        &queue_arn("raw"),
        &[("RawMessageDelivery", "true")],
    )
    .await;
    subscribe(
        &t,
        &topic,
        "sqs",
        &raw_url.replace("raw", "red"),
        &[("FilterPolicy", r#"{"color": ["red", {"prefix": "crim"}]}"#)],
    )
    .await;
    subscribe(
        &t,
        &topic,
        "sqs",
        &queue_arn("big"),
        &[(
            "FilterPolicy",
            r#"{"price": [{"numeric": [">=", 100]}], "color": [{"anything-but": "blue"}]}"#,
        )],
    )
    .await;
    subscribe(
        &t,
        &topic,
        "sqs",
        &queue_arn("body"),
        &[
            ("FilterPolicy", r#"{"order": {"status": ["shipped"]}}"#),
            ("FilterPolicyScope", "MessageBody"),
        ],
    )
    .await;

    let publish = |color: &'static str, price: &'static str, body: &'static str| {
        let topic = topic.clone();
        let t = &t;
        async move {
            ok(
                t,
                &[
                    ("Action", "Publish"),
                    ("TopicArn", &topic),
                    ("Message", body),
                    ("MessageAttributes.entry.1.Name", "color"),
                    ("MessageAttributes.entry.1.Value.DataType", "String"),
                    ("MessageAttributes.entry.1.Value.StringValue", color),
                    ("MessageAttributes.entry.2.Name", "price"),
                    ("MessageAttributes.entry.2.Value.DataType", "Number"),
                    ("MessageAttributes.entry.2.Value.StringValue", price),
                ],
            )
            .await;
        }
    };
    publish("red", "5", r#"{"order": {"status": "shipped"}}"#).await;
    publish("blue", "500", "not json").await;
    publish("crimson", "150", r#"{"order": {"status": "pending"}}"#).await;

    let raw = drain(&t, &raw_url).await;
    assert_eq!(raw.len(), 3);
    assert_eq!(raw[0]["Body"], r#"{"order": {"status": "shipped"}}"#);
    assert_eq!(
        raw[0]["MessageAttributes"]["color"],
        json!({"DataType": "String", "StringValue": "red"})
    );
    assert_eq!(raw[0]["MessageAttributes"]["price"]["DataType"], "Number");
    assert!(raw[0]["MD5OfMessageAttributes"].is_string());

    let red: Vec<String> = drain(&t, &red_url)
        .await
        .iter()
        .map(|m| {
            serde_json::from_str::<Value>(m["Body"].as_str().unwrap()).unwrap()["MessageAttributes"]
                ["color"]["Value"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(red, vec!["red", "crimson"]);
    let big = drain(&t, &big_url).await;
    assert_eq!(big.len(), 1);
    let body = drain(&t, &body_url).await;
    assert_eq!(body.len(), 1);
    assert!(body[0]["Body"].as_str().unwrap().contains("shipped"));
}

#[tokio::test]
async fn http_delivery_posts_envelope() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(HeaderMap, Bytes)>();
    let app = Router::new().route(
        "/hook",
        post(move |headers: HeaderMap, body: Bytes| {
            let tx = tx.clone();
            async move {
                let _ = tx.send((headers, body));
                "ok"
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let t = TestApp::new().await;
    let topic = create_topic(&t, "hooks").await;
    let endpoint = format!("http://{addr}/hook");
    let sub = subscribe(&t, &topic, "http", &endpoint, &[]).await;
    let mid = ok(
        &t,
        &[
            ("Action", "Publish"),
            ("TopicArn", &topic),
            ("Message", "ping"),
        ],
    )
    .await
    .child_text("MessageId")
    .unwrap();
    let (headers, body) = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(headers["x-amz-sns-message-type"], "Notification");
    assert_eq!(headers["x-amz-sns-message-id"], mid.as_str());
    assert_eq!(headers["x-amz-sns-topic-arn"], topic.as_str());
    assert_eq!(headers["x-amz-sns-subscription-arn"], sub.as_str());
    assert!(headers["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/plain"));
    let env: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(env["Message"], "ping");
    assert_eq!(env["Type"], "Notification");

    ok(
        &t,
        &[
            ("Action", "SetSubscriptionAttributes"),
            ("SubscriptionArn", &sub),
            ("AttributeName", "RawMessageDelivery"),
            ("AttributeValue", "true"),
        ],
    )
    .await;
    ok(
        &t,
        &[
            ("Action", "Publish"),
            ("TopicArn", &topic),
            ("Message", "raw-ping"),
        ],
    )
    .await;
    let (_, body) = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&body[..], b"raw-ping");
}

#[tokio::test]
async fn email_delivery_lands_in_mailbox() {
    let t = TestApp::new().await;
    let topic = create_topic(&t, "alerts").await;
    subscribe(&t, &topic, "email", "ops@example.com", &[]).await;
    subscribe(&t, &topic, "email-json", "json@example.com", &[]).await;
    ok(
        &t,
        &[
            ("Action", "Publish"),
            ("TopicArn", &topic),
            ("Message", "disk full"),
            ("Subject", "Alarm"),
        ],
    )
    .await;
    ok(
        &t,
        &[
            ("Action", "Publish"),
            ("TopicArn", &topic),
            ("Message", "no subject"),
        ],
    )
    .await;
    let rows: Vec<(String, String, String, String)> = t
        .state
        .db
        .call(|c| {
            let mut s = c.prepare("SELECT from_addr, to_addrs, subject, COALESCE(text_body, '') FROM mailbox_messages ORDER BY id")?;
            let rows = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].0, "no-reply@sns.amazonaws.com");
    let ops: Vec<_> = rows
        .iter()
        .filter(|r| r.1.contains("ops@example.com"))
        .collect();
    assert_eq!(ops[0].2, "Alarm");
    assert!(ops[0].3.contains("disk full"));
    assert_eq!(ops[1].2, "AWS Notification Message");
    let json_mail = rows
        .iter()
        .find(|r| r.1.contains("json@example.com"))
        .unwrap();
    let env: Value = serde_json::from_str(json_mail.3.trim()).unwrap();
    assert_eq!(env["Message"], "disk full");
}

#[tokio::test]
async fn topic_and_subscription_management() {
    let t = TestApp::new().await;
    let r = sns(
        &t,
        &[
            ("Action", "Publish"),
            ("TopicArn", "arn:aws:sns:us-east-1:000000000000:ghost"),
            ("Message", "x"),
        ],
    )
    .await;
    assert_eq!(r.status, 404);
    assert_eq!(error_code(&r), "NotFound");

    let topic = ok(
        &t,
        &[
            ("Action", "CreateTopic"),
            ("Name", "mgmt"),
            ("Attributes.entry.1.key", "DisplayName"),
            ("Attributes.entry.1.value", "Mgmt"),
            ("Tags.member.1.Key", "env"),
            ("Tags.member.1.Value", "dev"),
        ],
    )
    .await
    .child_text("TopicArn")
    .unwrap();
    assert_eq!(create_topic(&t, "mgmt").await, topic);

    let s1 = subscribe(&t, &topic, "email", "a@b.com", &[]).await;
    assert_eq!(subscribe(&t, &topic, "email", "a@b.com", &[]).await, s1);
    let s2 = subscribe(
        &t,
        &topic,
        "sqs",
        &queue_arn("whatever"),
        &[("RawMessageDelivery", "true")],
    )
    .await;
    let r = sns(
        &t,
        &[
            ("Action", "Subscribe"),
            ("TopicArn", &topic),
            ("Protocol", "carrier-pigeon"),
            ("Endpoint", "x"),
        ],
    )
    .await;
    assert_eq!(error_code(&r), "InvalidParameter");

    let attrs = attr_entries(
        &ok(
            &t,
            &[("Action", "GetTopicAttributes"), ("TopicArn", &topic)],
        )
        .await,
    );
    assert_eq!(attrs["DisplayName"], "Mgmt");
    assert_eq!(attrs["SubscriptionsConfirmed"], "2");
    assert_eq!(attrs["SubscriptionsPending"], "0");
    assert_eq!(attrs["Owner"], "000000000000");
    assert!(attrs["Policy"].contains("__default_policy_ID"));
    assert!(attrs.contains_key("EffectiveDeliveryPolicy"));
    ok(
        &t,
        &[
            ("Action", "SetTopicAttributes"),
            ("TopicArn", &topic),
            ("AttributeName", "DisplayName"),
            ("AttributeValue", "New"),
        ],
    )
    .await;
    let attrs = attr_entries(
        &ok(
            &t,
            &[("Action", "GetTopicAttributes"), ("TopicArn", &topic)],
        )
        .await,
    );
    assert_eq!(attrs["DisplayName"], "New");

    let sattrs = attr_entries(
        &ok(
            &t,
            &[
                ("Action", "GetSubscriptionAttributes"),
                ("SubscriptionArn", &s2),
            ],
        )
        .await,
    );
    assert_eq!(sattrs["RawMessageDelivery"], "true");
    assert_eq!(sattrs["PendingConfirmation"], "false");
    assert_eq!(sattrs["ConfirmationWasAuthenticated"], "true");
    assert_eq!(sattrs["Protocol"], "sqs");
    ok(
        &t,
        &[
            ("Action", "SetSubscriptionAttributes"),
            ("SubscriptionArn", &s2),
            ("AttributeName", "FilterPolicy"),
            ("AttributeValue", r#"{"a": ["b"]}"#),
        ],
    )
    .await;
    let sattrs = attr_entries(
        &ok(
            &t,
            &[
                ("Action", "GetSubscriptionAttributes"),
                ("SubscriptionArn", &s2),
            ],
        )
        .await,
    );
    assert_eq!(sattrs["FilterPolicyScope"], "MessageAttributes");
    let r = sns(
        &t,
        &[
            ("Action", "SetSubscriptionAttributes"),
            ("SubscriptionArn", &s2),
            ("AttributeName", "FilterPolicy"),
            ("AttributeValue", "not json"),
        ],
    )
    .await;
    assert_eq!(error_code(&r), "InvalidParameter");

    let list = ok(&t, &[("Action", "ListSubscriptions")]).await;
    assert_eq!(
        list.child("Subscriptions")
            .unwrap()
            .children_named("member")
            .count(),
        2
    );
    ok(&t, &[("Action", "Unsubscribe"), ("SubscriptionArn", &s1)]).await;
    let list = ok(
        &t,
        &[("Action", "ListSubscriptionsByTopic"), ("TopicArn", &topic)],
    )
    .await;
    assert_eq!(
        list.child("Subscriptions")
            .unwrap()
            .children_named("member")
            .count(),
        1
    );
    let r = sns(
        &t,
        &[
            ("Action", "GetSubscriptionAttributes"),
            ("SubscriptionArn", &s1),
        ],
    )
    .await;
    assert_eq!(r.status, 404);

    ok(
        &t,
        &[
            ("Action", "TagResource"),
            ("ResourceArn", &topic),
            ("Tags.member.1.Key", "team"),
            ("Tags.member.1.Value", "x"),
        ],
    )
    .await;
    ok(
        &t,
        &[
            ("Action", "UntagResource"),
            ("ResourceArn", &topic),
            ("TagKeys.member.1", "env"),
        ],
    )
    .await;
    let tags = ok(
        &t,
        &[("Action", "ListTagsForResource"), ("ResourceArn", &topic)],
    )
    .await;
    let members: Vec<_> = tags
        .child("Tags")
        .unwrap()
        .children_named("member")
        .collect();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].child_text("Key").unwrap(), "team");

    ok(&t, &[("Action", "DeleteTopic"), ("TopicArn", &topic)]).await;
    let list = ok(&t, &[("Action", "ListSubscriptions")]).await;
    assert_eq!(
        list.child("Subscriptions")
            .unwrap()
            .children_named("member")
            .count(),
        0
    );
    ok(&t, &[("Action", "DeleteTopic"), ("TopicArn", &topic)]).await;
}

#[tokio::test]
async fn message_structure_and_publish_batch() {
    let t = TestApp::new().await;
    let topic = create_topic(&t, "structured").await;
    let url = create_queue(&t, "structured-q").await;
    subscribe(
        &t,
        &topic,
        "sqs",
        &queue_arn("structured-q"),
        &[("RawMessageDelivery", "true")],
    )
    .await;

    let msg = json!({"default": "fallback", "sqs": "for-sqs"}).to_string();
    ok(
        &t,
        &[
            ("Action", "Publish"),
            ("TopicArn", &topic),
            ("Message", &msg),
            ("MessageStructure", "json"),
        ],
    )
    .await;
    assert_eq!(drain(&t, &url).await[0]["Body"], "for-sqs");
    let r = sns(
        &t,
        &[
            ("Action", "Publish"),
            ("TopicArn", &topic),
            ("Message", r#"{"sqs": "x"}"#),
            ("MessageStructure", "json"),
        ],
    )
    .await;
    assert_eq!(error_code(&r), "InvalidParameter");

    let res = ok(
        &t,
        &[
            ("Action", "PublishBatch"),
            ("TopicArn", &topic),
            ("PublishBatchRequestEntries.member.1.Id", "one"),
            ("PublishBatchRequestEntries.member.1.Message", "m1"),
            (
                "PublishBatchRequestEntries.member.1.MessageAttributes.entry.1.Name",
                "k",
            ),
            (
                "PublishBatchRequestEntries.member.1.MessageAttributes.entry.1.Value.DataType",
                "String",
            ),
            (
                "PublishBatchRequestEntries.member.1.MessageAttributes.entry.1.Value.StringValue",
                "v",
            ),
            ("PublishBatchRequestEntries.member.2.Id", "two"),
            ("PublishBatchRequestEntries.member.2.Message", ""),
        ],
    )
    .await;
    assert_eq!(
        res.child("Successful")
            .unwrap()
            .children_named("member")
            .count(),
        1
    );
    let failed: Vec<_> = res
        .child("Failed")
        .unwrap()
        .children_named("member")
        .collect();
    assert_eq!(failed[0].child_text("Id").unwrap(), "two");
    assert_eq!(failed[0].child_text("SenderFault").unwrap(), "true");
    let msgs = drain(&t, &url).await;
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["MessageAttributes"]["k"]["StringValue"], "v");

    let r = sns(
        &t,
        &[
            ("Action", "PublishBatch"),
            ("TopicArn", &topic),
            ("PublishBatchRequestEntries.member.1.Id", "a"),
            ("PublishBatchRequestEntries.member.1.Message", "x"),
            ("PublishBatchRequestEntries.member.2.Id", "a"),
            ("PublishBatchRequestEntries.member.2.Message", "y"),
        ],
    )
    .await;
    assert_eq!(error_code(&r), "BatchEntryIdsNotDistinct");
}

#[tokio::test]
async fn fifo_topic_to_fifo_queue() {
    let t = TestApp::new().await;
    let topic = ok(
        &t,
        &[
            ("Action", "CreateTopic"),
            ("Name", "o.fifo"),
            ("Attributes.entry.1.key", "FifoTopic"),
            ("Attributes.entry.1.value", "true"),
            ("Attributes.entry.2.key", "ContentBasedDeduplication"),
            ("Attributes.entry.2.value", "true"),
        ],
    )
    .await
    .child_text("TopicArn")
    .unwrap();
    let r = t
        .call(aws_json(
            "AmazonSQS.CreateQueue",
            json!({"QueueName": "q.fifo", "Attributes": {"FifoQueue": "true"}}),
        ))
        .await;
    let url = r.json()["QueueUrl"].as_str().unwrap().to_string();
    subscribe(
        &t,
        &topic,
        "sqs",
        &queue_arn("q.fifo"),
        &[("RawMessageDelivery", "true")],
    )
    .await;
    let r = sns(
        &t,
        &[
            ("Action", "Publish"),
            ("TopicArn", &topic),
            ("Message", "x"),
        ],
    )
    .await;
    assert_eq!(error_code(&r), "InvalidParameter");
    let res = ok(
        &t,
        &[
            ("Action", "Publish"),
            ("TopicArn", &topic),
            ("Message", "x"),
            ("MessageGroupId", "g"),
        ],
    )
    .await;
    assert!(res.child_text("SequenceNumber").is_some());
    ok(
        &t,
        &[
            ("Action", "Publish"),
            ("TopicArn", &topic),
            ("Message", "x"),
            ("MessageGroupId", "g"),
        ],
    )
    .await;
    // Content-based dedup on the topic carries through to the FIFO queue.
    assert_eq!(drain(&t, &url).await.len(), 1);
}

#[tokio::test]
async fn publish_internal_and_seed() {
    let t = TestApp::new().await;
    crate::aws::sqs::seed(&t.state, &json!({"queues": ["inbox", "filtered"]}))
        .await
        .unwrap();
    let doc = json!({"topics": [
        "bare",
        {"name": "fanout", "attributes": {"DisplayName": "Fan"}, "subscriptions": [
            {"protocol": "sqs", "endpoint": "inbox", "attributes": {"RawMessageDelivery": "true"}},
            {"protocol": "sqs", "endpoint": "arn:aws:sqs:us-east-1:000000000000:filtered",
             "attributes": {"FilterPolicy": "{\"kind\": [\"keep\"]}"}},
        ]},
    ]});
    super::seed(&t.state, &doc).await.unwrap();
    super::seed(&t.state, &doc).await.unwrap();
    let list = ok(&t, &[("Action", "ListSubscriptions")]).await;
    assert_eq!(
        list.child("Subscriptions")
            .unwrap()
            .children_named("member")
            .count(),
        2
    );

    let topic = super::service::topic_arn("fanout");
    super::publish_internal(
        &t.state,
        &topic,
        "evt".into(),
        Some("S".into()),
        Some(json!({"kind": {"DataType": "String", "StringValue": "keep"}})),
    )
    .await
    .unwrap();
    super::publish_internal(&t.state, &topic, "evt2".into(), None, None)
        .await
        .unwrap();
    let inbox = drain(&t, "inbox").await;
    assert_eq!(inbox.len(), 2);
    assert_eq!(inbox[0]["Body"], "evt");
    assert_eq!(inbox[0]["MessageAttributes"]["kind"]["StringValue"], "keep");
    let filtered = drain(&t, "filtered").await;
    assert_eq!(filtered.len(), 1);
    assert!(super::publish_internal(
        &t.state,
        "arn:aws:sns:us-east-1:000000000000:none",
        "x".into(),
        None,
        None
    )
    .await
    .is_err());
}

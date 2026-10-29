use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};

use super::models::{md5_of_attrs, parse_attrs};
use crate::testutil::{aws_json, TestApp, TestResponse};

async fn sqs(t: &TestApp, action: &str, body: Value) -> TestResponse {
    t.call(aws_json(&format!("AmazonSQS.{action}"), body)).await
}

async fn ok(t: &TestApp, action: &str, body: Value) -> Value {
    let r = sqs(t, action, body).await;
    assert_eq!(r.status, 200, "{action} failed: {}", r.text());
    r.json()
}

async fn create(t: &TestApp, name: &str, attrs: Value) -> String {
    ok(
        t,
        "CreateQueue",
        json!({"QueueName": name, "Attributes": attrs}),
    )
    .await["QueueUrl"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn receive(t: &TestApp, url: &str, extra: Value) -> Vec<Value> {
    let mut body = json!({"QueueUrl": url, "MaxNumberOfMessages": 10, "MessageSystemAttributeNames": ["All"], "MessageAttributeNames": ["All"]});
    for (k, v) in extra.as_object().unwrap() {
        body[k] = v.clone();
    }
    ok(t, "ReceiveMessage", body).await["Messages"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn assert_error(r: &TestResponse, code: &str, query_code: &str) {
    assert_eq!(r.status, 400, "{}", r.text());
    assert_eq!(r.json()["__type"], format!("com.amazonaws.sqs#{code}"));
    assert_eq!(
        r.header("x-amzn-query-error").unwrap(),
        format!("{query_code};Sender")
    );
}

#[test]
fn md5_of_message_attributes_matches_aws_vectors() {
    // Values observed from AWS (as asserted in moto's SQS test suite).
    let cases = [
        (
            json!({"SOME_Valid.attribute-Name": {"StringValue": "1493147359900", "DataType": "Number"}}),
            "36655e7e9d7c0e8479fa3f3f42247ae7",
        ),
        (
            json!({"timestamp": {"StringValue": "1493147359900", "DataType": "Number"}}),
            "235c5c510d26fb653d073faed50ae77c",
        ),
        (
            json!({"timestamp": {"StringValue": "1493147359900", "DataType": "Number.java.lang.Long"}}),
            "2e2e4876d8e0bd6b8c2c8f556831c349",
        ),
        (
            json!({"somevalue": {"StringValue": "somevalue", "DataType": "String.custom"}}),
            "9e05cca738e70ff6c6041e82d5e77ef1",
        ),
        (
            json!({
                "id": {"StringValue": "453ae55e-f03b-21a6-a4b1-70c2e2e8fe71", "DataType": "String"},
                "mybin": {"BinaryValue": base64::engine::general_purpose::STANDARD.encode("kekchebukek"), "DataType": "Binary"},
                "timestamp": {"StringValue": "1603134247654", "DataType": "Number.java.lang.Long"},
                "contentType": {"StringValue": "application/json", "DataType": "String"},
            }),
            "049075255ebc53fb95f7f9f3cedf3c50",
        ),
    ];
    for (attrs, want) in cases {
        assert_eq!(
            md5_of_attrs(&parse_attrs(Some(&attrs)).unwrap()),
            want,
            "{attrs}"
        );
    }
}

#[tokio::test]
async fn nonexistent_queue_errors_are_query_compatible() {
    let t = TestApp::new().await;
    let r = sqs(&t, "GetQueueUrl", json!({"QueueName": "ghost"})).await;
    assert_error(
        &r,
        "QueueDoesNotExist",
        "AWS.SimpleQueueService.NonExistentQueue",
    );
    assert!(r.json()["message"]
        .as_str()
        .unwrap()
        .contains("does not exist"));
    for action in [
        "SendMessage",
        "ReceiveMessage",
        "GetQueueAttributes",
        "PurgeQueue",
        "DeleteQueue",
        "ListQueueTags",
    ] {
        let r = sqs(
            &t,
            action,
            json!({"QueueUrl": "http://localhost:4793/000000000000/ghost", "MessageBody": "x"}),
        )
        .await;
        assert_error(
            &r,
            "QueueDoesNotExist",
            "AWS.SimpleQueueService.NonExistentQueue",
        );
    }
}

#[tokio::test]
async fn send_and_receive_with_attributes_and_md5() {
    let t = TestApp::new().await;
    let url = create(&t, "attrs", json!({})).await;
    let attrs = json!({
        "timestamp": {"StringValue": "1493147359900", "DataType": "Number"},
        "app.name": {"StringValue": "svc", "DataType": "String"},
        "app.blob": {"BinaryValue": base64::engine::general_purpose::STANDARD.encode([0u8, 1, 2]), "DataType": "Binary"},
    });
    let sent = ok(&t, "SendMessage", json!({
        "QueueUrl": url, "MessageBody": "derp", "MessageAttributes": attrs,
        "MessageSystemAttributes": {"AWSTraceHeader": {"DataType": "String", "StringValue": "Root=1-abc"}},
    }))
    .await;
    assert_eq!(sent["MD5OfMessageBody"], "58fd9edd83341c29f1aebba81c31e257");
    assert_eq!(
        sent["MD5OfMessageAttributes"],
        md5_of_attrs(&parse_attrs(Some(&attrs)).unwrap())
    );
    assert!(sent["MD5OfMessageSystemAttributes"].is_string());

    let msgs = receive(
        &t,
        &url,
        json!({"MessageAttributeNames": ["app.*"], "VisibilityTimeout": 0}),
    )
    .await;
    let m = &msgs[0];
    let got = m["MessageAttributes"].as_object().unwrap();
    assert_eq!(got.len(), 2);
    assert!(got.contains_key("app.name") && got.contains_key("app.blob"));
    assert_eq!(
        m["MD5OfMessageAttributes"],
        md5_of_attrs(&parse_attrs(Some(&m["MessageAttributes"])).unwrap())
    );
    assert_eq!(m["Attributes"]["AWSTraceHeader"], "Root=1-abc");
    assert_eq!(m["Attributes"]["ApproximateReceiveCount"], "1");
    assert_eq!(m["Attributes"]["SenderId"], "000000000000");
    assert!(
        m["Attributes"]["SentTimestamp"]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .unwrap()
            > 1_600_000_000_000
    );

    let m = &receive(&t, &url, json!({"MessageAttributeNames": ["timestamp"], "MessageSystemAttributeNames": ["ApproximateReceiveCount"]})).await[0];
    assert_eq!(
        m["MD5OfMessageAttributes"],
        "235c5c510d26fb653d073faed50ae77c"
    );
    assert_eq!(m["Attributes"], json!({"ApproximateReceiveCount": "2"}));

    let r = sqs(&t, "SendMessage", json!({"QueueUrl": url, "MessageBody": "x", "MessageAttributes": {"n": {"DataType": "Number", "StringValue": "abc"}}})).await;
    assert_error(&r, "InvalidParameterValue", "InvalidParameterValue");
}

#[tokio::test]
async fn queue_attributes_lifecycle() {
    let t = TestApp::new().await;
    let url = create(
        &t,
        "cfg",
        json!({"VisibilityTimeout": "45", "DelaySeconds": "0"}),
    )
    .await;
    // Idempotent with matching attributes, error with different ones.
    assert_eq!(
        create(&t, "cfg", json!({"VisibilityTimeout": "45"})).await,
        url
    );
    let r = sqs(
        &t,
        "CreateQueue",
        json!({"QueueName": "cfg", "Attributes": {"VisibilityTimeout": "10"}}),
    )
    .await;
    assert_error(&r, "QueueNameExists", "QueueAlreadyExists");

    let attrs = ok(
        &t,
        "GetQueueAttributes",
        json!({"QueueUrl": url, "AttributeNames": ["All"]}),
    )
    .await["Attributes"]
        .clone();
    assert_eq!(attrs["VisibilityTimeout"], "45");
    assert_eq!(attrs["MessageRetentionPeriod"], "345600");
    assert_eq!(attrs["MaximumMessageSize"], "262144");
    assert_eq!(attrs["QueueArn"], "arn:aws:sqs:us-east-1:000000000000:cfg");
    assert_eq!(attrs["ApproximateNumberOfMessages"], "0");
    assert!(
        attrs["CreatedTimestamp"]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .unwrap()
            > 1_600_000_000
    );
    assert!(attrs.get("FifoQueue").is_none());

    ok(&t, "SetQueueAttributes", json!({"QueueUrl": url, "Attributes": {"DelaySeconds": "5", "Policy": "{\"Version\":\"2012-10-17\"}"}})).await;
    let attrs = ok(
        &t,
        "GetQueueAttributes",
        json!({"QueueUrl": url, "AttributeNames": ["DelaySeconds", "Policy"]}),
    )
    .await["Attributes"]
        .clone();
    assert_eq!(
        attrs,
        json!({"DelaySeconds": "5", "Policy": "{\"Version\":\"2012-10-17\"}"})
    );

    let r = sqs(
        &t,
        "GetQueueAttributes",
        json!({"QueueUrl": url, "AttributeNames": ["Bogus"]}),
    )
    .await;
    assert_error(&r, "InvalidAttributeName", "InvalidAttributeName");
    let r = sqs(
        &t,
        "SetQueueAttributes",
        json!({"QueueUrl": url, "Attributes": {"VisibilityTimeout": "99999"}}),
    )
    .await;
    assert_error(&r, "InvalidAttributeValue", "InvalidAttributeValue");

    // Tags
    ok(
        &t,
        "TagQueue",
        json!({"QueueUrl": url, "Tags": {"env": "dev", "team": "a"}}),
    )
    .await;
    ok(
        &t,
        "UntagQueue",
        json!({"QueueUrl": url, "TagKeys": ["team"]}),
    )
    .await;
    assert_eq!(
        ok(&t, "ListQueueTags", json!({"QueueUrl": url})).await["Tags"],
        json!({"env": "dev"})
    );

    // ListQueues pagination
    for i in 0..3 {
        create(&t, &format!("page-{i}"), json!({})).await;
    }
    let p1 = ok(
        &t,
        "ListQueues",
        json!({"QueueNamePrefix": "page-", "MaxResults": 2}),
    )
    .await;
    assert_eq!(p1["QueueUrls"].as_array().unwrap().len(), 2);
    let p2 = ok(
        &t,
        "ListQueues",
        json!({"QueueNamePrefix": "page-", "MaxResults": 2, "NextToken": p1["NextToken"]}),
    )
    .await;
    assert_eq!(p2["QueueUrls"].as_array().unwrap().len(), 1);
    assert!(p2.get("NextToken").is_none());

    ok(&t, "DeleteQueue", json!({"QueueUrl": url})).await;
    assert_eq!(
        sqs(&t, "GetQueueUrl", json!({"QueueName": "cfg"}))
            .await
            .status,
        400
    );
}

#[tokio::test]
async fn delay_retention_and_purge() {
    let t = TestApp::new().await;
    let url = create(&t, "delay", json!({"MaximumMessageSize": "1024"})).await;
    ok(
        &t,
        "SendMessage",
        json!({"QueueUrl": url, "MessageBody": "later", "DelaySeconds": 60}),
    )
    .await;
    ok(
        &t,
        "SendMessage",
        json!({"QueueUrl": url, "MessageBody": "now"}),
    )
    .await;
    let attrs = ok(
        &t,
        "GetQueueAttributes",
        json!({"QueueUrl": url, "AttributeNames": ["All"]}),
    )
    .await["Attributes"]
        .clone();
    assert_eq!(attrs["ApproximateNumberOfMessages"], "1");
    assert_eq!(attrs["ApproximateNumberOfMessagesDelayed"], "1");
    let msgs = receive(&t, &url, json!({})).await;
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["Body"], "now");
    let attrs = ok(
        &t,
        "GetQueueAttributes",
        json!({"QueueUrl": url, "AttributeNames": ["ApproximateNumberOfMessagesNotVisible"]}),
    )
    .await;
    assert_eq!(
        attrs["Attributes"]["ApproximateNumberOfMessagesNotVisible"],
        "1"
    );

    // Size limit enforced on send.
    let r = sqs(
        &t,
        "SendMessage",
        json!({"QueueUrl": url, "MessageBody": "x".repeat(2000)}),
    )
    .await;
    assert_error(&r, "InvalidParameterValue", "InvalidParameterValue");

    // The dashboard counts `visible = 1`; it must track receivability.
    let visible: i64 = t
        .state
        .db
        .call(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM sqs_messages WHERE visible = 1",
                [],
                |r| r.get(0),
            )
        })
        .await
        .unwrap();
    assert_eq!(visible, 0);

    ok(&t, "PurgeQueue", json!({"QueueUrl": url})).await;
    let attrs = ok(
        &t,
        "GetQueueAttributes",
        json!({"QueueUrl": url, "AttributeNames": ["All"]}),
    )
    .await["Attributes"]
        .clone();
    assert_eq!(attrs["ApproximateNumberOfMessagesDelayed"], "0");
    assert_eq!(attrs["ApproximateNumberOfMessagesNotVisible"], "0");
}

#[tokio::test]
async fn retention_period_purges_expired_messages() {
    let t = TestApp::new().await;
    let url = create(&t, "short-lived", json!({"MessageRetentionPeriod": "60"})).await;
    ok(
        &t,
        "SendMessage",
        json!({"QueueUrl": url, "MessageBody": "old"}),
    )
    .await;
    ok(
        &t,
        "SendMessage",
        json!({"QueueUrl": url, "MessageBody": "new"}),
    )
    .await;
    t.state
        .db
        .call(|c| {
            c.execute(
                "UPDATE sqs_messages SET sent_at = sent_at - 61000 WHERE body = 'old'",
                [],
            )
        })
        .await
        .unwrap();
    let msgs = receive(&t, &url, json!({})).await;
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["Body"], "new");
    let r = sqs(
        &t,
        "SetQueueAttributes",
        json!({"QueueUrl": url, "Attributes": {"MessageRetentionPeriod": "30"}}),
    )
    .await;
    assert_error(&r, "InvalidAttributeValue", "InvalidAttributeValue");
}

#[tokio::test]
async fn visibility_timeout_expiry_and_change_visibility() {
    let t = TestApp::new().await;
    let url = create(&t, "vis", json!({"VisibilityTimeout": "1"})).await;
    let sent = ok(
        &t,
        "SendMessage",
        json!({"QueueUrl": url, "MessageBody": "hello"}),
    )
    .await;
    let first = receive(&t, &url, json!({})).await;
    assert_eq!(first.len(), 1);
    assert!(receive(&t, &url, json!({})).await.is_empty());
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let second = receive(&t, &url, json!({"VisibilityTimeout": 30})).await;
    assert_eq!(second.len(), 1);
    assert_eq!(second[0]["MessageId"], sent["MessageId"]);
    assert_eq!(second[0]["Attributes"]["ApproximateReceiveCount"], "2");
    assert_ne!(second[0]["ReceiptHandle"], first[0]["ReceiptHandle"]);

    // Timeout 0 makes the message visible immediately.
    let rh = second[0]["ReceiptHandle"].as_str().unwrap();
    ok(
        &t,
        "ChangeMessageVisibility",
        json!({"QueueUrl": url, "ReceiptHandle": rh, "VisibilityTimeout": 0}),
    )
    .await;
    let third = receive(&t, &url, json!({})).await;
    assert_eq!(third.len(), 1);

    let r = sqs(
        &t,
        "ChangeMessageVisibility",
        json!({"QueueUrl": url, "ReceiptHandle": "garbage", "VisibilityTimeout": 5}),
    )
    .await;
    assert_error(&r, "ReceiptHandleIsInvalid", "ReceiptHandleIsInvalid");
    let r = sqs(
        &t,
        "DeleteMessage",
        json!({"QueueUrl": url, "ReceiptHandle": "garbage"}),
    )
    .await;
    assert_error(&r, "ReceiptHandleIsInvalid", "ReceiptHandleIsInvalid");
    // A stale but well-formed handle deletes nothing and succeeds.
    ok(
        &t,
        "DeleteMessage",
        json!({"QueueUrl": url, "ReceiptHandle": first[0]["ReceiptHandle"]}),
    )
    .await;
    ok(
        &t,
        "DeleteMessage",
        json!({"QueueUrl": url, "ReceiptHandle": third[0]["ReceiptHandle"]}),
    )
    .await;
    let attrs = ok(
        &t,
        "GetQueueAttributes",
        json!({"QueueUrl": url, "AttributeNames": ["All"]}),
    )
    .await["Attributes"]
        .clone();
    assert_eq!(attrs["ApproximateNumberOfMessages"], "0");
    assert_eq!(attrs["ApproximateNumberOfMessagesNotVisible"], "0");
}

#[tokio::test]
async fn long_polling_returns_when_message_arrives() {
    let t = TestApp::new().await;
    let url = create(&t, "poll", json!({})).await;

    let start = Instant::now();
    assert!(receive(&t, &url, json!({"WaitTimeSeconds": 1}))
        .await
        .is_empty());
    assert!(start.elapsed() >= Duration::from_millis(950));

    let start = Instant::now();
    let (msgs, _) = tokio::join!(receive(&t, &url, json!({"WaitTimeSeconds": 5})), async {
        tokio::time::sleep(Duration::from_millis(300)).await;
        ok(
            &t,
            "SendMessage",
            json!({"QueueUrl": url, "MessageBody": "late"}),
        )
        .await
    });
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["Body"], "late");
    assert!(start.elapsed() < Duration::from_secs(2));

    // Queue default ReceiveMessageWaitTimeSeconds applies when not given.
    let url2 = create(
        &t,
        "poll-default",
        json!({"ReceiveMessageWaitTimeSeconds": "1"}),
    )
    .await;
    let start = Instant::now();
    assert!(receive(&t, &url2, json!({})).await.is_empty());
    assert!(start.elapsed() >= Duration::from_millis(950));

    let r = sqs(
        &t,
        "ReceiveMessage",
        json!({"QueueUrl": url, "WaitTimeSeconds": 21}),
    )
    .await;
    assert_error(&r, "InvalidParameterValue", "InvalidParameterValue");
}

#[tokio::test]
async fn dead_letter_queue_redrive() {
    let t = TestApp::new().await;
    let dlq = create(&t, "orders-dlq", json!({})).await;
    let r = sqs(&t, "CreateQueue", json!({"QueueName": "bad", "Attributes": {"RedrivePolicy": "{\"deadLetterTargetArn\":\"arn:aws:sqs:us-east-1:000000000000:nope\",\"maxReceiveCount\":2}"}})).await;
    assert_error(&r, "InvalidParameterValue", "InvalidParameterValue");
    let src = create(&t, "orders", json!({
        "RedrivePolicy": "{\"deadLetterTargetArn\":\"arn:aws:sqs:us-east-1:000000000000:orders-dlq\",\"maxReceiveCount\":\"2\"}"
    }))
    .await;
    let attrs = ok(
        &t,
        "GetQueueAttributes",
        json!({"QueueUrl": src, "AttributeNames": ["RedrivePolicy"]}),
    )
    .await;
    assert_eq!(
        serde_json::from_str::<Value>(attrs["Attributes"]["RedrivePolicy"].as_str().unwrap())
            .unwrap(),
        json!({"deadLetterTargetArn": "arn:aws:sqs:us-east-1:000000000000:orders-dlq", "maxReceiveCount": 2})
    );
    let sources = ok(&t, "ListDeadLetterSourceQueues", json!({"QueueUrl": dlq})).await;
    assert_eq!(sources["queueUrls"], json!([src]));

    let sent = ok(
        &t,
        "SendMessage",
        json!({
            "QueueUrl": src, "MessageBody": "poison",
            "MessageAttributes": {"k": {"DataType": "String", "StringValue": "v"}}
        }),
    )
    .await;
    for _ in 0..2 {
        assert_eq!(
            receive(&t, &src, json!({"VisibilityTimeout": 0}))
                .await
                .len(),
            1
        );
    }
    // Third receive exceeds maxReceiveCount: moved to the DLQ instead.
    assert!(receive(&t, &src, json!({})).await.is_empty());
    let moved = receive(&t, &dlq, json!({})).await;
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0]["MessageId"], sent["MessageId"]);
    assert_eq!(moved[0]["Body"], "poison");
    assert_eq!(moved[0]["MessageAttributes"]["k"]["StringValue"], "v");
    assert_eq!(moved[0]["Attributes"]["ApproximateReceiveCount"], "1");
    assert_eq!(
        moved[0]["Attributes"]["DeadLetterQueueSourceArn"],
        "arn:aws:sqs:us-east-1:000000000000:orders"
    );
}

#[tokio::test]
async fn batch_operations() {
    let t = TestApp::new().await;
    let url = create(&t, "batch", json!({})).await;
    let res = ok(&t, "SendMessageBatch", json!({"QueueUrl": url, "Entries": [
        {"Id": "a", "MessageBody": "one", "MessageAttributes": {"x": {"DataType": "String", "StringValue": "1"}}},
        {"Id": "b", "MessageBody": "two", "DelaySeconds": 0},
        {"Id": "c", "MessageBody": "three", "DelaySeconds": 5000},
    ]}))
    .await;
    assert_eq!(res["Successful"].as_array().unwrap().len(), 2);
    assert!(res["Successful"][0]["MD5OfMessageAttributes"].is_string());
    assert_eq!(res["Failed"][0]["Id"], "c");
    assert_eq!(res["Failed"][0]["SenderFault"], true);
    assert_eq!(res["Failed"][0]["Code"], "InvalidParameterValue");

    let msgs = receive(&t, &url, json!({})).await;
    assert_eq!(msgs.len(), 2);
    let res = ok(
        &t,
        "ChangeMessageVisibilityBatch",
        json!({"QueueUrl": url, "Entries": [
            {"Id": "a", "ReceiptHandle": msgs[0]["ReceiptHandle"], "VisibilityTimeout": 0},
            {"Id": "b", "ReceiptHandle": "bogus", "VisibilityTimeout": 0},
        ]}),
    )
    .await;
    assert_eq!(res["Successful"], json!([{"Id": "a"}]));
    assert_eq!(res["Failed"][0]["Code"], "ReceiptHandleIsInvalid");

    let again = receive(&t, &url, json!({})).await;
    assert_eq!(again.len(), 1);
    let res = ok(
        &t,
        "DeleteMessageBatch",
        json!({"QueueUrl": url, "Entries": [
            {"Id": "a", "ReceiptHandle": again[0]["ReceiptHandle"]},
            {"Id": "b", "ReceiptHandle": msgs[1]["ReceiptHandle"]},
        ]}),
    )
    .await;
    assert_eq!(res["Successful"].as_array().unwrap().len(), 2);
    assert_eq!(res["Failed"], json!([]));

    let entries: Vec<Value> = (0..11)
        .map(|i| json!({"Id": format!("m{i}"), "MessageBody": "x"}))
        .collect();
    let r = sqs(
        &t,
        "SendMessageBatch",
        json!({"QueueUrl": url, "Entries": entries}),
    )
    .await;
    assert_error(
        &r,
        "TooManyEntriesInBatchRequest",
        "AWS.SimpleQueueService.TooManyEntriesInBatchRequest",
    );
    let r = sqs(&t, "SendMessageBatch", json!({"QueueUrl": url, "Entries": [{"Id": "a", "MessageBody": "x"}, {"Id": "a", "MessageBody": "y"}]})).await;
    assert_error(
        &r,
        "BatchEntryIdsNotDistinct",
        "AWS.SimpleQueueService.BatchEntryIdsNotDistinct",
    );
    let r = sqs(
        &t,
        "DeleteMessageBatch",
        json!({"QueueUrl": url, "Entries": []}),
    )
    .await;
    assert_error(
        &r,
        "EmptyBatchRequest",
        "AWS.SimpleQueueService.EmptyBatchRequest",
    );
    let r = sqs(
        &t,
        "SendMessageBatch",
        json!({"QueueUrl": url, "Entries": [{"Id": "bad id!", "MessageBody": "x"}]}),
    )
    .await;
    assert_error(
        &r,
        "InvalidBatchEntryId",
        "AWS.SimpleQueueService.InvalidBatchEntryId",
    );
}

#[tokio::test]
async fn fifo_queues() {
    let t = TestApp::new().await;
    let r = sqs(
        &t,
        "CreateQueue",
        json!({"QueueName": "plain", "Attributes": {"FifoQueue": "true"}}),
    )
    .await;
    assert_error(&r, "InvalidParameterValue", "InvalidParameterValue");
    let url = create(
        &t,
        "jobs.fifo",
        json!({"FifoQueue": "true", "ContentBasedDeduplication": "true"}),
    )
    .await;

    let r = sqs(
        &t,
        "SendMessage",
        json!({"QueueUrl": url, "MessageBody": "x"}),
    )
    .await;
    assert_error(&r, "MissingParameter", "MissingParameter");
    let a1 = ok(
        &t,
        "SendMessage",
        json!({"QueueUrl": url, "MessageBody": "a1", "MessageGroupId": "a"}),
    )
    .await;
    let dup = ok(
        &t,
        "SendMessage",
        json!({"QueueUrl": url, "MessageBody": "a1", "MessageGroupId": "a"}),
    )
    .await;
    assert_eq!(a1["MessageId"], dup["MessageId"]);
    assert_eq!(a1["SequenceNumber"], dup["SequenceNumber"]);
    let a2 = ok(&t, "SendMessage", json!({"QueueUrl": url, "MessageBody": "a2", "MessageGroupId": "a", "MessageDeduplicationId": "d2"})).await;
    assert!(a2["SequenceNumber"].as_str().unwrap() > a1["SequenceNumber"].as_str().unwrap());
    ok(
        &t,
        "SendMessage",
        json!({"QueueUrl": url, "MessageBody": "b1", "MessageGroupId": "b"}),
    )
    .await;

    // Receive one message: group "a" is now in flight and blocks a2.
    let first = receive(&t, &url, json!({"MaxNumberOfMessages": 1})).await;
    assert_eq!(first[0]["Body"], "a1");
    assert_eq!(first[0]["Attributes"]["MessageGroupId"], "a");
    assert!(first[0]["Attributes"]["SequenceNumber"].is_string());
    assert!(first[0]["Attributes"]["MessageDeduplicationId"].is_string());
    let next = receive(&t, &url, json!({})).await;
    assert_eq!(next.len(), 1);
    assert_eq!(next[0]["Body"], "b1");
    ok(
        &t,
        "DeleteMessage",
        json!({"QueueUrl": url, "ReceiptHandle": first[0]["ReceiptHandle"]}),
    )
    .await;
    let after = receive(&t, &url, json!({})).await;
    assert_eq!(after[0]["Body"], "a2");

    let r = sqs(
        &t,
        "SendMessage",
        json!({"QueueUrl": url, "MessageBody": "x", "MessageGroupId": "a", "DelaySeconds": 1}),
    )
    .await;
    assert_error(&r, "InvalidParameterValue", "InvalidParameterValue");
    let attrs = ok(
        &t,
        "GetQueueAttributes",
        json!({"QueueUrl": url, "AttributeNames": ["All"]}),
    )
    .await["Attributes"]
        .clone();
    assert_eq!(attrs["FifoQueue"], "true");
    assert_eq!(attrs["ContentBasedDeduplication"], "true");
}

#[tokio::test]
async fn send_internal_and_seed() {
    let t = TestApp::new().await;
    let doc = json!({"queues": [
        "simple",
        {"name": "main", "attributes": {"VisibilityTimeout": 10, "DelaySeconds": 0,
            "RedrivePolicy": {"deadLetterTargetArn": "main-dlq", "maxReceiveCount": 3}},
         "tags": {"env": "test"}},
        {"name": "main-dlq"},
        "ordered.fifo",
    ]});
    super::seed(&t.state, &doc).await.unwrap();
    super::seed(&t.state, &doc).await.unwrap();
    let main = ok(&t, "GetQueueUrl", json!({"QueueName": "main"})).await["QueueUrl"]
        .as_str()
        .unwrap()
        .to_string();
    let attrs = ok(
        &t,
        "GetQueueAttributes",
        json!({"QueueUrl": main, "AttributeNames": ["All"]}),
    )
    .await["Attributes"]
        .clone();
    assert_eq!(attrs["VisibilityTimeout"], "10");
    assert!(attrs["RedrivePolicy"]
        .as_str()
        .unwrap()
        .contains("main-dlq"));
    assert_eq!(
        ok(&t, "ListQueueTags", json!({"QueueUrl": main})).await["Tags"],
        json!({"env": "test"})
    );
    let listed = ok(&t, "ListQueues", json!({})).await["QueueUrls"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(listed, 4);

    let arn = "arn:aws:sqs:us-east-1:000000000000:main";
    let mid = super::send_internal(
        &t.state,
        arn,
        "from-arn".into(),
        Some(json!({"src": {"DataType": "String", "StringValue": "s3"}})),
    )
    .await
    .unwrap();
    super::send_internal(&t.state, &main, "from-url".into(), None)
        .await
        .unwrap();
    super::send_internal(&t.state, "ordered.fifo", "fifo".into(), None)
        .await
        .unwrap();
    let msgs = receive(&t, &main, json!({})).await;
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0]["MessageId"], mid);
    assert_eq!(msgs[0]["MessageAttributes"]["src"]["StringValue"], "s3");
    assert!(super::send_internal(&t.state, "missing", "x".into(), None)
        .await
        .is_err());
}

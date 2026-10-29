//! SES tests driven through HTTP (and SMTP), like an SDK.

use base64::Engine;
use serde_json::{json, Value};

use crate::testutil::{aws_query, raw, rest, TestApp, TestResponse};

async fn v2(t: &TestApp, method: &str, uri: &str, body: Option<Value>) -> TestResponse {
    t.call(rest(method, uri, body)).await
}

async fn create(t: &TestApp, identity: &str) -> TestResponse {
    v2(
        t,
        "POST",
        "/v2/email/identities",
        Some(json!({"EmailIdentity": identity})),
    )
    .await
}

fn simple_send(from: &str, to: &[&str]) -> Value {
    json!({
        "FromEmailAddress": from,
        "Destination": {"ToAddresses": to},
        "Content": {"Simple": {"Subject": {"Data": "Hi"}, "Body": {"Text": {"Data": "Body"}}}},
    })
}

async fn send(t: &TestApp, body: Value) -> TestResponse {
    v2(t, "POST", "/v2/email/outbound-emails", Some(body)).await
}

fn urlencode(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

async fn events(t: &TestApp, message_id: &str) -> Vec<Value> {
    let r = v2(
        t,
        "GET",
        &format!(
            "/_cloudtwin/ses/events?message_id={}",
            urlencode(message_id)
        ),
        None,
    )
    .await;
    r.json()["events"].as_array().cloned().unwrap_or_default()
}

async fn mailbox_count(t: &TestApp) -> i64 {
    t.state
        .db
        .call(|c| c.query_row("SELECT COUNT(*) FROM mailbox_messages", [], |r| r.get(0)))
        .await
        .unwrap()
}

async fn create_topic(t: &TestApp, name: &str) -> String {
    let r = t
        .call(aws_query(&[("Action", "CreateTopic"), ("Name", name)]))
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
    format!("arn:aws:sns:us-east-1:000000000000:{name}")
}

async fn sns_bodies(t: &TestApp, topic: &str) -> Vec<Value> {
    let topic = topic.to_string();
    let rows: Vec<String> = t
        .state
        .db
        .call(move |c| {
            let mut s =
                c.prepare("SELECT body FROM sns_messages WHERE topic_arn = ?1 ORDER BY id")?;
            let rows = s
                .query_map([topic], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(rows)
        })
        .await
        .unwrap();
    rows.iter()
        .map(|b| serde_json::from_str(b).unwrap())
        .collect()
}

fn query(pairs: &[(&str, String)]) -> axum::http::Request<axum::body::Body> {
    let refs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    aws_query(&refs)
}

// ── Item 1: DKIM + verification status ──────────────────────────────────────

#[tokio::test]
async fn domain_identity_returns_dkim_tokens_and_status() {
    let t = TestApp::new().await;
    let r = create(&t, "Example.COM").await;
    assert_eq!(r.status, 200, "{}", r.text());
    let b = r.json();
    assert_eq!(b["IdentityType"], "DOMAIN");
    assert_eq!(b["VerifiedForSendingStatus"], true);
    let dkim = &b["DkimAttributes"];
    assert_eq!(dkim["SigningAttributesOrigin"], "AWS_SES");
    assert_eq!(dkim["Status"], "SUCCESS");
    let tokens = dkim["Tokens"].as_array().unwrap();
    assert_eq!(tokens.len(), 3);
    assert!(tokens.iter().all(|t| t.as_str().unwrap().len() == 32));

    let g = v2(&t, "GET", "/v2/email/identities/example.com", None)
        .await
        .json();
    assert_eq!(g["VerificationStatus"], "SUCCESS");
    assert_eq!(g["DkimAttributes"]["Tokens"], dkim["Tokens"]);
    assert!(g["DkimAttributes"]["LastKeyGenerationTimestamp"].is_number());

    let e = create(&t, "user@example.com").await.json();
    assert_eq!(e["IdentityType"], "EMAIL_ADDRESS");
    assert_eq!(e["DkimAttributes"]["Status"], "NOT_STARTED");

    let list = v2(&t, "GET", "/v2/email/identities", None).await.json();
    assert_eq!(list["EmailIdentities"][0]["VerificationStatus"], "SUCCESS");
}

// ── Item 2: controllable verification ───────────────────────────────────────

#[tokio::test]
async fn manual_verification_waits_for_control_endpoint() {
    let t = TestApp::with_config(|c| c.ses.verification.mode = "manual".into()).await;
    let r = create(&t, "pending.test").await.json();
    assert_eq!(r["VerifiedForSendingStatus"], false);
    assert_eq!(r["DkimAttributes"]["Status"], "PENDING");
    for _ in 0..3 {
        let g = v2(&t, "GET", "/v2/email/identities/pending.test", None)
            .await
            .json();
        assert_eq!(g["VerificationStatus"], "PENDING");
    }
    let c = v2(
        &t,
        "POST",
        "/_cloudtwin/ses/identities/pending.test/verification",
        Some(json!({"status": "FAILED"})),
    )
    .await;
    assert_eq!(c.status, 200, "{}", c.text());
    let g = v2(&t, "GET", "/v2/email/identities/pending.test", None)
        .await
        .json();
    assert_eq!(g["VerificationStatus"], "FAILED");
    assert_eq!(g["DkimAttributes"]["Status"], "FAILED");
    assert_eq!(g["VerificationInfo"]["ErrorType"], "HOST_NOT_FOUND");

    // Email identities get a verification email whose link verifies them.
    create(&t, "me@pending.test").await;
    let body: String = t
        .state
        .db
        .call(|c| {
            c.query_row(
                "SELECT text_body FROM mailbox_messages WHERE to_addrs LIKE '%me@pending.test%'",
                [],
                |r| r.get(0),
            )
        })
        .await
        .unwrap();
    let link = body
        .lines()
        .find(|l| l.contains("/_cloudtwin/ses/verify-email"))
        .unwrap();
    let path = link.trim().trim_start_matches("http://localhost:4793");
    assert_eq!(v2(&t, "GET", path, None).await.status, 200);
    let g = v2(&t, "GET", "/v2/email/identities/me@pending.test", None)
        .await
        .json();
    assert_eq!(g["VerificationStatus"], "SUCCESS");
}

#[tokio::test]
async fn after_checks_delay_and_override_modes() {
    use crate::aws::ses::config::VerificationRule;
    let t = TestApp::with_config(|c| {
        c.ses.verification.mode = "after_checks".into();
        c.ses.verification.checks = 2;
        c.ses.verification.overrides.insert(
            "*.slow.test".into(),
            VerificationRule {
                mode: Some("delay".into()),
                delay_seconds: Some(3600),
                ..Default::default()
            },
        );
        c.ses.verification.overrides.insert(
            "bad.test".into(),
            VerificationRule {
                mode: Some("auto".into()),
                outcome: Some("TEMPORARY_FAILURE".into()),
                ..Default::default()
            },
        );
    })
    .await;
    let status = |b: Value| b["VerificationStatus"].as_str().unwrap().to_string();
    create(&t, "checks.test").await;
    assert_eq!(
        status(
            v2(&t, "GET", "/v2/email/identities/checks.test", None)
                .await
                .json()
        ),
        "PENDING"
    );
    assert_eq!(
        status(
            v2(&t, "GET", "/v2/email/identities/checks.test", None)
                .await
                .json()
        ),
        "SUCCESS"
    );

    create(&t, "a.slow.test").await;
    for _ in 0..3 {
        assert_eq!(
            status(
                v2(&t, "GET", "/v2/email/identities/a.slow.test", None)
                    .await
                    .json()
            ),
            "PENDING"
        );
    }
    create(&t, "bad.test").await;
    assert_eq!(
        status(
            v2(&t, "GET", "/v2/email/identities/bad.test", None)
                .await
                .json()
        ),
        "TEMPORARY_FAILURE"
    );

    // v1 reads count as checks too and use v1 spelling.
    t.call(aws_query(&[
        ("Action", "VerifyDomainIdentity"),
        ("Domain", "v1.test"),
    ]))
    .await;
    let attrs = [
        ("Action", "GetIdentityVerificationAttributes"),
        ("Identities.member.1", "v1.test"),
    ];
    assert!(t
        .call(aws_query(&attrs))
        .await
        .text()
        .contains("<VerificationStatus>Pending</VerificationStatus>"));
    assert!(t
        .call(aws_query(&attrs))
        .await
        .text()
        .contains("<VerificationStatus>Success</VerificationStatus>"));
}

// ── Item 3: AWS-shaped errors ───────────────────────────────────────────────

#[tokio::test]
async fn rest_errors_carry_aws_error_types() {
    let t = TestApp::new().await;
    let r = v2(&t, "GET", "/v2/email/identities/missing.test", None).await;
    assert_eq!(r.status, 404);
    assert_eq!(
        r.header("x-amzn-errortype").as_deref(),
        Some("NotFoundException")
    );
    assert_eq!(r.json()["__type"], "NotFoundException");

    let r = v2(&t, "DELETE", "/v2/email/identities/missing.test", None).await;
    assert_eq!(r.status, 404);
    assert_eq!(
        r.header("x-amzn-errortype").as_deref(),
        Some("NotFoundException")
    );

    create(&t, "dup.test").await;
    let r = create(&t, "dup.test").await;
    assert_eq!(r.status, 400);
    assert_eq!(
        r.header("x-amzn-errortype").as_deref(),
        Some("AlreadyExistsException")
    );

    assert_eq!(
        v2(&t, "DELETE", "/v2/email/identities/dup.test", None)
            .await
            .status,
        200
    );
}

// ── Item 4: custom MAIL FROM ────────────────────────────────────────────────

#[tokio::test]
async fn mail_from_attributes_and_enforcement() {
    let t = TestApp::new().await;
    create(&t, "mf.test").await;
    let bad = v2(
        &t,
        "PUT",
        "/v2/email/identities/mf.test/mail-from",
        Some(json!({"MailFromDomain": "other.test"})),
    )
    .await;
    assert_eq!(bad.status, 400);

    let r = v2(
        &t,
        "PUT",
        "/v2/email/identities/mf.test/mail-from",
        Some(json!({"MailFromDomain": "bounce.mf.test", "BehaviorOnMxFailure": "REJECT_MESSAGE"})),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text());
    let g = v2(&t, "GET", "/v2/email/identities/mf.test", None)
        .await
        .json();
    assert_eq!(
        g["MailFromAttributes"],
        json!({"MailFromDomain": "bounce.mf.test", "MailFromDomainStatus": "SUCCESS", "BehaviorOnMxFailure": "REJECT_MESSAGE"})
    );

    // A pending MAIL FROM with REJECT_MESSAGE blocks sending.
    v2(
        &t,
        "POST",
        "/_cloudtwin/ses/identities/mf.test/verification",
        Some(json!({"mail_from_status": "PENDING"})),
    )
    .await;
    let r = send(&t, simple_send("a@mf.test", &["x@y.test"])).await;
    assert_eq!(r.status, 400);
    assert_eq!(
        r.header("x-amzn-errortype").as_deref(),
        Some("MailFromDomainNotVerifiedException")
    );

    let x = t
        .call(aws_query(&[
            ("Action", "GetIdentityMailFromDomainAttributes"),
            ("Identities.member.1", "mf.test"),
        ]))
        .await
        .text();
    assert!(
        x.contains("<MailFromDomainStatus>Pending</MailFromDomainStatus>"),
        "{x}"
    );
    assert!(
        x.contains("<BehaviorOnMXFailure>RejectMessage</BehaviorOnMXFailure>"),
        "{x}"
    );
}

// ── Item 5: account ─────────────────────────────────────────────────────────

#[tokio::test]
async fn get_account_and_sending_pause() {
    let t = TestApp::new().await;
    create(&t, "acct.test").await;
    send(&t, simple_send("a@acct.test", &["x@y.test", "z@y.test"])).await;
    let a = v2(&t, "GET", "/v2/email/account", None).await.json();
    assert_eq!(a["ProductionAccessEnabled"], true);
    assert_eq!(a["SendingEnabled"], true);
    assert_eq!(a["SendQuota"]["SentLast24Hours"], 2.0);
    assert_eq!(a["EnforcementStatus"], "HEALTHY");

    v2(
        &t,
        "PUT",
        "/v2/email/account/sending",
        Some(json!({"SendingEnabled": false})),
    )
    .await;
    let r = send(&t, simple_send("a@acct.test", &["x@y.test"])).await;
    assert_eq!(
        r.header("x-amzn-errortype").as_deref(),
        Some("SendingPausedException")
    );

    let c = v2(
        &t,
        "POST",
        "/_cloudtwin/ses/account",
        Some(json!({"sending_enabled": null, "production_access_enabled": false})),
    )
    .await
    .json();
    assert_eq!(c["SendingEnabled"], true);
    assert_eq!(c["ProductionAccessEnabled"], false);
    // Sandbox: unverified recipients are rejected, simulator addresses allowed.
    let r = send(&t, simple_send("a@acct.test", &["x@y.test"])).await;
    assert_eq!(
        r.header("x-amzn-errortype").as_deref(),
        Some("MessageRejected")
    );
    let r = send(
        &t,
        simple_send("a@acct.test", &["success@simulator.amazonses.com"]),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text());
}

// ── Items 12, 13: raw mail and sender enforcement ───────────────────────────

#[tokio::test]
async fn raw_mime_send_and_sender_enforcement() {
    let t = TestApp::new().await;
    let r = send(&t, simple_send("nobody@unverified.test", &["x@y.test"])).await;
    assert_eq!(r.status, 400);
    assert_eq!(
        r.header("x-amzn-errortype").as_deref(),
        Some("MessageRejected")
    );
    assert!(r.json()["message"]
        .as_str()
        .unwrap()
        .contains("nobody@unverified.test"));

    create(&t, "raw.test").await;
    let mime = "From: Sender <s@sub.raw.test>\r\nTo: r@y.test\r\nSubject: Raw!\r\nIn-Reply-To: <orig@x>\r\nReferences: <orig@x>\r\n\
MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"B\"\r\n\r\n--B\r\nContent-Type: text/plain\r\n\r\nhello\r\n\
--B\r\nContent-Type: application/pdf; name=\"a.pdf\"\r\nContent-Disposition: attachment; filename=\"a.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0=\r\n--B--\r\n";
    let data = base64::engine::general_purpose::STANDARD.encode(mime);
    let r = send(
        &t,
        json!({
            "Content": {"Raw": {"Data": data}},
            "EmailTags": [{"Name": "org", "Value": "o1"}],
        }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text());
    let mid = r.json()["MessageId"].as_str().unwrap().to_string();
    let atts: i64 = t
        .state
        .db
        .call(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM mailbox_attachments WHERE filename = 'a.pdf'",
                [],
                |r| r.get(0),
            )
        })
        .await
        .unwrap();
    assert_eq!(atts, 1);
    let ev = events(&t, &mid).await;
    let mail = &ev[0]["event"]["mail"];
    assert_eq!(mail["tags"]["org"], json!(["o1"]));
    assert_eq!(mail["commonHeaders"]["subject"], "Raw!");
    assert_eq!(mail["destination"], json!(["r@y.test"]));

    // Simple content: Reply-To and custom headers reach the message.
    let mut body = simple_send("s@raw.test", &["r@y.test"]);
    body["ReplyToAddresses"] = json!(["help@raw.test"]);
    body["Content"]["Simple"]["Headers"] = json!([{"Name": "X-Thread", "Value": "t1"}]);
    let mid = send(&t, body).await.json()["MessageId"]
        .as_str()
        .unwrap()
        .to_string();
    let msgs = v2(&t, "GET", "/_cloudtwin/ses/messages", None).await.json();
    let m = msgs["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["message_id"] == mid)
        .unwrap()
        .clone();
    let raw_text = m["raw"].as_str().unwrap();
    assert!(raw_text.contains("Reply-To: help@raw.test"));
    assert!(raw_text.contains("X-Thread: t1"));

    // Non-strict mode lets unverified senders through.
    let lax = TestApp::with_config(|c| c.ses.strict_verification = false).await;
    assert_eq!(
        send(&lax, simple_send("any@where.test", &["x@y.test"]))
            .await
            .status,
        200
    );
}

// ── Items 14, 15: configuration sets, events, simulator ─────────────────────

#[tokio::test]
async fn configuration_set_events_and_mailbox_simulator() {
    let t = TestApp::new().await;
    create(&t, "events.test").await;
    let topic = create_topic(&t, "ses-events").await;
    let cs = json!({"ConfigurationSetName": "cs1"});
    assert_eq!(
        v2(&t, "POST", "/v2/email/configuration-sets", Some(cs.clone()))
            .await
            .status,
        200
    );
    let dup = v2(&t, "POST", "/v2/email/configuration-sets", Some(cs)).await;
    assert_eq!(
        dup.header("x-amzn-errortype").as_deref(),
        Some("AlreadyExistsException")
    );
    let r = v2(
        &t,
        "POST",
        "/v2/email/configuration-sets/cs1/event-destinations",
        Some(json!({
            "EventDestinationName": "sns",
            "EventDestination": {
                "Enabled": true,
                "MatchingEventTypes": ["SEND", "DELIVERY", "BOUNCE", "COMPLAINT", "DELIVERY_DELAY", "REJECT"],
                "SnsDestination": {"TopicArn": topic},
            }
        })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text());
    let d = v2(
        &t,
        "GET",
        "/v2/email/configuration-sets/cs1/event-destinations",
        None,
    )
    .await
    .json();
    assert_eq!(d["EventDestinations"][0]["Name"], "sns");

    let mut body = simple_send(
        "s@events.test",
        &[
            "success@simulator.amazonses.com",
            "bounce@simulator.amazonses.com",
            "complaint@simulator.amazonses.com",
            "suppressionlist@simulator.amazonses.com",
            "ooto@simulator.amazonses.com",
            "delay@simulator.amazonses.com",
        ],
    );
    body["ConfigurationSetName"] = json!("cs1");
    let mid = send(&t, body).await.json()["MessageId"]
        .as_str()
        .unwrap()
        .to_string();

    let published = sns_bodies(&t, &topic).await;
    let types: Vec<&str> = published
        .iter()
        .map(|e| e["eventType"].as_str().unwrap())
        .collect();
    assert_eq!(
        types,
        vec![
            "Send",
            "Delivery",
            "Bounce",
            "Bounce",
            "Complaint",
            "DeliveryDelay"
        ]
    );
    let delivery = &published[1];
    assert_eq!(
        delivery["delivery"]["recipients"],
        json!([
            "success@simulator.amazonses.com",
            "complaint@simulator.amazonses.com",
            "ooto@simulator.amazonses.com"
        ])
    );
    assert_eq!(delivery["mail"]["messageId"], mid);
    assert_eq!(
        delivery["mail"]["tags"]["ses:configuration-set"],
        json!(["cs1"])
    );
    let subtypes: Vec<&str> = published
        .iter()
        .filter(|e| e["eventType"] == "Bounce")
        .map(|e| e["bounce"]["bounceSubType"].as_str().unwrap())
        .collect();
    assert!(
        subtypes.contains(&"General") && subtypes.contains(&"Suppressed"),
        "{subtypes:?}"
    );
    assert_eq!(
        published[4]["complaint"]["complainedRecipients"][0]["emailAddress"],
        "complaint@simulator.amazonses.com"
    );

    // Reject (CloudTwin extension) produces Send + Reject only.
    let mut body = simple_send("s@events.test", &["reject@simulator.amazonses.com"]);
    body["ConfigurationSetName"] = json!("cs1");
    let mid = send(&t, body).await.json()["MessageId"]
        .as_str()
        .unwrap()
        .to_string();
    let types: Vec<String> = events(&t, &mid)
        .await
        .iter()
        .map(|e| e["event_type"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(types, vec!["Send", "Reject"]);

    let mut body = simple_send("s@events.test", &["x@y.test"]);
    body["ConfigurationSetName"] = json!("nope");
    let r = send(&t, body).await;
    assert_eq!(r.status, 404);
    assert_eq!(
        r.header("x-amzn-errortype").as_deref(),
        Some("NotFoundException")
    );
}

// ── Item 16: suppression list ───────────────────────────────────────────────

#[tokio::test]
async fn suppression_list_crud_and_enforcement() {
    let t = TestApp::new().await;
    create(&t, "sup.test").await;
    let put = v2(
        &t,
        "PUT",
        "/v2/email/suppression/addresses",
        Some(json!({"EmailAddress": "Blocked@Y.test", "Reason": "BOUNCE"})),
    )
    .await;
    assert_eq!(put.status, 200, "{}", put.text());
    let g = v2(
        &t,
        "GET",
        "/v2/email/suppression/addresses/blocked@y.test",
        None,
    )
    .await
    .json();
    assert_eq!(g["SuppressedDestination"]["Reason"], "BOUNCE");
    assert!(g["SuppressedDestination"]["LastUpdateTime"].is_number());
    let l = v2(
        &t,
        "GET",
        "/v2/email/suppression/addresses?Reason=COMPLAINT",
        None,
    )
    .await
    .json();
    assert_eq!(l["SuppressedDestinationSummaries"], json!([]));
    let l = v2(&t, "GET", "/v2/email/suppression/addresses", None)
        .await
        .json();
    assert_eq!(
        l["SuppressedDestinationSummaries"][0]["EmailAddress"],
        "blocked@y.test"
    );

    let mid = send(&t, simple_send("s@sup.test", &["blocked@y.test"]))
        .await
        .json()["MessageId"]
        .as_str()
        .unwrap()
        .to_string();
    let ev = events(&t, &mid).await;
    assert_eq!(
        ev[1]["event"]["bounce"]["bounceSubType"],
        "OnAccountSuppressionList"
    );

    assert_eq!(
        v2(
            &t,
            "DELETE",
            "/v2/email/suppression/addresses/blocked@y.test",
            None
        )
        .await
        .status,
        200
    );
    let missing = v2(
        &t,
        "GET",
        "/v2/email/suppression/addresses/blocked@y.test",
        None,
    )
    .await;
    assert_eq!(
        missing.header("x-amzn-errortype").as_deref(),
        Some("NotFoundException")
    );
}

// ── Items 7, 8: receiving ───────────────────────────────────────────────────

#[tokio::test]
async fn receipt_rules_route_inbound_mail_to_s3_and_sns() {
    let t = TestApp::new().await;
    assert_eq!(t.call(rest("PUT", "/mail-inbound", None)).await.status, 200);
    let topic = create_topic(&t, "inbound-notify").await;

    let r = t
        .call(query(&[
            ("Action", "CreateReceiptRuleSet".into()),
            ("RuleSetName", "rs".into()),
        ]))
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
    let bad = t
        .call(query(&[
            ("Action", "CreateReceiptRule".into()),
            ("RuleSetName", "rs".into()),
            ("Rule.Name", "broken".into()),
            (
                "Rule.Actions.member.1.S3Action.BucketName",
                "no-such-bucket".into(),
            ),
        ]))
        .await;
    assert!(
        bad.text().contains("InvalidS3Configuration"),
        "{}",
        bad.text()
    );

    let r = t
        .call(query(&[
            ("Action", "CreateReceiptRule".into()),
            ("RuleSetName", "rs".into()),
            ("Rule.Name", "store".into()),
            ("Rule.Enabled", "true".into()),
            ("Rule.Recipients.member.1", "orkystra.test".into()),
            (
                "Rule.Actions.member.1.AddHeaderAction.HeaderName",
                "X-Org".into(),
            ),
            (
                "Rule.Actions.member.1.AddHeaderAction.HeaderValue",
                "o1".into(),
            ),
            (
                "Rule.Actions.member.2.S3Action.BucketName",
                "mail-inbound".into(),
            ),
            (
                "Rule.Actions.member.2.S3Action.ObjectKeyPrefix",
                "raw/".into(),
            ),
            ("Rule.Actions.member.2.S3Action.TopicArn", topic.clone()),
            ("Rule.Actions.member.3.StopAction.Scope", "RuleSet".into()),
        ]))
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
    let r = t
        .call(query(&[
            ("Action", "CreateReceiptRule".into()),
            ("RuleSetName", "rs".into()),
            ("After", "store".into()),
            ("Rule.Name", "never".into()),
            ("Rule.Enabled", "true".into()),
            ("Rule.Recipients.member.1", "orkystra.test".into()),
            ("Rule.Actions.member.1.SNSAction.TopicArn", topic.clone()),
        ]))
        .await;
    assert_eq!(r.status, 200, "{}", r.text());

    // No active rule set: everything goes to the capture inbox.
    let msg =
        "From: ext@example.net\r\nTo: inbox@orkystra.test\r\nSubject: Hello\r\n\r\nHi there\r\n";
    let before = mailbox_count(&t).await;
    let r = t
        .call(raw(
            "POST",
            "/_cloudtwin/ses/inbound?from=ext@example.net&to=inbox@orkystra.test",
            &[],
            msg,
        ))
        .await
        .json();
    assert_eq!(r["handled"], json!([]));
    assert_eq!(mailbox_count(&t).await, before + 1);

    t.call(query(&[
        ("Action", "SetActiveReceiptRuleSet".into()),
        ("RuleSetName", "rs".into()),
    ]))
    .await;
    let active = t
        .call(query(&[("Action", "DescribeActiveReceiptRuleSet".into())]))
        .await
        .text();
    assert!(active.contains("<Name>store</Name>"), "{active}");
    assert!(
        active.contains("<BucketName>mail-inbound</BucketName>"),
        "{active}"
    );
    assert!(active.contains("<Enabled>true</Enabled>"), "{active}");

    let r = t
        .call(raw(
            "POST",
            "/_cloudtwin/ses/inbound?from=ext@example.net&to=inbox@orkystra.test,other@else.test",
            &[],
            msg,
        ))
        .await
        .json();
    assert_eq!(r["handled"], json!(["inbox@orkystra.test"]));
    assert_eq!(r["unhandled"], json!(["other@else.test"]));
    let mid = r["message_id"].as_str().unwrap().to_string();
    assert_eq!(mid.len(), 40);

    let obj = t
        .call(rest("GET", &format!("/mail-inbound/raw/{mid}"), None))
        .await;
    assert_eq!(obj.status, 200);
    let stored = obj.text();
    assert!(
        stored.starts_with("X-Org: o1\r\nReturn-Path: <ext@example.net>"),
        "{stored}"
    );
    assert!(stored.contains("Subject: Hello"));

    // S3 action topic notification; StopAction prevented the second rule.
    let notes = sns_bodies(&t, &topic).await;
    assert_eq!(notes.len(), 1, "{notes:?}");
    let n = &notes[0];
    assert_eq!(n["notificationType"], "Received");
    assert_eq!(n["receipt"]["action"]["type"], "S3");
    assert_eq!(n["receipt"]["action"]["objectKey"], format!("raw/{mid}"));
    assert_eq!(n["receipt"]["recipients"], json!(["inbox@orkystra.test"]));
    assert_eq!(n["mail"]["commonHeaders"]["subject"], "Hello");

    let d = t
        .call(query(&[
            ("Action", "DeleteReceiptRuleSet".into()),
            ("RuleSetName", "rs".into()),
        ]))
        .await;
    assert!(d.text().contains("CannotDelete"));
}

async fn smtp_reply(r: &mut tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>) -> String {
    use tokio::io::AsyncBufReadExt;
    let mut line = String::new();
    r.read_line(&mut line).await.unwrap();
    line
}

#[tokio::test]
async fn smtp_listener_runs_receipt_rules() {
    use tokio::io::{AsyncWriteExt, BufReader};
    let t = TestApp::new().await;
    t.call(rest("PUT", "/smtp-bucket", None)).await;
    crate::aws::ses::seed(
        &t.state,
        &json!({"receipt_rule_sets": [{"name": "rs", "active": true, "rules": [{
            "Name": "s3", "Enabled": true, "Recipients": ["smtp.test"],
            "Actions": [{"S3Action": {"BucketName": "smtp-bucket"}}]
        }]}]}),
    )
    .await
    .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(crate::mailbox::smtp::serve(
        t.state.clone(),
        addr.to_string(),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let sock = tokio::net::TcpStream::connect(addr).await.unwrap();
    let (r, mut w) = sock.into_split();
    let mut r = BufReader::new(r);
    assert!(smtp_reply(&mut r).await.starts_with("220"));
    for cmd in [
        "EHLO x\r\n",
        "MAIL FROM:<a@ext.test>\r\n",
        "RCPT TO:<u@smtp.test>\r\n",
        "DATA\r\n",
    ] {
        w.write_all(cmd.as_bytes()).await.unwrap();
        smtp_reply(&mut r).await;
    }
    w.write_all(b"Subject: via smtp\r\n\r\nbody\r\n.\r\n")
        .await
        .unwrap();
    let reply = smtp_reply(&mut r).await;
    assert!(reply.starts_with("250"), "{reply}");
    w.write_all(b"QUIT\r\n").await.unwrap();

    let keys: Vec<String> = t
        .state
        .db
        .call(|c| {
            let mut s = c.prepare("SELECT key FROM s3_objects WHERE bucket = 'smtp-bucket'")?;
            let rows = s
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(rows)
        })
        .await
        .unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(mailbox_count(&t).await, 0);
}

// ── v1 compatibility and seeding ────────────────────────────────────────────

#[tokio::test]
async fn v1_identity_filters_and_raw_send() {
    let t = TestApp::new().await;
    t.call(aws_query(&[
        ("Action", "VerifyDomainIdentity"),
        ("Domain", "v1.test"),
    ]))
    .await;
    t.call(aws_query(&[
        ("Action", "VerifyEmailIdentity"),
        ("EmailAddress", "a@v1.test"),
    ]))
    .await;
    let d = t
        .call(aws_query(&[
            ("Action", "ListIdentities"),
            ("IdentityType", "Domain"),
        ]))
        .await
        .text();
    assert!(
        d.contains("<member>v1.test</member>") && !d.contains("a@v1.test"),
        "{d}"
    );
    let e = t
        .call(aws_query(&[
            ("Action", "ListIdentities"),
            ("IdentityType", "EmailAddress"),
        ]))
        .await
        .text();
    assert!(
        e.contains("a@v1.test") && !e.contains("<member>v1.test</member>"),
        "{e}"
    );
    let dk = t
        .call(aws_query(&[
            ("Action", "GetIdentityDkimAttributes"),
            ("Identities.member.1", "v1.test"),
        ]))
        .await
        .text();
    assert_eq!(dk.matches("<member>").count(), 3, "{dk}");

    let raw_msg = base64::engine::general_purpose::STANDARD
        .encode("From: a@v1.test\r\nTo: b@x.test\r\nSubject: s\r\n\r\nb");
    let r = t
        .call(aws_query(&[
            ("Action", "SendRawEmail"),
            ("RawMessage.Data", &raw_msg),
        ]))
        .await;
    assert!(r.text().contains("<MessageId>"), "{}", r.text());
    let r = t
        .call(aws_query(&[
            ("Action", "SendEmail"),
            ("Source", "x@unverified.test"),
            ("Destination.ToAddresses.member.1", "b@x.test"),
        ]))
        .await;
    assert_eq!(r.status, 400);
    assert!(r.text().contains("MessageRejected"));
}

#[tokio::test]
async fn seed_is_idempotent() {
    let t = TestApp::new().await;
    create_topic(&t, "seed-topic").await;
    let doc = json!({
        "configuration_sets": [{"name": "cs", "event_destinations": [
            {"name": "d", "sns_topic_arn": "arn:aws:sns:us-east-1:000000000000:seed-topic"}
        ]}],
        "identities": ["seed.test", {"identity": "pending.seed.test", "status": "PENDING",
            "configuration_set": "cs", "mail_from_domain": "mail.pending.seed.test"}],
        "suppressed_destinations": [{"email": "no@seed.test", "reason": "COMPLAINT"}],
    });
    crate::aws::ses::seed(&t.state, &doc).await.unwrap();
    crate::aws::ses::seed(&t.state, &doc).await.unwrap();
    let g = v2(&t, "GET", "/v2/email/identities/pending.seed.test", None)
        .await
        .json();
    assert_eq!(g["VerificationStatus"], "PENDING");
    assert_eq!(g["ConfigurationSetName"], "cs");
    assert_eq!(g["MailFromAttributes"]["MailFromDomainStatus"], "SUCCESS");
    let s = v2(&t, "GET", "/v2/email/identities/seed.test", None)
        .await
        .json();
    assert_eq!(s["VerificationStatus"], "SUCCESS");
    let l = v2(&t, "GET", "/v2/email/configuration-sets", None)
        .await
        .json();
    assert_eq!(l["ConfigurationSets"], json!(["cs"]));
}

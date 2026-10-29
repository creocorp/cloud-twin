use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};

use crate::testutil::{aws_json, rest, TestApp, TestResponse};

async fn kms(t: &TestApp, op: &str, body: Value) -> TestResponse {
    t.call(aws_json(&format!("TrentService.{op}"), body)).await
}

async fn ok(t: &TestApp, op: &str, body: Value) -> Value {
    let r = kms(t, op, body).await;
    assert_eq!(r.status, 200, "{op}: {}", r.text());
    r.json()
}

async fn err(t: &TestApp, op: &str, body: Value) -> String {
    let r = kms(t, op, body).await;
    assert_eq!(r.status, 400, "{op} expected error: {}", r.text());
    r.json()["__type"].as_str().unwrap().to_string()
}

async fn create_key(t: &TestApp) -> String {
    let v = ok(t, "CreateKey", json!({"Description": "test key"})).await;
    v["KeyMetadata"]["KeyId"].as_str().unwrap().to_string()
}

fn arn(id: &str) -> String {
    format!("arn:aws:kms:us-east-1:000000000000:key/{id}")
}

#[tokio::test]
async fn create_and_describe_key_metadata() {
    let t = TestApp::new().await;
    let v = ok(
        &t,
        "CreateKey",
        json!({"Description": "d", "Tags": [{"TagKey": "env", "TagValue": "dev"}]}),
    )
    .await;
    let m = &v["KeyMetadata"];
    let id = m["KeyId"].as_str().unwrap();
    assert_eq!(m["Arn"], arn(id));
    assert_eq!(m["AWSAccountId"], "000000000000");
    assert_eq!(m["KeyUsage"], "ENCRYPT_DECRYPT");
    assert_eq!(m["KeySpec"], "SYMMETRIC_DEFAULT");
    assert_eq!(m["CustomerMasterKeySpec"], "SYMMETRIC_DEFAULT");
    assert_eq!(m["KeyState"], "Enabled");
    assert_eq!(m["Enabled"], true);
    assert_eq!(m["KeyManager"], "CUSTOMER");
    assert_eq!(m["Origin"], "AWS_KMS");
    assert_eq!(m["MultiRegion"], false);
    assert_eq!(m["EncryptionAlgorithms"], json!(["SYMMETRIC_DEFAULT"]));
    assert!(m["CreationDate"].is_number());

    let d = ok(&t, "DescribeKey", json!({"KeyId": arn(id)})).await;
    assert_eq!(d["KeyMetadata"]["Description"], "d");
    let tags = ok(&t, "ListResourceTags", json!({"KeyId": id})).await;
    assert_eq!(tags["Tags"], json!([{"TagKey": "env", "TagValue": "dev"}]));

    assert_eq!(
        err(&t, "DescribeKey", json!({"KeyId": "nope"})).await,
        "NotFoundException"
    );
    assert_eq!(
        err(&t, "Bogus", json!({})).await,
        "UnsupportedOperationException"
    );
}

#[tokio::test]
async fn encrypt_decrypt_roundtrip_via_alias_and_arn() {
    let t = TestApp::new().await;
    let id = create_key(&t).await;
    ok(
        &t,
        "CreateAlias",
        json!({"AliasName": "alias/app", "TargetKeyId": id}),
    )
    .await;

    let pt = B64.encode(b"hello kms");
    for key_ref in [
        "alias/app".to_string(),
        "arn:aws:kms:us-east-1:000000000000:alias/app".to_string(),
        arn(&id),
        id.clone(),
    ] {
        let e = ok(&t, "Encrypt", json!({"KeyId": key_ref, "Plaintext": pt})).await;
        assert_eq!(e["KeyId"], arn(&id));
        assert_eq!(e["EncryptionAlgorithm"], "SYMMETRIC_DEFAULT");
        let blob = e["CiphertextBlob"].as_str().unwrap();
        let d = ok(&t, "Decrypt", json!({"CiphertextBlob": blob})).await;
        assert_eq!(d["Plaintext"], pt);
        assert_eq!(d["KeyId"], arn(&id));
        // explicit KeyId matching the blob is accepted
        ok(
            &t,
            "Decrypt",
            json!({"CiphertextBlob": blob, "KeyId": "alias/app"}),
        )
        .await;
    }

    // tampered ciphertext
    let e = ok(
        &t,
        "Encrypt",
        json!({"KeyId": "alias/app", "Plaintext": pt}),
    )
    .await;
    let mut raw = B64.decode(e["CiphertextBlob"].as_str().unwrap()).unwrap();
    let last = raw.len() - 1;
    raw[last] ^= 1;
    assert_eq!(
        err(&t, "Decrypt", json!({"CiphertextBlob": B64.encode(&raw)})).await,
        "InvalidCiphertextException"
    );
    assert_eq!(
        err(
            &t,
            "Decrypt",
            json!({"CiphertextBlob": B64.encode(b"garbage")})
        )
        .await,
        "InvalidCiphertextException"
    );

    // wrong KeyId on decrypt
    let other = create_key(&t).await;
    assert_eq!(
        err(
            &t,
            "Decrypt",
            json!({"CiphertextBlob": e["CiphertextBlob"], "KeyId": other})
        )
        .await,
        "IncorrectKeyException"
    );

    // plaintext limit
    let big = B64.encode(vec![1u8; 4097]);
    assert_eq!(
        err(&t, "Encrypt", json!({"KeyId": id, "Plaintext": big})).await,
        "ValidationException"
    );
    ok(
        &t,
        "Encrypt",
        json!({"KeyId": id, "Plaintext": B64.encode(vec![1u8; 4096])}),
    )
    .await;
}

#[tokio::test]
async fn encryption_context_is_bound() {
    let t = TestApp::new().await;
    let id = create_key(&t).await;
    let pt = B64.encode(b"secret");
    let e = ok(
        &t,
        "Encrypt",
        json!({"KeyId": id, "Plaintext": pt, "EncryptionContext": {"b": "2", "a": "1"}}),
    )
    .await;
    let blob = e["CiphertextBlob"].clone();
    // key order does not matter
    let d = ok(
        &t,
        "Decrypt",
        json!({"CiphertextBlob": blob, "EncryptionContext": {"a": "1", "b": "2"}}),
    )
    .await;
    assert_eq!(d["Plaintext"], pt);
    for bad in [json!(null), json!({"a": "1"}), json!({"a": "1", "b": "3"})] {
        assert_eq!(
            err(
                &t,
                "Decrypt",
                json!({"CiphertextBlob": blob, "EncryptionContext": bad})
            )
            .await,
            "InvalidCiphertextException"
        );
    }
}

#[tokio::test]
async fn data_keys_random_and_reencrypt() {
    let t = TestApp::new().await;
    let id = create_key(&t).await;
    let ctx = json!({"purpose": "test"});
    let dk = ok(
        &t,
        "GenerateDataKey",
        json!({"KeyId": id, "KeySpec": "AES_256", "EncryptionContext": ctx}),
    )
    .await;
    let pt = B64.decode(dk["Plaintext"].as_str().unwrap()).unwrap();
    assert_eq!(pt.len(), 32);
    assert_eq!(dk["KeyId"], arn(&id));
    let d = ok(
        &t,
        "Decrypt",
        json!({"CiphertextBlob": dk["CiphertextBlob"], "EncryptionContext": ctx}),
    )
    .await;
    assert_eq!(d["Plaintext"], dk["Plaintext"]);

    let dk = ok(
        &t,
        "GenerateDataKey",
        json!({"KeyId": id, "KeySpec": "AES_128"}),
    )
    .await;
    assert_eq!(
        B64.decode(dk["Plaintext"].as_str().unwrap()).unwrap().len(),
        16
    );
    let dk = ok(
        &t,
        "GenerateDataKey",
        json!({"KeyId": id, "NumberOfBytes": 64}),
    )
    .await;
    assert_eq!(
        B64.decode(dk["Plaintext"].as_str().unwrap()).unwrap().len(),
        64
    );
    assert_eq!(
        err(&t, "GenerateDataKey", json!({"KeyId": id})).await,
        "ValidationException"
    );
    assert_eq!(
        err(
            &t,
            "GenerateDataKey",
            json!({"KeyId": id, "KeySpec": "AES_256", "NumberOfBytes": 2})
        )
        .await,
        "ValidationException"
    );
    assert_eq!(
        err(
            &t,
            "GenerateDataKey",
            json!({"KeyId": id, "NumberOfBytes": 1025})
        )
        .await,
        "ValidationException"
    );

    let wp = ok(
        &t,
        "GenerateDataKeyWithoutPlaintext",
        json!({"KeyId": id, "KeySpec": "AES_256"}),
    )
    .await;
    assert!(wp.get("Plaintext").is_none());
    let d = ok(
        &t,
        "Decrypt",
        json!({"CiphertextBlob": wp["CiphertextBlob"]}),
    )
    .await;
    assert_eq!(
        B64.decode(d["Plaintext"].as_str().unwrap()).unwrap().len(),
        32
    );

    let r = ok(&t, "GenerateRandom", json!({"NumberOfBytes": 24})).await;
    assert_eq!(
        B64.decode(r["Plaintext"].as_str().unwrap()).unwrap().len(),
        24
    );
    assert_eq!(
        err(&t, "GenerateRandom", json!({"NumberOfBytes": 0})).await,
        "ValidationException"
    );

    // ReEncrypt to a second key with a new context
    let dest = create_key(&t).await;
    let e = ok(
        &t,
        "Encrypt",
        json!({"KeyId": id, "Plaintext": B64.encode(b"move me"), "EncryptionContext": ctx}),
    )
    .await;
    let re = ok(
        &t,
        "ReEncrypt",
        json!({"CiphertextBlob": e["CiphertextBlob"], "SourceEncryptionContext": ctx,
               "DestinationKeyId": dest, "DestinationEncryptionContext": {"x": "y"}}),
    )
    .await;
    assert_eq!(re["SourceKeyId"], arn(&id));
    assert_eq!(re["KeyId"], arn(&dest));
    let d = ok(
        &t,
        "Decrypt",
        json!({"CiphertextBlob": re["CiphertextBlob"], "EncryptionContext": {"x": "y"}}),
    )
    .await;
    assert_eq!(d["Plaintext"], B64.encode(b"move me"));
    assert_eq!(d["KeyId"], arn(&dest));
}

#[tokio::test]
async fn disabled_key_rejects_crypto() {
    let t = TestApp::new().await;
    let id = create_key(&t).await;
    let e = ok(
        &t,
        "Encrypt",
        json!({"KeyId": id, "Plaintext": B64.encode(b"x")}),
    )
    .await;
    ok(&t, "DisableKey", json!({"KeyId": id})).await;
    let d = ok(&t, "DescribeKey", json!({"KeyId": id})).await;
    assert_eq!(d["KeyMetadata"]["KeyState"], "Disabled");
    assert_eq!(d["KeyMetadata"]["Enabled"], false);
    assert_eq!(
        err(
            &t,
            "Encrypt",
            json!({"KeyId": id, "Plaintext": B64.encode(b"x")})
        )
        .await,
        "DisabledException"
    );
    assert_eq!(
        err(
            &t,
            "Decrypt",
            json!({"CiphertextBlob": e["CiphertextBlob"]})
        )
        .await,
        "DisabledException"
    );
    assert_eq!(
        err(
            &t,
            "GenerateDataKey",
            json!({"KeyId": id, "KeySpec": "AES_256"})
        )
        .await,
        "DisabledException"
    );
    ok(&t, "EnableKey", json!({"KeyId": id})).await;
    ok(
        &t,
        "Decrypt",
        json!({"CiphertextBlob": e["CiphertextBlob"]}),
    )
    .await;
}

#[tokio::test]
async fn non_symmetric_keys_reject_encrypt() {
    let t = TestApp::new().await;
    let v = ok(
        &t,
        "CreateKey",
        json!({"KeySpec": "RSA_2048", "KeyUsage": "SIGN_VERIFY"}),
    )
    .await;
    let id = v["KeyMetadata"]["KeyId"].as_str().unwrap().to_string();
    assert!(v["KeyMetadata"]["SigningAlgorithms"].is_array());
    assert_eq!(
        err(
            &t,
            "Encrypt",
            json!({"KeyId": id, "Plaintext": B64.encode(b"x")})
        )
        .await,
        "InvalidKeyUsageException"
    );
    let v = ok(
        &t,
        "CreateKey",
        json!({"KeySpec": "RSA_2048", "KeyUsage": "ENCRYPT_DECRYPT"}),
    )
    .await;
    let id = v["KeyMetadata"]["KeyId"].as_str().unwrap().to_string();
    assert_eq!(
        err(
            &t,
            "Encrypt",
            json!({"KeyId": id, "Plaintext": B64.encode(b"x")})
        )
        .await,
        "UnsupportedOperationException"
    );
    assert_eq!(
        err(
            &t,
            "CreateKey",
            json!({"KeySpec": "SYMMETRIC_DEFAULT", "KeyUsage": "SIGN_VERIFY"})
        )
        .await,
        "ValidationException"
    );
}

#[tokio::test]
async fn alias_lifecycle() {
    let t = TestApp::new().await;
    let k1 = create_key(&t).await;
    let k2 = create_key(&t).await;
    ok(
        &t,
        "CreateAlias",
        json!({"AliasName": "alias/svc", "TargetKeyId": k1}),
    )
    .await;
    assert_eq!(
        err(
            &t,
            "CreateAlias",
            json!({"AliasName": "alias/svc", "TargetKeyId": k2})
        )
        .await,
        "AlreadyExistsException"
    );
    assert_eq!(
        err(
            &t,
            "CreateAlias",
            json!({"AliasName": "svc", "TargetKeyId": k2})
        )
        .await,
        "InvalidAliasNameException"
    );
    assert_eq!(
        err(
            &t,
            "CreateAlias",
            json!({"AliasName": "alias/aws/s3", "TargetKeyId": k2})
        )
        .await,
        "InvalidAliasNameException"
    );
    assert_eq!(
        err(
            &t,
            "CreateAlias",
            json!({"AliasName": "alias/x", "TargetKeyId": "missing"})
        )
        .await,
        "NotFoundException"
    );

    let l = ok(&t, "ListAliases", json!({})).await;
    let a = &l["Aliases"][0];
    assert_eq!(a["AliasName"], "alias/svc");
    assert_eq!(
        a["AliasArn"],
        "arn:aws:kms:us-east-1:000000000000:alias/svc"
    );
    assert_eq!(a["TargetKeyId"], k1);
    assert!(a["CreationDate"].is_number() && a["LastUpdatedDate"].is_number());

    ok(
        &t,
        "UpdateAlias",
        json!({"AliasName": "alias/svc", "TargetKeyId": k2}),
    )
    .await;
    let d = ok(&t, "DescribeKey", json!({"KeyId": "alias/svc"})).await;
    assert_eq!(d["KeyMetadata"]["KeyId"], k2);
    let l = ok(&t, "ListAliases", json!({"KeyId": k1})).await;
    assert_eq!(l["Aliases"], json!([]));
    let l = ok(&t, "ListAliases", json!({"KeyId": arn(&k2)})).await;
    assert_eq!(l["Aliases"].as_array().unwrap().len(), 1);

    ok(&t, "DeleteAlias", json!({"AliasName": "alias/svc"})).await;
    assert_eq!(
        err(&t, "DeleteAlias", json!({"AliasName": "alias/svc"})).await,
        "NotFoundException"
    );
    assert_eq!(
        err(&t, "DescribeKey", json!({"KeyId": "alias/svc"})).await,
        "NotFoundException"
    );
    assert_eq!(
        err(
            &t,
            "UpdateAlias",
            json!({"AliasName": "alias/svc", "TargetKeyId": k1})
        )
        .await,
        "NotFoundException"
    );
}

#[tokio::test]
async fn deletion_scheduling() {
    let t = TestApp::new().await;
    let id = create_key(&t).await;
    assert_eq!(
        err(
            &t,
            "ScheduleKeyDeletion",
            json!({"KeyId": id, "PendingWindowInDays": 3})
        )
        .await,
        "ValidationException"
    );
    let s = ok(
        &t,
        "ScheduleKeyDeletion",
        json!({"KeyId": id, "PendingWindowInDays": 7}),
    )
    .await;
    assert_eq!(s["KeyId"], arn(&id));
    assert_eq!(s["KeyState"], "PendingDeletion");
    assert_eq!(s["PendingWindowInDays"], 7);
    assert!(s["DeletionDate"].is_number());
    let d = ok(&t, "DescribeKey", json!({"KeyId": id})).await;
    assert_eq!(d["KeyMetadata"]["KeyState"], "PendingDeletion");
    assert!(d["KeyMetadata"]["DeletionDate"].is_number());
    assert_eq!(
        err(
            &t,
            "Encrypt",
            json!({"KeyId": id, "Plaintext": B64.encode(b"x")})
        )
        .await,
        "KMSInvalidStateException"
    );
    assert_eq!(
        err(&t, "EnableKey", json!({"KeyId": id})).await,
        "KMSInvalidStateException"
    );
    assert_eq!(
        err(&t, "ScheduleKeyDeletion", json!({"KeyId": id})).await,
        "KMSInvalidStateException"
    );

    let c = ok(&t, "CancelKeyDeletion", json!({"KeyId": id})).await;
    assert_eq!(c["KeyId"], arn(&id));
    let d = ok(&t, "DescribeKey", json!({"KeyId": id})).await;
    assert_eq!(d["KeyMetadata"]["KeyState"], "Disabled");
    assert!(d["KeyMetadata"].get("DeletionDate").is_none());
    assert_eq!(
        err(&t, "CancelKeyDeletion", json!({"KeyId": id})).await,
        "KMSInvalidStateException"
    );

    // Expired deletions are purged lazily.
    let s = ok(&t, "ScheduleKeyDeletion", json!({"KeyId": id})).await;
    assert_eq!(s["PendingWindowInDays"], 30);
    let kid = id.clone();
    t.state
        .db
        .call(move |c| {
            c.execute("UPDATE kms_keys SET deletion_date = 1 WHERE id = ?1", [kid])?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        err(&t, "DescribeKey", json!({"KeyId": id})).await,
        "NotFoundException"
    );
}

#[tokio::test]
async fn rotation_tags_policy_description_and_paging() {
    let t = TestApp::new().await;
    let id = create_key(&t).await;
    let r = ok(&t, "GetKeyRotationStatus", json!({"KeyId": id})).await;
    assert_eq!(r["KeyRotationEnabled"], false);
    ok(&t, "EnableKeyRotation", json!({"KeyId": id})).await;
    let r = ok(&t, "GetKeyRotationStatus", json!({"KeyId": id})).await;
    assert_eq!(r["KeyRotationEnabled"], true);
    assert_eq!(r["RotationPeriodInDays"], 365);
    ok(&t, "DisableKeyRotation", json!({"KeyId": id})).await;

    ok(&t, "TagResource", json!({"KeyId": id, "Tags": [{"TagKey": "a", "TagValue": "1"}, {"TagKey": "b", "TagValue": "2"}]})).await;
    ok(
        &t,
        "TagResource",
        json!({"KeyId": id, "Tags": [{"TagKey": "a", "TagValue": "9"}]}),
    )
    .await;
    ok(&t, "UntagResource", json!({"KeyId": id, "TagKeys": ["b"]})).await;
    let tags = ok(&t, "ListResourceTags", json!({"KeyId": id})).await;
    assert_eq!(tags["Tags"], json!([{"TagKey": "a", "TagValue": "9"}]));
    assert_eq!(tags["Truncated"], false);

    let p = ok(
        &t,
        "GetKeyPolicy",
        json!({"KeyId": id, "PolicyName": "default"}),
    )
    .await;
    let pol: Value = serde_json::from_str(p["Policy"].as_str().unwrap()).unwrap();
    assert_eq!(pol["Version"], "2012-10-17");
    let new_policy = json!({"Version": "2012-10-17", "Statement": []}).to_string();
    ok(
        &t,
        "PutKeyPolicy",
        json!({"KeyId": id, "PolicyName": "default", "Policy": new_policy}),
    )
    .await;
    let p = ok(&t, "GetKeyPolicy", json!({"KeyId": id})).await;
    assert_eq!(p["Policy"], new_policy);
    assert_eq!(
        err(
            &t,
            "PutKeyPolicy",
            json!({"KeyId": id, "Policy": "not json"})
        )
        .await,
        "MalformedPolicyDocumentException"
    );
    let lp = ok(&t, "ListKeyPolicies", json!({"KeyId": id})).await;
    assert_eq!(lp["PolicyNames"], json!(["default"]));

    ok(
        &t,
        "UpdateKeyDescription",
        json!({"KeyId": id, "Description": "new"}),
    )
    .await;
    let d = ok(&t, "DescribeKey", json!({"KeyId": id})).await;
    assert_eq!(d["KeyMetadata"]["Description"], "new");

    for _ in 0..4 {
        create_key(&t).await;
    }
    let p1 = ok(&t, "ListKeys", json!({"Limit": 2})).await;
    assert_eq!(p1["Keys"].as_array().unwrap().len(), 2);
    assert_eq!(p1["Truncated"], true);
    assert_eq!(p1["Keys"][0]["KeyId"], id);
    assert_eq!(p1["Keys"][0]["KeyArn"], arn(&id));
    let mut seen = 2;
    let mut marker = p1["NextMarker"].as_str().unwrap().to_string();
    loop {
        let p = ok(&t, "ListKeys", json!({"Limit": 2, "Marker": marker})).await;
        seen += p["Keys"].as_array().unwrap().len();
        match p["NextMarker"].as_str() {
            Some(m) => marker = m.to_string(),
            None => {
                assert_eq!(p["Truncated"], false);
                break;
            }
        }
    }
    assert_eq!(seen, 5);
    assert_eq!(
        err(&t, "ListKeys", json!({"Marker": "zzz"})).await,
        "InvalidMarkerException"
    );
}

#[tokio::test]
async fn seed_is_idempotent_and_stable() {
    let t = TestApp::new().await;
    let id = "11111111-2222-4333-8444-555555555555";
    let doc = json!({"kms": {"keys": [
        {"id": id, "alias": "alias/seeded", "description": "seeded key"},
        {"aliases": ["alias/second", "alias/second-b"], "enabled": false},
        {"description": "no alias"}
    ]}});
    for _ in 0..3 {
        let r = t
            .call(rest("POST", "/_cloudtwin/seed", Some(doc.clone())))
            .await;
        assert_eq!(r.status, 200, "{}", r.text());
    }
    let keys = ok(&t, "ListKeys", json!({})).await;
    assert_eq!(keys["Keys"].as_array().unwrap().len(), 3);
    let aliases = ok(&t, "ListAliases", json!({})).await;
    assert_eq!(aliases["Aliases"].as_array().unwrap().len(), 3);

    let d = ok(&t, "DescribeKey", json!({"KeyId": "alias/seeded"})).await;
    assert_eq!(d["KeyMetadata"]["KeyId"], id);
    assert_eq!(d["KeyMetadata"]["Description"], "seeded key");
    let d2 = ok(&t, "DescribeKey", json!({"KeyId": "alias/second-b"})).await;
    assert_eq!(d2["KeyMetadata"]["KeyState"], "Disabled");

    // Ciphertexts from a seeded key survive a reset + reseed (deterministic material).
    let e = ok(
        &t,
        "Encrypt",
        json!({"KeyId": "alias/seeded", "Plaintext": B64.encode(b"stable")}),
    )
    .await;
    let r = t
        .call(rest(
            "POST",
            "/_cloudtwin/reset",
            Some(json!({"services": ["kms"]})),
        ))
        .await;
    assert_eq!(r.status, 200);
    let r = t
        .call(rest("POST", "/_cloudtwin/seed", Some(doc.clone())))
        .await;
    assert_eq!(r.status, 200);
    let d = ok(
        &t,
        "Decrypt",
        json!({"CiphertextBlob": e["CiphertextBlob"]}),
    )
    .await;
    assert_eq!(d["Plaintext"], B64.encode(b"stable"));

    let bad = json!({"kms": {"keys": [{"id": "not-a-uuid"}]}});
    let r = t.call(rest("POST", "/_cloudtwin/seed", Some(bad))).await;
    assert_eq!(r.status, 400);
}

#[tokio::test]
async fn mutating_actions_emit_telemetry() {
    let t = TestApp::new().await;
    let id = create_key(&t).await;
    ok(
        &t,
        "CreateAlias",
        json!({"AliasName": "alias/tele", "TargetKeyId": id}),
    )
    .await;
    let actions: Vec<String> = t
        .state
        .db
        .call(|c| {
            let mut s = c.prepare("SELECT action FROM events WHERE service = 'kms' ORDER BY id")?;
            let v = s
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(v)
        })
        .await
        .unwrap();
    assert_eq!(actions, vec!["create_key", "create_alias"]);
}

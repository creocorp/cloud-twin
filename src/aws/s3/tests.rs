//! S3 behavior tests driven through HTTP like an SDK would.

use serde_json::{json, Value};

use crate::testutil::*;

async fn req(
    t: &TestApp,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    body: impl Into<axum::body::Bytes>,
) -> TestResponse {
    t.call(raw(method, uri, headers, body)).await
}

async fn get(t: &TestApp, uri: &str) -> TestResponse {
    req(t, "GET", uri, &[], "").await
}

async fn put(t: &TestApp, uri: &str, body: &str) -> TestResponse {
    req(t, "PUT", uri, &[], body.to_string()).await
}

async fn bucket(t: &TestApp, name: &str) {
    assert_eq!(put(t, &format!("/{name}"), "").await.status, 200);
}

fn xml(r: &TestResponse) -> crate::xml::Element {
    crate::xml::parse(&r.body).unwrap_or_else(|e| panic!("bad xml {e}: {}", r.text()))
}

fn texts(e: &crate::xml::Element, path: &[&str], leaf: &str) -> Vec<String> {
    let mut nodes = vec![e];
    for p in path {
        nodes = nodes
            .into_iter()
            .flat_map(|n| n.children_named(p))
            .collect();
    }
    nodes
        .into_iter()
        .filter_map(|n| n.child(leaf).map(|c| c.text.clone()))
        .collect()
}

fn code(r: &TestResponse) -> String {
    xml(r).child_text("Code").unwrap_or_default()
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

async fn receive(t: &TestApp, url: &str) -> Vec<Value> {
    let r = t
        .call(aws_json(
            "AmazonSQS.ReceiveMessage",
            json!({"QueueUrl": url, "MaxNumberOfMessages": 10}),
        ))
        .await;
    r.json()["Messages"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|m| serde_json::from_str(m["Body"].as_str().unwrap()).unwrap())
        .collect()
}

#[tokio::test]
async fn bucket_lifecycle_and_errors() {
    let t = TestApp::new().await;
    bucket(&t, "b1").await;
    // Re-creating an owned bucket in us-east-1 succeeds.
    bucket(&t, "b1").await;
    assert_eq!(req(&t, "HEAD", "/b1", &[], "").await.status, 200);
    let missing = req(&t, "HEAD", "/nope", &[], "").await;
    assert_eq!(missing.status, 404);
    assert!(missing.body.is_empty());

    let list = get(&t, "/").await;
    let names = texts(&xml(&list), &["Buckets", "Bucket"], "Name");
    assert_eq!(names, vec!["b1"]);
    let created = texts(&xml(&list), &["Buckets", "Bucket"], "CreationDate").remove(0);
    assert!(created.ends_with('Z') && created.len() == 24, "{created}");

    put(&t, "/b1/k", "x").await;
    let r = req(&t, "DELETE", "/b1", &[], "").await;
    assert_eq!(r.status, 409);
    assert_eq!(code(&r), "BucketNotEmpty");
    req(&t, "DELETE", "/b1/k", &[], "").await;
    assert_eq!(req(&t, "DELETE", "/b1", &[], "").await.status, 204);

    let r = get(&t, "/b1/k").await;
    assert_eq!(r.status, 404);
    let e = xml(&r);
    assert_eq!(e.child_text("Code").as_deref(), Some("NoSuchBucket"));
    assert_eq!(e.child_text("BucketName").as_deref(), Some("b1"));
    assert!(e.child_text("RequestId").is_some_and(|id| !id.is_empty()));
    assert!(r.header("x-amz-request-id").is_some());

    let r = get(&t, "/b2?location").await;
    assert_eq!(code(&r), "NoSuchBucket");
    bucket(&t, "b2").await;
    let r = get(&t, "/b2?location").await;
    assert_eq!(r.status, 200);
    assert_eq!(xml(&r).name, "LocationConstraint");
    let r = get(&t, "/b2/missing").await;
    assert_eq!(code(&r), "NoSuchKey");
    let r = req(&t, "HEAD", "/b2/missing", &[], "").await;
    assert_eq!((r.status, r.body.len()), (404, 0));
}

#[tokio::test]
async fn list_objects_v2_paging_delimiter_and_encoding() {
    let t = TestApp::new().await;
    bucket(&t, "lb").await;
    for k in ["a/1", "a/2", "b/1", "c", "d", "x y+z"] {
        put(
            &t,
            &format!("/lb/{}", k.replace(' ', "%20").replace('+', "%2B")),
            "v",
        )
        .await;
    }
    let r = get(&t, "/lb?list-type=2&delimiter=%2F&max-keys=2").await;
    let doc = xml(&r);
    assert_eq!(doc.child_text("KeyCount").as_deref(), Some("2"));
    assert_eq!(doc.child_text("IsTruncated").as_deref(), Some("true"));
    assert_eq!(texts(&doc, &["CommonPrefixes"], "Prefix"), vec!["a/", "b/"]);
    let token = doc.child_text("NextContinuationToken").unwrap();
    let r = get(
        &t,
        &format!("/lb?list-type=2&delimiter=%2F&max-keys=2&continuation-token={token}"),
    )
    .await;
    let doc = xml(&r);
    assert_eq!(texts(&doc, &["Contents"], "Key"), vec!["c", "d"]);
    let token = doc.child_text("NextContinuationToken").unwrap();
    let r = get(
        &t,
        &format!(
            "/lb?list-type=2&delimiter=%2F&max-keys=2&continuation-token={token}&encoding-type=url"
        ),
    )
    .await;
    let doc = xml(&r);
    assert_eq!(texts(&doc, &["Contents"], "Key"), vec!["x%20y%2Bz"]);
    assert_eq!(doc.child_text("IsTruncated").as_deref(), Some("false"));
    assert!(doc.child("NextContinuationToken").is_none());

    let r = get(
        &t,
        "/lb?list-type=2&prefix=a%2F&start-after=a%2F1&fetch-owner=true",
    )
    .await;
    let doc = xml(&r);
    assert_eq!(texts(&doc, &["Contents"], "Key"), vec!["a/2"]);
    assert!(doc.child("Contents").unwrap().child("Owner").is_some());
    let lm = texts(&doc, &["Contents"], "LastModified").remove(0);
    assert!(lm.ends_with('Z') && lm.len() == 24, "{lm}");

    // V1 with marker / NextMarker.
    let r = get(&t, "/lb?delimiter=%2F&max-keys=3").await;
    let doc = xml(&r);
    assert_eq!(doc.child_text("NextMarker").as_deref(), Some("c"));
    let r = get(&t, "/lb?delimiter=%2F&marker=c").await;
    assert_eq!(texts(&xml(&r), &["Contents"], "Key"), vec!["d", "x y+z"]);

    // Presigned-URL auth parameters are ignored for dispatch.
    let r = get(&t, "/lb/c?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=x&X-Amz-Signature=y&X-Amz-Date=z&X-Amz-Expires=60&X-Amz-SignedHeaders=host").await;
    assert_eq!((r.status, r.text().as_str()), (200, "v"));
}

#[tokio::test]
async fn metadata_headers_range_and_conditionals() {
    let t = TestApp::new().await;
    bucket(&t, "mb").await;
    let r = req(
        &t,
        "PUT",
        "/mb/doc.txt",
        &[
            ("content-type", "text/plain"),
            ("x-amz-meta-Owner", "alice"),
            ("content-encoding", "gzip"),
            ("content-disposition", "attachment"),
            ("content-language", "en"),
            ("cache-control", "max-age=60"),
            ("expires", "Thu, 01 Dec 2094 16:00:00 GMT"),
        ],
        "0123456789",
    )
    .await;
    assert_eq!(r.status, 200);
    let etag = r.header("etag").unwrap();
    assert_eq!(
        r.header("x-amz-server-side-encryption").as_deref(),
        Some("AES256")
    );
    assert!(r.header("x-amz-version-id").is_none());

    for method in ["GET", "HEAD"] {
        let r = req(&t, method, "/mb/doc.txt", &[], "").await;
        assert_eq!(r.status, 200);
        assert_eq!(r.header("x-amz-meta-owner").as_deref(), Some("alice"));
        assert_eq!(r.header("content-type").as_deref(), Some("text/plain"));
        assert_eq!(r.header("content-encoding").as_deref(), Some("gzip"));
        assert_eq!(
            r.header("content-disposition").as_deref(),
            Some("attachment")
        );
        assert_eq!(r.header("content-language").as_deref(), Some("en"));
        assert_eq!(r.header("cache-control").as_deref(), Some("max-age=60"));
        assert_eq!(
            r.header("expires").as_deref(),
            Some("Thu, 01 Dec 2094 16:00:00 GMT")
        );
        assert_eq!(r.header("accept-ranges").as_deref(), Some("bytes"));
        assert_eq!(r.header("etag"), Some(etag.clone()));
        assert!(r.header("last-modified").unwrap().ends_with(" GMT"));
        if method == "HEAD" {
            assert_eq!(r.header("content-length").as_deref(), Some("10"));
        } else {
            assert_eq!(r.body.len(), 10);
        }
    }

    let r = req(&t, "GET", "/mb/doc.txt", &[("range", "bytes=2-4")], "").await;
    assert_eq!((r.status, r.text().as_str()), (206, "234"));
    assert_eq!(r.header("content-range").as_deref(), Some("bytes 2-4/10"));
    let r = req(&t, "GET", "/mb/doc.txt", &[("range", "bytes=7-")], "").await;
    assert_eq!(r.text(), "789");
    let r = req(&t, "GET", "/mb/doc.txt", &[("range", "bytes=-3")], "").await;
    assert_eq!(r.text(), "789");
    let r = req(&t, "GET", "/mb/doc.txt", &[("range", "bytes=5-100")], "").await;
    assert_eq!(r.header("content-range").as_deref(), Some("bytes 5-9/10"));
    let r = req(&t, "GET", "/mb/doc.txt", &[("range", "bytes=20-30")], "").await;
    assert_eq!(r.status, 416);
    assert_eq!(code(&r), "InvalidRange");
    let r = req(&t, "GET", "/mb/doc.txt?partNumber=1", &[], "").await;
    assert_eq!(r.header("x-amz-mp-parts-count").as_deref(), Some("1"));

    let r = req(&t, "GET", "/mb/doc.txt", &[("if-none-match", &etag)], "").await;
    assert_eq!(r.status, 304);
    let r = req(&t, "GET", "/mb/doc.txt", &[("if-match", "\"nope\"")], "").await;
    assert_eq!((r.status, code(&r).as_str()), (412, "PreconditionFailed"));
    let r = req(
        &t,
        "GET",
        "/mb/doc.txt",
        &[("if-modified-since", "Thu, 01 Dec 2094 16:00:00 GMT")],
        "",
    )
    .await;
    assert_eq!(r.status, 304);
    let r = req(
        &t,
        "GET",
        "/mb/doc.txt",
        &[("if-unmodified-since", "Thu, 01 Dec 1994 16:00:00 GMT")],
        "",
    )
    .await;
    assert_eq!(r.status, 412);
    let r = req(&t, "PUT", "/mb/doc.txt", &[("if-none-match", "*")], "zzz").await;
    assert_eq!(r.status, 412);

    let r = get(
        &t,
        "/mb/doc.txt?response-content-type=application%2Fjson&response-cache-control=no-cache",
    )
    .await;
    assert_eq!(
        r.header("content-type").as_deref(),
        Some("application/json")
    );
    assert_eq!(r.header("cache-control").as_deref(), Some("no-cache"));

    let r = req(
        &t,
        "PUT",
        "/mb/bad",
        &[("content-md5", "AAAAAAAAAAAAAAAAAAAAAA==")],
        "x",
    )
    .await;
    assert_eq!(code(&r), "BadDigest");
    let r = req(
        &t,
        "PUT",
        "/mb/crc",
        &[("x-amz-checksum-crc32", "AAAAAA==")],
        "x",
    )
    .await;
    assert_eq!(code(&r), "BadDigest");
}

#[tokio::test]
async fn object_and_bucket_tagging() {
    let t = TestApp::new().await;
    bucket(&t, "tb").await;
    req(
        &t,
        "PUT",
        "/tb/k",
        &[("x-amz-tagging", "env=dev&team=a%20b")],
        "x",
    )
    .await;
    let r = get(&t, "/tb/k").await;
    assert_eq!(r.header("x-amz-tagging-count").as_deref(), Some("2"));
    let r = get(&t, "/tb/k?tagging").await;
    let doc = xml(&r);
    assert_eq!(texts(&doc, &["TagSet", "Tag"], "Value"), vec!["dev", "a b"]);

    let body = "<Tagging><TagSet><Tag><Key>x</Key><Value>1</Value></Tag></TagSet></Tagging>";
    assert_eq!(put(&t, "/tb/k?tagging", body).await.status, 200);
    assert_eq!(
        texts(
            &xml(&get(&t, "/tb/k?tagging").await),
            &["TagSet", "Tag"],
            "Key"
        ),
        vec!["x"]
    );
    assert_eq!(
        req(&t, "DELETE", "/tb/k?tagging", &[], "").await.status,
        204
    );
    assert!(texts(
        &xml(&get(&t, "/tb/k?tagging").await),
        &["TagSet", "Tag"],
        "Key"
    )
    .is_empty());
    assert_eq!(
        code(&put(&t, "/tb/k?tagging", "<Tagging><TagSet>").await),
        "MalformedXML"
    );

    let r = get(&t, "/tb?tagging").await;
    assert_eq!((r.status, code(&r).as_str()), (404, "NoSuchTagSet"));
    assert_eq!(put(&t, "/tb?tagging", body).await.status, 204);
    assert_eq!(
        texts(
            &xml(&get(&t, "/tb?tagging").await),
            &["TagSet", "Tag"],
            "Key"
        ),
        vec!["x"]
    );
    assert_eq!(req(&t, "DELETE", "/tb?tagging", &[], "").await.status, 204);
    assert_eq!(get(&t, "/tb?tagging").await.status, 404);
}

#[tokio::test]
async fn copy_object_and_delete_objects() {
    let t = TestApp::new().await;
    bucket(&t, "cb").await;
    req(
        &t,
        "PUT",
        "/cb/src%20file.txt",
        &[
            ("content-type", "text/plain"),
            ("x-amz-meta-a", "1"),
            ("x-amz-tagging", "t=1"),
        ],
        "hello",
    )
    .await;
    let r = req(
        &t,
        "PUT",
        "/cb/copy1",
        &[("x-amz-copy-source", "/cb/src%20file.txt")],
        "",
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text());
    let doc = xml(&r);
    assert_eq!(doc.name, "CopyObjectResult");
    assert_eq!(
        doc.child_text("ETag").unwrap(),
        crate::aws::s3::service::md5_etag(b"hello")
    );
    assert!(doc.child_text("LastModified").unwrap().ends_with('Z'));
    let g = get(&t, "/cb/copy1").await;
    assert_eq!(g.text(), "hello");
    assert_eq!(g.header("x-amz-meta-a").as_deref(), Some("1"));
    assert_eq!(g.header("content-type").as_deref(), Some("text/plain"));
    assert_eq!(g.header("x-amz-tagging-count").as_deref(), Some("1"));

    let r = req(
        &t,
        "PUT",
        "/cb/copy2",
        &[
            ("x-amz-copy-source", "cb/src%20file.txt"),
            ("x-amz-metadata-directive", "REPLACE"),
            ("x-amz-meta-b", "2"),
            ("content-type", "application/json"),
            ("x-amz-tagging-directive", "REPLACE"),
        ],
        "",
    )
    .await;
    assert_eq!(r.status, 200);
    let g = get(&t, "/cb/copy2").await;
    assert_eq!(g.header("x-amz-meta-a"), None);
    assert_eq!(g.header("x-amz-meta-b").as_deref(), Some("2"));
    assert_eq!(
        g.header("content-type").as_deref(),
        Some("application/json")
    );
    assert_eq!(g.header("x-amz-tagging-count"), None);

    let r = req(
        &t,
        "PUT",
        "/cb/copy2",
        &[("x-amz-copy-source", "/cb/copy2")],
        "",
    )
    .await;
    assert_eq!(code(&r), "InvalidRequest");
    let r = req(
        &t,
        "PUT",
        "/cb/copy3",
        &[("x-amz-copy-source", "/cb/missing")],
        "",
    )
    .await;
    assert_eq!(code(&r), "NoSuchKey");

    let body = "<Delete><Quiet>false</Quiet><Object><Key>copy1</Key></Object><Object><Key>copy2</Key></Object><Object><Key>ghost</Key></Object></Delete>";
    let r = req(
        &t,
        "POST",
        "/cb?delete",
        &[("content-md5", "ignored")],
        body,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(
        texts(&xml(&r), &["Deleted"], "Key"),
        vec!["copy1", "copy2", "ghost"]
    );
    assert_eq!(get(&t, "/cb/copy1").await.status, 404);
    let r = req(
        &t,
        "POST",
        "/cb?delete",
        &[],
        "<Delete><Quiet>true</Quiet><Object><Key>src file.txt</Key></Object></Delete>",
    )
    .await;
    assert!(xml(&r).children.is_empty());
    assert_eq!(
        code(&req(&t, "POST", "/cb?delete", &[], "<oops").await),
        "MalformedXML"
    );
}

#[tokio::test]
async fn versioning_lifecycle() {
    let t = TestApp::new().await;
    bucket(&t, "vb").await;
    let r = get(&t, "/vb?versioning").await;
    assert!(xml(&r).children.is_empty());
    put(&t, "/vb/pre", "null-version").await;
    let cfg = "<VersioningConfiguration><Status>Enabled</Status></VersioningConfiguration>";
    assert_eq!(put(&t, "/vb?versioning", cfg).await.status, 200);
    assert_eq!(
        xml(&get(&t, "/vb?versioning").await)
            .child_text("Status")
            .as_deref(),
        Some("Enabled")
    );
    assert_eq!(
        code(
            &put(
                &t,
                "/vb?versioning",
                "<VersioningConfiguration><Status>On</Status></VersioningConfiguration>"
            )
            .await
        ),
        "MalformedXML"
    );

    let v1 = put(&t, "/vb/k", "one")
        .await
        .header("x-amz-version-id")
        .unwrap();
    let v2 = put(&t, "/vb/k", "two")
        .await
        .header("x-amz-version-id")
        .unwrap();
    assert_ne!(v1, v2);
    let r = get(&t, "/vb/k").await;
    assert_eq!(
        (r.text().as_str(), r.header("x-amz-version-id")),
        ("two", Some(v2.clone()))
    );
    assert_eq!(
        get(&t, &format!("/vb/k?versionId={v1}")).await.text(),
        "one"
    );
    assert_eq!(
        req(&t, "HEAD", &format!("/vb/k?versionId={v1}"), &[], "")
            .await
            .header("x-amz-version-id"),
        Some(v1.clone())
    );
    assert_eq!(
        get(&t, "/vb/pre")
            .await
            .header("x-amz-version-id")
            .as_deref(),
        Some("null")
    );

    let d = req(&t, "DELETE", "/vb/k", &[], "").await;
    assert_eq!(d.status, 204);
    assert_eq!(d.header("x-amz-delete-marker").as_deref(), Some("true"));
    let marker = d.header("x-amz-version-id").unwrap();
    let r = get(&t, "/vb/k").await;
    assert_eq!((r.status, code(&r).as_str()), (404, "NoSuchKey"));
    assert_eq!(r.header("x-amz-delete-marker").as_deref(), Some("true"));
    let r = get(&t, &format!("/vb/k?versionId={marker}")).await;
    assert_eq!(r.status, 405);

    let r = get(&t, "/vb?versions").await;
    let doc = xml(&r);
    assert_eq!(
        texts(&doc, &["Version"], "VersionId"),
        vec![v2.clone(), v1.clone(), "null".into()]
    );
    assert_eq!(
        texts(&doc, &["DeleteMarker"], "VersionId"),
        vec![marker.clone()]
    );
    assert_eq!(texts(&doc, &["DeleteMarker"], "IsLatest"), vec!["true"]);
    assert_eq!(
        texts(&doc, &["Version"], "IsLatest"),
        vec!["false", "false", "true"]
    );

    // Paging through versions.
    let r = get(&t, "/vb?versions&max-keys=2").await;
    let doc = xml(&r);
    assert_eq!(doc.child_text("IsTruncated").as_deref(), Some("true"));
    let (km, vm) = (
        doc.child_text("NextKeyMarker").unwrap(),
        doc.child_text("NextVersionIdMarker").unwrap(),
    );
    let r = get(
        &t,
        &format!("/vb?versions&key-marker={km}&version-id-marker={vm}"),
    )
    .await;
    assert_eq!(
        texts(&xml(&r), &["Version"], "VersionId"),
        vec![v1.clone(), "null".into()]
    );

    // Removing the delete marker restores the previous version.
    let d = req(&t, "DELETE", &format!("/vb/k?versionId={marker}"), &[], "").await;
    assert_eq!(d.header("x-amz-delete-marker").as_deref(), Some("true"));
    assert_eq!(get(&t, "/vb/k").await.text(), "two");
    // Deleting the latest version makes the previous one current.
    req(&t, "DELETE", &format!("/vb/k?versionId={v2}"), &[], "").await;
    assert_eq!(get(&t, "/vb/k").await.text(), "one");

    // Suspended: writes get version "null" and replace the null version.
    put(
        &t,
        "/vb?versioning",
        "<VersioningConfiguration><Status>Suspended</Status></VersioningConfiguration>",
    )
    .await;
    assert_eq!(
        put(&t, "/vb/k", "s1")
            .await
            .header("x-amz-version-id")
            .as_deref(),
        Some("null")
    );
    put(&t, "/vb/k", "s2").await;
    let r = get(&t, "/vb?versions&prefix=k").await;
    assert_eq!(
        texts(&xml(&r), &["Version"], "VersionId"),
        vec!["null".to_string(), v1.clone()]
    );
    assert_eq!(get(&t, "/vb/k?versionId=null").await.text(), "s2");
    let r = req(&t, "DELETE", "/vb", &[], "").await;
    assert_eq!(code(&r), "BucketNotEmpty");
}

#[tokio::test]
async fn server_side_encryption_headers_and_defaults() {
    let t = TestApp::new().await;
    bucket(&t, "eb").await;
    let r = get(&t, "/eb?encryption").await;
    assert_eq!(
        (r.status, code(&r).as_str()),
        (404, "ServerSideEncryptionConfigurationNotFoundError")
    );
    let r = req(
        &t,
        "PUT",
        "/eb/kms",
        &[
            ("x-amz-server-side-encryption", "aws:kms"),
            (
                "x-amz-server-side-encryption-aws-kms-key-id",
                "arn:aws:kms:us-east-1:000000000000:key/abc",
            ),
            ("x-amz-server-side-encryption-bucket-key-enabled", "true"),
        ],
        "x",
    )
    .await;
    assert_eq!(
        r.header("x-amz-server-side-encryption").as_deref(),
        Some("aws:kms")
    );
    assert_eq!(
        r.header("x-amz-server-side-encryption-bucket-key-enabled")
            .as_deref(),
        Some("true")
    );
    for m in ["GET", "HEAD"] {
        let g = req(&t, m, "/eb/kms", &[], "").await;
        assert_eq!(
            g.header("x-amz-server-side-encryption").as_deref(),
            Some("aws:kms")
        );
        assert_eq!(
            g.header("x-amz-server-side-encryption-aws-kms-key-id")
                .as_deref(),
            Some("arn:aws:kms:us-east-1:000000000000:key/abc")
        );
    }
    let r = req(
        &t,
        "PUT",
        "/eb/bad",
        &[("x-amz-server-side-encryption", "rot13")],
        "x",
    )
    .await;
    assert_eq!(code(&r), "InvalidArgument");

    let cfg = "<ServerSideEncryptionConfiguration><Rule><ApplyServerSideEncryptionByDefault><SSEAlgorithm>aws:kms</SSEAlgorithm><KMSMasterKeyID>alias/app</KMSMasterKeyID></ApplyServerSideEncryptionByDefault><BucketKeyEnabled>true</BucketKeyEnabled></Rule></ServerSideEncryptionConfiguration>";
    assert_eq!(put(&t, "/eb?encryption", cfg).await.status, 200);
    assert_eq!(
        xml(&get(&t, "/eb?encryption").await).name,
        "ServerSideEncryptionConfiguration"
    );
    let r = put(&t, "/eb/default", "x").await;
    assert_eq!(
        r.header("x-amz-server-side-encryption").as_deref(),
        Some("aws:kms")
    );
    assert_eq!(
        r.header("x-amz-server-side-encryption-aws-kms-key-id")
            .as_deref(),
        Some("alias/app")
    );
    let r = req(
        &t,
        "PUT",
        "/eb/copy",
        &[("x-amz-copy-source", "/eb/kms")],
        "",
    )
    .await;
    assert_eq!(
        r.header("x-amz-server-side-encryption").as_deref(),
        Some("aws:kms")
    );
    assert_eq!(
        req(&t, "DELETE", "/eb?encryption", &[], "").await.status,
        204
    );
    assert_eq!(
        put(&t, "/eb/plain", "x")
            .await
            .header("x-amz-server-side-encryption")
            .as_deref(),
        Some("AES256")
    );
}

#[tokio::test]
async fn bucket_configuration_documents() {
    let t = TestApp::new().await;
    bucket(&t, "cfg").await;
    let cases = [
        ("publicAccessBlock", "NoSuchPublicAccessBlockConfiguration", "<PublicAccessBlockConfiguration><BlockPublicAcls>true</BlockPublicAcls></PublicAccessBlockConfiguration>"),
        ("lifecycle", "NoSuchLifecycleConfiguration", "<LifecycleConfiguration><Rule><ID>r</ID><Filter><Prefix>tmp/</Prefix></Filter><Status>Enabled</Status><Expiration><Days>1</Days></Expiration></Rule></LifecycleConfiguration>"),
        ("ownershipControls", "OwnershipControlsNotFoundError", "<OwnershipControls><Rule><ObjectOwnership>BucketOwnerEnforced</ObjectOwnership></Rule></OwnershipControls>"),
        ("policy", "NoSuchBucketPolicy", r#"{"Version":"2012-10-17","Statement":[]}"#),
    ];
    for (param, missing, body) in cases {
        let r = get(&t, &format!("/cfg?{param}")).await;
        assert_eq!((r.status, code(&r)), (404, missing.to_string()), "{param}");
        assert!(
            put(&t, &format!("/cfg?{param}"), body).await.status < 300,
            "{param}"
        );
        let r = get(&t, &format!("/cfg?{param}")).await;
        assert_eq!((r.status, r.text()), (200, body.to_string()), "{param}");
        assert_eq!(
            req(&t, "DELETE", &format!("/cfg?{param}"), &[], "")
                .await
                .status,
            204
        );
        assert_eq!(get(&t, &format!("/cfg?{param}")).await.status, 404);
    }
    assert_eq!(
        code(&put(&t, "/cfg?policy", "not json").await),
        "MalformedPolicy"
    );
    assert_eq!(code(&put(&t, "/cfg?lifecycle", "<LifecycleConfiguration><Rule><Status>Maybe</Status></Rule></LifecycleConfiguration>").await), "MalformedXML");
    assert_eq!(
        code(&put(&t, "/cfg?publicAccessBlock", "<Wrong/>").await),
        "MalformedXML"
    );

    let r = get(&t, "/cfg?acl").await;
    assert_eq!(
        texts(&xml(&r), &["AccessControlList", "Grant"], "Permission"),
        vec!["FULL_CONTROL"]
    );
    assert_eq!(
        req(&t, "PUT", "/cfg?acl", &[("x-amz-acl", "private")], "")
            .await
            .status,
        200
    );
    put(&t, "/cfg/o", "x").await;
    assert_eq!(get(&t, "/cfg/o?acl").await.status, 200);
}

#[tokio::test]
async fn event_notifications_to_sqs() {
    let t = TestApp::new().await;
    bucket(&t, "nb").await;
    let url = create_queue(&t, "s3-events").await;
    let arn = "arn:aws:sqs:us-east-1:000000000000:s3-events";

    let bad = "<NotificationConfiguration><QueueConfiguration><Queue>arn:aws:sqs:us-east-1:000000000000:missing</Queue><Event>s3:ObjectCreated:*</Event></QueueConfiguration></NotificationConfiguration>";
    let r = put(&t, "/nb?notification", bad).await;
    assert_eq!((r.status, code(&r).as_str()), (400, "InvalidArgument"));
    assert!(r
        .text()
        .contains("Unable to validate the following destination configurations"));

    let cfg = format!(
        "<NotificationConfiguration><QueueConfiguration><Id>uploads</Id><Queue>{arn}</Queue><Event>s3:ObjectCreated:*</Event><Event>s3:ObjectRemoved:DeleteMarkerCreated</Event><Event>s3:ObjectRemoved:Delete</Event>\
<Filter><S3Key><FilterRule><Name>prefix</Name><Value>in/</Value></FilterRule><FilterRule><Name>suffix</Name><Value>.txt</Value></FilterRule></S3Key></Filter></QueueConfiguration>\
<CloudFunctionConfiguration><CloudFunction>arn:aws:lambda:us-east-1:000000000000:function:f</CloudFunction><Event>s3:ObjectCreated:Put</Event></CloudFunctionConfiguration></NotificationConfiguration>"
    );
    assert_eq!(put(&t, "/nb?notification", &cfg).await.status, 200);
    let msgs = receive(&t, &url).await;
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["Event"], "s3:TestEvent");
    assert_eq!(msgs[0]["Service"], "Amazon S3");
    assert_eq!(msgs[0]["Bucket"], "nb");

    let doc = xml(&get(&t, "/nb?notification").await);
    assert_eq!(texts(&doc, &["QueueConfiguration"], "Id"), vec!["uploads"]);
    assert_eq!(doc.children_named("CloudFunctionConfiguration").count(), 1);

    put(&t, "/nb/in/a%20b.txt", "hello").await;
    put(&t, "/nb/out/skip.txt", "x").await;
    put(&t, "/nb/in/skip.bin", "x").await;
    let msgs = receive(&t, &url).await;
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    let rec = &msgs[0]["Records"][0];
    assert_eq!(rec["eventName"], "ObjectCreated:Put");
    assert_eq!(rec["eventSource"], "aws:s3");
    assert_eq!(rec["s3"]["configurationId"], "uploads");
    assert_eq!(rec["s3"]["bucket"]["arn"], "arn:aws:s3:::nb");
    assert_eq!(rec["s3"]["object"]["key"], "in/a+b.txt");
    assert_eq!(rec["s3"]["object"]["size"], 5);
    assert_eq!(
        rec["s3"]["object"]["eTag"],
        "5d41402abc4b2a76b9719d911017c592"
    );
    assert!(rec["s3"]["object"].get("versionId").is_none());

    req(
        &t,
        "PUT",
        "/nb/in/c.txt",
        &[("x-amz-copy-source", "/nb/in/a%20b.txt")],
        "",
    )
    .await;
    req(&t, "DELETE", "/nb/in/c.txt", &[], "").await;
    let names: Vec<String> = receive(&t, &url)
        .await
        .iter()
        .map(|m| m["Records"][0]["eventName"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, vec!["ObjectCreated:Copy", "ObjectRemoved:Delete"]);

    put(
        &t,
        "/nb?versioning",
        "<VersioningConfiguration><Status>Enabled</Status></VersioningConfiguration>",
    )
    .await;
    let v = put(&t, "/nb/in/v.txt", "x")
        .await
        .header("x-amz-version-id")
        .unwrap();
    req(&t, "DELETE", "/nb/in/v.txt", &[], "").await;
    let msgs = receive(&t, &url).await;
    assert_eq!(
        msgs[0]["Records"][0]["s3"]["object"]["versionId"],
        v.as_str()
    );
    assert_eq!(
        msgs[1]["Records"][0]["eventName"],
        "ObjectRemoved:DeleteMarkerCreated"
    );
    assert!(msgs[1]["Records"][0]["s3"]["object"].get("size").is_none());
    let s1 = msgs[0]["Records"][0]["s3"]["object"]["sequencer"]
        .as_str()
        .unwrap()
        .to_string();
    let s2 = msgs[1]["Records"][0]["s3"]["object"]["sequencer"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(s2 > s1);

    // Internal writes (e.g. SES receipt rules) notify too.
    crate::aws::s3::put_object_internal(
        &t.state,
        "nb",
        "in/internal.txt",
        b"i".to_vec(),
        "text/plain",
        vec![("x-amz-meta-src".into(), "ses".into())],
    )
    .await
    .unwrap();
    assert_eq!(receive(&t, &url).await.len(), 1);
    assert_eq!(
        get(&t, "/nb/in/internal.txt")
            .await
            .header("x-amz-meta-src")
            .as_deref(),
        Some("ses")
    );
}

#[tokio::test]
async fn notifications_to_sns_topic() {
    let t = TestApp::new().await;
    bucket(&t, "sb").await;
    let r = t
        .call(aws_query(&[
            ("Action", "CreateTopic"),
            ("Name", "s3-topic"),
        ]))
        .await;
    assert_eq!(r.status, 200);
    let missing = "<NotificationConfiguration><TopicConfiguration><Topic>arn:aws:sns:us-east-1:000000000000:nope</Topic><Event>s3:ObjectCreated:Put</Event></TopicConfiguration></NotificationConfiguration>";
    assert_eq!(
        code(&put(&t, "/sb?notification", missing).await),
        "InvalidArgument"
    );
    let arn = "arn:aws:sns:us-east-1:000000000000:s3-topic";
    let cfg = format!("<NotificationConfiguration><TopicConfiguration><Topic>{arn}</Topic><Event>s3:ObjectCreated:Put</Event></TopicConfiguration></NotificationConfiguration>");
    assert_eq!(put(&t, "/sb?notification", &cfg).await.status, 200);
    put(&t, "/sb/k", "x").await;
    let bodies: Vec<(String, Option<String>)> = t
        .state
        .db
        .call(|c| {
            let mut s = c.prepare("SELECT body, subject FROM sns_messages ORDER BY id")?;
            let rows = s
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap();
    assert_eq!(bodies.len(), 2);
    assert!(bodies[0].0.contains("s3:TestEvent"));
    assert!(bodies[1].0.contains("ObjectCreated:Put"));
    assert_eq!(bodies[1].1.as_deref(), Some("Amazon S3 Notification"));
}

#[tokio::test]
async fn multipart_upload_flow() {
    let t = TestApp::new().await;
    bucket(&t, "mp").await;
    let r = req(
        &t,
        "POST",
        "/mp/big.bin?uploads",
        &[
            ("content-type", "application/zip"),
            ("x-amz-meta-k", "v"),
            ("x-amz-tagging", "a=b"),
        ],
        "",
    )
    .await;
    assert_eq!(r.status, 200);
    let id = xml(&r).child_text("UploadId").unwrap();
    let p1 = put(
        &t,
        &format!("/mp/big.bin?partNumber=1&uploadId={id}"),
        "aaaa",
    )
    .await
    .header("etag")
    .unwrap();
    let p2 = put(&t, &format!("/mp/big.bin?partNumber=2&uploadId={id}"), "bb")
        .await
        .header("etag")
        .unwrap();
    put(&t, "/mp/src", "0123456789").await;
    let r = req(
        &t,
        "PUT",
        &format!("/mp/big.bin?partNumber=3&uploadId={id}"),
        &[
            ("x-amz-copy-source", "/mp/src"),
            ("x-amz-copy-source-range", "bytes=2-4"),
        ],
        "",
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text());
    let p3 = xml(&r).child_text("ETag").unwrap();

    let r = get(&t, &format!("/mp/big.bin?uploadId={id}")).await;
    assert_eq!(texts(&xml(&r), &["Part"], "Size"), vec!["4", "2", "3"]);
    let r = get(&t, "/mp?uploads").await;
    assert_eq!(texts(&xml(&r), &["Upload"], "UploadId"), vec![id.clone()]);

    let complete = |parts: &[(i32, &str)]| {
        let inner: String = parts
            .iter()
            .map(|(n, e)| format!("<Part><PartNumber>{n}</PartNumber><ETag>{e}</ETag></Part>"))
            .collect();
        format!("<CompleteMultipartUpload>{inner}</CompleteMultipartUpload>")
    };
    let r = req(
        &t,
        "POST",
        &format!("/mp/big.bin?uploadId={id}"),
        &[],
        complete(&[(2, &p2), (1, &p1)]),
    )
    .await;
    assert_eq!(code(&r), "InvalidPartOrder");
    let r = req(
        &t,
        "POST",
        &format!("/mp/big.bin?uploadId={id}"),
        &[],
        complete(&[(1, "\"bad\"")]),
    )
    .await;
    assert_eq!(code(&r), "InvalidPart");
    let r = req(
        &t,
        "POST",
        &format!("/mp/big.bin?uploadId={id}"),
        &[],
        complete(&[(1, &p1), (2, &p2), (3, &p3)]),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text());
    let etag = xml(&r).child_text("ETag").unwrap();
    use md5::Digest;
    let mut cat = Vec::new();
    for part in [&b"aaaa"[..], b"bb", b"234"] {
        cat.extend_from_slice(&md5::Md5::digest(part));
    }
    assert_eq!(
        etag,
        format!("\"{}-3\"", hex::encode(md5::Md5::digest(&cat)))
    );

    let g = get(&t, "/mp/big.bin").await;
    assert_eq!(g.text(), "aaaabb234");
    assert_eq!(g.header("content-type").as_deref(), Some("application/zip"));
    assert_eq!(g.header("x-amz-meta-k").as_deref(), Some("v"));
    assert_eq!(g.header("x-amz-tagging-count").as_deref(), Some("1"));
    let g = get(&t, "/mp/big.bin?partNumber=2").await;
    assert_eq!((g.status, g.text().as_str()), (206, "bb"));
    assert_eq!(g.header("x-amz-mp-parts-count").as_deref(), Some("3"));
    let a = xml(&get(&t, "/mp/big.bin?attributes").await);
    assert_eq!(
        a.path(&["ObjectParts"])
            .unwrap()
            .child_text("PartsCount")
            .as_deref(),
        Some("3")
    );
    assert_eq!(a.child_text("ObjectSize").as_deref(), Some("9"));

    let r = get(&t, &format!("/mp/big.bin?uploadId={id}")).await;
    assert_eq!((r.status, code(&r).as_str()), (404, "NoSuchUpload"));
    let r = req(&t, "POST", "/mp/x?uploads", &[], "").await;
    let id2 = xml(&r).child_text("UploadId").unwrap();
    assert_eq!(
        req(&t, "DELETE", &format!("/mp/x?uploadId={id2}"), &[], "")
            .await
            .status,
        204
    );
    let r = put(&t, &format!("/mp/x?partNumber=1&uploadId={id2}"), "z").await;
    assert_eq!(code(&r), "NoSuchUpload");
}

#[tokio::test]
async fn cors_preflight_and_response_headers() {
    let t = TestApp::new().await;
    bucket(&t, "web").await;
    let pre = |origin: &'static str, method: &'static str, hdrs: &'static str| {
        raw(
            "OPTIONS",
            "/web/file.txt",
            &[
                ("origin", origin),
                ("access-control-request-method", method),
                ("access-control-request-headers", hdrs),
            ],
            "",
        )
    };
    let r = t
        .call(pre("http://app.example.com", "PUT", "content-type"))
        .await;
    assert_eq!((r.status, code(&r).as_str()), (403, "CORSResponse"));
    assert_eq!(get(&t, "/web?cors").await.status, 404);

    let cfg = "<CORSConfiguration><CORSRule><AllowedOrigin>http://*.example.com</AllowedOrigin><AllowedMethod>GET</AllowedMethod><AllowedMethod>PUT</AllowedMethod><AllowedHeader>*</AllowedHeader><ExposeHeader>ETag</ExposeHeader><MaxAgeSeconds>300</MaxAgeSeconds></CORSRule>\
<CORSRule><AllowedOrigin>*</AllowedOrigin><AllowedMethod>GET</AllowedMethod></CORSRule></CORSConfiguration>";
    assert_eq!(put(&t, "/web?cors", cfg).await.status, 200);
    assert_eq!(code(&put(&t, "/web?cors", "<CORSConfiguration><CORSRule><AllowedOrigin>*</AllowedOrigin><AllowedMethod>PATCH</AllowedMethod></CORSRule></CORSConfiguration>").await), "InvalidRequest");
    assert_eq!(get(&t, "/web?cors").await.text(), cfg);

    let r = t
        .call(pre(
            "http://app.example.com",
            "PUT",
            "Content-Type, X-Amz-Date",
        ))
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(
        r.header("access-control-allow-origin").as_deref(),
        Some("http://app.example.com")
    );
    assert_eq!(
        r.header("access-control-allow-methods").as_deref(),
        Some("GET, PUT")
    );
    assert_eq!(
        r.header("access-control-allow-headers").as_deref(),
        Some("content-type, x-amz-date")
    );
    assert_eq!(r.header("access-control-max-age").as_deref(), Some("300"));
    assert_eq!(
        r.header("access-control-expose-headers").as_deref(),
        Some("ETag")
    );
    assert!(r.header("vary").unwrap().contains("Origin"));

    let r = t.call(pre("http://evil.com", "PUT", "")).await;
    assert_eq!(r.status, 403);
    let r = t.call(pre("http://evil.com", "GET", "")).await;
    assert_eq!(
        r.header("access-control-allow-origin").as_deref(),
        Some("*")
    );
    let r = t
        .call(raw(
            "OPTIONS",
            "/web",
            &[
                ("origin", "http://a.example.com"),
                ("access-control-request-method", "GET"),
            ],
            "",
        ))
        .await;
    assert_eq!(r.status, 200);

    put(&t, "/web/file.txt", "x").await;
    let r = req(
        &t,
        "GET",
        "/web/file.txt",
        &[("origin", "http://app.example.com")],
        "",
    )
    .await;
    assert_eq!(
        r.header("access-control-allow-origin").as_deref(),
        Some("http://app.example.com")
    );
    assert_eq!(
        r.header("access-control-expose-headers").as_deref(),
        Some("ETag")
    );
    let r = get(&t, "/web/file.txt").await;
    assert!(r.header("access-control-allow-origin").is_none());

    assert_eq!(req(&t, "DELETE", "/web?cors", &[], "").await.status, 204);
    assert_eq!(code(&get(&t, "/web?cors").await), "NoSuchCORSConfiguration");
}

fn form_body(fields: &[(&str, &str)], filename: &str, content: &str) -> (String, String) {
    let boundary = "----cloudtwinboundary";
    let mut b = String::new();
    for (k, v) in fields {
        b.push_str(&format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n"
        ));
    }
    b.push_str(&format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n{content}\r\n--{boundary}--\r\n"
    ));
    (format!("multipart/form-data; boundary={boundary}"), b)
}

fn policy(conditions: Value, expires_in_secs: i64) -> String {
    use base64::Engine;
    let exp = (chrono::Utc::now() + chrono::Duration::seconds(expires_in_secs))
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    base64::engine::general_purpose::STANDARD
        .encode(json!({"expiration": exp, "conditions": conditions}).to_string())
}

#[tokio::test]
async fn browser_post_policy_uploads() {
    let t = TestApp::new().await;
    bucket(&t, "up").await;
    let url = create_queue(&t, "posts").await;
    put(&t, "/up?notification", "<NotificationConfiguration><QueueConfiguration><Queue>posts</Queue><Event>s3:ObjectCreated:Post</Event></QueueConfiguration></NotificationConfiguration>").await;
    receive(&t, &url).await;

    let p = policy(
        json!([{"bucket": "up"}, ["starts-with", "$key", "uploads/"], ["content-length-range", 1, 10], {"success_action_status": "201"}]),
        300,
    );
    let (ct, body) = form_body(
        &[
            ("key", "uploads/${filename}"),
            ("policy", &p),
            ("x-amz-signature", "sig"),
            ("Content-Type", "text/plain"),
            ("x-amz-meta-from", "browser"),
            ("success_action_status", "201"),
            (
                "tagging",
                "<Tagging><TagSet><Tag><Key>src</Key><Value>web</Value></Tag></TagSet></Tagging>",
            ),
        ],
        "photo.txt",
        "hello",
    );
    let r = req(&t, "POST", "/up", &[("content-type", &ct)], body).await;
    assert_eq!(r.status, 201, "{}", r.text());
    let doc = xml(&r);
    assert_eq!(doc.name, "PostResponse");
    assert_eq!(doc.child_text("Key").as_deref(), Some("uploads/photo.txt"));
    let g = get(&t, "/up/uploads/photo.txt").await;
    assert_eq!(g.text(), "hello");
    assert_eq!(g.header("content-type").as_deref(), Some("text/plain"));
    assert_eq!(g.header("x-amz-meta-from").as_deref(), Some("browser"));
    assert_eq!(g.header("x-amz-tagging-count").as_deref(), Some("1"));
    let msgs = receive(&t, &url).await;
    assert_eq!(msgs[0]["Records"][0]["eventName"], "ObjectCreated:Post");

    let (ct, body) = form_body(
        &[
            ("key", "uploads/big"),
            ("policy", &p),
            ("success_action_status", "201"),
        ],
        "f",
        "this is too long",
    );
    let r = req(&t, "POST", "/up", &[("content-type", &ct)], body).await;
    assert_eq!(code(&r), "EntityTooLarge");
    let (ct, body) = form_body(
        &[
            ("key", "other/x"),
            ("policy", &p),
            ("success_action_status", "201"),
        ],
        "f",
        "ok",
    );
    let r = req(&t, "POST", "/up", &[("content-type", &ct)], body).await;
    assert_eq!((r.status, code(&r).as_str()), (403, "AccessDenied"));
    let expired = policy(json!([]), -60);
    let (ct, body) = form_body(&[("key", "uploads/x"), ("policy", &expired)], "f", "ok");
    let r = req(&t, "POST", "/up", &[("content-type", &ct)], body).await;
    assert!(r.text().contains("Policy expired"));

    let (ct, body) = form_body(
        &[
            ("key", "plain.txt"),
            ("success_action_redirect", "http://localhost/done"),
        ],
        "f",
        "ok",
    );
    let r = req(&t, "POST", "/up", &[("content-type", &ct)], body).await;
    assert_eq!(r.status, 303);
    assert!(r
        .header("location")
        .unwrap()
        .starts_with("http://localhost/done?bucket=up&key=plain.txt&etag=%22"));
    let (ct, body) = form_body(&[("key", "default.txt")], "f", "ok");
    assert_eq!(
        req(&t, "POST", "/up", &[("content-type", &ct)], body)
            .await
            .status,
        204
    );
}

#[tokio::test]
async fn aws_chunked_streaming_uploads() {
    let t = TestApp::new().await;
    bucket(&t, "ch").await;
    use base64::Engine;
    let crc = base64::engine::general_purpose::STANDARD
        .encode(crc32fast::hash(b"hello world").to_be_bytes());
    let body = format!("5\r\nhello\r\n6\r\n world\r\n0\r\nx-amz-checksum-crc32:{crc}\r\n\r\n");
    let r = req(
        &t,
        "PUT",
        "/ch/s.txt",
        &[
            ("content-encoding", "aws-chunked"),
            ("x-amz-content-sha256", "STREAMING-UNSIGNED-PAYLOAD-TRAILER"),
            ("x-amz-decoded-content-length", "11"),
            ("x-amz-trailer", "x-amz-checksum-crc32"),
            ("content-type", "text/plain"),
        ],
        body,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(r.header("x-amz-checksum-crc32"), Some(crc.clone()));
    let g = req(
        &t,
        "GET",
        "/ch/s.txt",
        &[("x-amz-checksum-mode", "ENABLED")],
        "",
    )
    .await;
    assert_eq!(g.text(), "hello world");
    assert!(g.header("content-encoding").is_none());
    assert_eq!(g.header("x-amz-checksum-crc32"), Some(crc));

    let signed = "3;chunk-signature=aaaa\r\nabc\r\n0;chunk-signature=bbbb\r\n\r\n";
    let r = req(
        &t,
        "PUT",
        "/ch/signed",
        &[
            ("x-amz-content-sha256", "STREAMING-AWS4-HMAC-SHA256-PAYLOAD"),
            ("content-encoding", "aws-chunked,gzip"),
        ],
        signed,
    )
    .await;
    assert_eq!(r.status, 200);
    let g = get(&t, "/ch/signed").await;
    assert_eq!(
        (g.text().as_str(), g.header("content-encoding").as_deref()),
        ("abc", Some("gzip"))
    );

    let r = req(&t, "POST", "/ch/m?uploads", &[], "").await;
    let id = xml(&r).child_text("UploadId").unwrap();
    let r = req(
        &t,
        "PUT",
        &format!("/ch/m?partNumber=1&uploadId={id}"),
        &[("x-amz-content-sha256", "STREAMING-UNSIGNED-PAYLOAD-TRAILER")],
        "2\r\nhi\r\n0\r\n\r\n",
    )
    .await;
    assert_eq!(
        r.header("etag").unwrap(),
        crate::aws::s3::service::md5_etag(b"hi")
    );
}

#[tokio::test]
async fn virtual_hosted_style_addressing() {
    let t = TestApp::new().await;
    bucket(&t, "vh").await;
    let r = t
        .call(raw(
            "PUT",
            "/a/b.txt",
            &[("host", "vh.s3.localhost:4793")],
            "virtual",
        ))
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(get(&t, "/vh/a/b.txt").await.text(), "virtual");
    let r = t
        .call(raw(
            "GET",
            "/?list-type=2",
            &[("host", "vh.s3.localhost")],
            "",
        ))
        .await;
    assert_eq!(texts(&xml(&r), &["Contents"], "Key"), vec!["a/b.txt"]);
}

#[tokio::test]
async fn seed_is_idempotent_and_configures_buckets() {
    let t = TestApp::new().await;
    let url = create_queue(&t, "seeded-q").await;
    let doc = json!({"buckets": [
        "plain",
        {"name": "rich", "versioning": true,
         "cors": [{"AllowedOrigins": ["*"], "AllowedMethods": ["GET", "PUT"], "AllowedHeaders": ["*"], "MaxAgeSeconds": 60}],
         "notifications": {"queues": [{"queue": "seeded-q", "events": ["s3:ObjectCreated:*"], "suffix": ".json"}]},
         "lifecycle": [{"ID": "tmp", "Status": "Enabled", "Filter": {"Prefix": "tmp/"}, "Expiration": {"Days": 1}}],
         "public_access_block": {"BlockPublicAcls": true, "IgnorePublicAcls": true},
         "encryption": "aws:kms",
         "tags": {"team": "core"}}
    ]});
    crate::aws::s3::seed(&t.state, &doc).await.unwrap();
    crate::aws::s3::seed(&t.state, &doc).await.unwrap();
    assert!(
        receive(&t, &url).await.is_empty(),
        "seeding must not send s3:TestEvent"
    );
    let names = texts(&xml(&get(&t, "/").await), &["Buckets", "Bucket"], "Name");
    assert_eq!(names, vec!["plain", "rich"]);
    assert_eq!(
        xml(&get(&t, "/rich?versioning").await)
            .child_text("Status")
            .as_deref(),
        Some("Enabled")
    );
    assert_eq!(
        xml(&get(&t, "/rich?lifecycle").await)
            .path(&["Rule", "Filter"])
            .unwrap()
            .child_text("Prefix")
            .as_deref(),
        Some("tmp/")
    );
    assert_eq!(
        xml(&get(&t, "/rich?publicAccessBlock").await)
            .child_text("BlockPublicAcls")
            .as_deref(),
        Some("true")
    );
    assert_eq!(
        texts(
            &xml(&get(&t, "/rich?tagging").await),
            &["TagSet", "Tag"],
            "Value"
        ),
        vec!["core"]
    );
    let r = t
        .call(raw(
            "OPTIONS",
            "/rich/x",
            &[
                ("origin", "http://x"),
                ("access-control-request-method", "PUT"),
            ],
            "",
        ))
        .await;
    assert_eq!(r.status, 200);
    let r = put(&t, "/rich/data.json", "{}").await;
    assert!(r.header("x-amz-version-id").is_some());
    assert_eq!(
        r.header("x-amz-server-side-encryption").as_deref(),
        Some("aws:kms")
    );
    let msgs = receive(&t, &url).await;
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["Records"][0]["s3"]["object"]["key"], "data.json");
}

/// The AWS Rust SDK addresses buckets as `/{bucket}/` (trailing slash) and
/// adds `x-id=<Operation>` to object requests.
#[tokio::test]
async fn rust_sdk_request_shapes() {
    let t = TestApp::new().await;
    let r = req(
        &t,
        "PUT",
        "/rs/",
        &[("content-type", "application/xml"), ("content-length", "0")],
        "",
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(req(&t, "HEAD", "/rs/", &[], "").await.status, 200);
    let r = req(
        &t,
        "PUT",
        "/rs/a/b.txt?x-id=PutObject",
        &[
            ("x-amz-meta-org", "o1"),
            ("x-amz-checksum-crc32", "DUoRhQ=="),
            ("x-amz-sdk-checksum-algorithm", "CRC32"),
        ],
        "hello world",
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text());
    let g = req(
        &t,
        "GET",
        "/rs/a/b.txt?x-id=GetObject",
        &[("x-amz-checksum-mode", "ENABLED")],
        "",
    )
    .await;
    assert_eq!(
        (g.text().as_str(), g.header("x-amz-meta-org").as_deref()),
        ("hello world", Some("o1"))
    );
    assert_eq!(
        g.header("x-amz-checksum-crc32").as_deref(),
        Some("DUoRhQ==")
    );
    let g = req(
        &t,
        "GET",
        "/rs/a/b.txt?x-id=GetObject",
        &[("range", "bytes=0-4"), ("x-amz-checksum-mode", "ENABLED")],
        "",
    )
    .await;
    assert_eq!((g.status, g.text().as_str()), (206, "hello"));
    assert!(g.header("x-amz-checksum-crc32").is_none());
    let l = get(&t, "/rs/?list-type=2&delimiter=%2F").await;
    assert_eq!(texts(&xml(&l), &["CommonPrefixes"], "Prefix"), vec!["a/"]);
    req(&t, "DELETE", "/rs/a/b.txt?x-id=DeleteObject", &[], "").await;
    assert_eq!(req(&t, "DELETE", "/rs/", &[], "").await.status, 204);
}

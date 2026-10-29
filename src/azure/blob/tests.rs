//! Blob Storage tests driven through HTTP with the headers/query shapes the
//! azure-storage-blob SDK sends (root `/{account}` alias included).

use crate::testutil::{raw, TestApp, TestResponse};

async fn call(
    t: &TestApp,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> TestResponse {
    t.call(raw(method, uri, headers, body.to_vec())).await
}

fn acct(t: &TestApp) -> String {
    format!("/{}", t.state.cfg.azure_account)
}

async fn create_container(t: &TestApp, name: &str) -> TestResponse {
    call(
        t,
        "PUT",
        &format!("{}/{name}?restype=container", acct(t)),
        &[],
        b"",
    )
    .await
}

async fn put(t: &TestApp, c: &str, b: &str, data: &[u8]) -> TestResponse {
    call(
        t,
        "PUT",
        &format!("{}/{c}/{b}", acct(t)),
        &[("x-ms-blob-type", "BlockBlob")],
        data,
    )
    .await
}

#[tokio::test]
async fn container_lifecycle() {
    let t = TestApp::new().await;
    let r = create_container(&t, "c1").await;
    assert_eq!(r.status, 201);
    assert!(r.header("etag").is_some());
    assert!(r.header("last-modified").unwrap().ends_with(" GMT"));
    assert!(r.header("x-ms-version").is_some());
    assert_eq!(create_container(&t, "c1").await.status, 409);

    // Trailing-slash account URL, as the SDK sends it.
    let r = call(&t, "GET", &format!("{}/?comp=list", acct(&t)), &[], b"").await;
    assert_eq!(r.status, 200);
    assert!(r.text().contains("<Name>c1</Name>"), "{}", r.text());
    assert!(r.text().contains("<Last-Modified>"));

    let props = call(
        &t,
        "GET",
        &format!("{}/c1?restype=container", acct(&t)),
        &[],
        b"",
    )
    .await;
    assert_eq!(props.status, 200);

    put(&t, "c1", "a.txt", b"x").await;
    let r = call(
        &t,
        "DELETE",
        &format!("{}/c1?restype=container", acct(&t)),
        &[],
        b"",
    )
    .await;
    assert_eq!(r.status, 202);
    let r = call(
        &t,
        "GET",
        &format!("{}/c1?restype=container", acct(&t)),
        &[],
        b"",
    )
    .await;
    assert_eq!(r.status, 404);
    assert_eq!(
        r.header("x-ms-error-code").as_deref(),
        Some("ContainerNotFound")
    );
    let n: i64 = t
        .state
        .db
        .call(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM azure_blobs WHERE container='c1'",
                [],
                |r| r.get(0),
            )
        })
        .await
        .unwrap();
    assert_eq!(n, 0, "blobs are removed with their container");
}

#[tokio::test]
async fn upload_download_binary_with_range() {
    let t = TestApp::new().await;
    create_container(&t, "c").await;
    let data: Vec<u8> = (0..=255u8).collect();
    let r = put(&t, "c", "dir/bin.dat", &data).await;
    assert_eq!(r.status, 201);
    assert!(r.header("content-md5").is_some());

    let r = call(
        &t,
        "GET",
        &format!("{}/c/dir/bin.dat", acct(&t)),
        &[("x-ms-range", "bytes=0-33554431")],
        b"",
    )
    .await;
    assert_eq!(r.status, 206);
    assert_eq!(
        r.header("content-range").as_deref(),
        Some("bytes 0-255/256")
    );
    assert_eq!(r.header("x-ms-blob-type").as_deref(), Some("BlockBlob"));
    assert_eq!(r.body.as_ref(), data.as_slice());

    let r = call(
        &t,
        "GET",
        &format!("{}/c/dir/bin.dat", acct(&t)),
        &[("range", "bytes=10-19")],
        b"",
    )
    .await;
    assert_eq!(r.body.as_ref(), &data[10..20]);

    let r = call(&t, "GET", &format!("{}/c/dir/bin.dat", acct(&t)), &[], b"").await;
    assert_eq!(r.status, 200);
    assert_eq!(r.body.len(), 256);
}

#[tokio::test]
async fn empty_blob_range_is_416() {
    let t = TestApp::new().await;
    create_container(&t, "c").await;
    put(&t, "c", "empty", b"").await;
    let r = call(
        &t,
        "GET",
        &format!("{}/c/empty", acct(&t)),
        &[("x-ms-range", "bytes=0-100")],
        b"",
    )
    .await;
    assert_eq!(r.status, 416);
    let r = call(&t, "GET", &format!("{}/c/empty", acct(&t)), &[], b"").await;
    assert_eq!(r.status, 200);
    assert!(r.body.is_empty());
}

#[tokio::test]
async fn overwrite_conditions_and_delete() {
    let t = TestApp::new().await;
    create_container(&t, "c").await;
    put(&t, "c", "k", b"v1").await;
    put(&t, "c", "k", b"v2").await;
    let r = call(&t, "GET", &format!("{}/c/k", acct(&t)), &[], b"").await;
    assert_eq!(r.body.as_ref(), b"v2");

    let uri = format!("{}/c/k", acct(&t));
    let r = call(
        &t,
        "PUT",
        &uri,
        &[("x-ms-blob-type", "BlockBlob"), ("if-none-match", "*")],
        b"v3",
    )
    .await;
    assert_eq!(r.status, 409);
    assert_eq!(
        r.header("x-ms-error-code").as_deref(),
        Some("BlobAlreadyExists")
    );

    let r = call(&t, "HEAD", &uri, &[], b"").await;
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-length").as_deref(), Some("2"));

    assert_eq!(call(&t, "DELETE", &uri, &[], b"").await.status, 202);
    let r = call(&t, "GET", &uri, &[], b"").await;
    assert_eq!(r.status, 404);
    assert_eq!(r.header("x-ms-error-code").as_deref(), Some("BlobNotFound"));
    assert_eq!(call(&t, "DELETE", &uri, &[], b"").await.status, 404);

    let r = put(&t, "missing", "k", b"x").await;
    assert_eq!(r.status, 404);
}

#[tokio::test]
async fn list_blobs_prefix_delimiter_and_paging() {
    let t = TestApp::new().await;
    create_container(&t, "c").await;
    for n in ["a/1", "a/2", "b", "c%_x"] {
        put(&t, "c", n, b"x").await;
    }
    let base = format!("{}/c?restype=container&comp=list", acct(&t));
    let r = call(&t, "GET", &base, &[], b"").await;
    assert_eq!(r.status, 200);
    let doc = crate::xml::parse(&r.body).unwrap();
    let names: Vec<String> = doc
        .child("Blobs")
        .unwrap()
        .children_named("Blob")
        .filter_map(|b| b.child_text("Name"))
        .collect();
    assert_eq!(names, vec!["a/1", "a/2", "b", "c%_x"]);

    let r = call(&t, "GET", &format!("{base}&prefix=c%25_"), &[], b"").await;
    assert!(r.text().contains("<Name>c%_x</Name>") && !r.text().contains("<Name>b</Name>"));

    let r = call(&t, "GET", &format!("{base}&delimiter=/"), &[], b"").await;
    assert!(
        r.text()
            .contains("<BlobPrefix><Name>a/</Name></BlobPrefix>"),
        "{}",
        r.text()
    );
    assert!(!r.text().contains("<Name>a/1</Name>"));

    let r = call(&t, "GET", &format!("{base}&maxresults=2"), &[], b"").await;
    let doc = crate::xml::parse(&r.body).unwrap();
    assert_eq!(doc.child_text("NextMarker").as_deref(), Some("b"));
    let r = call(
        &t,
        "GET",
        &format!("{base}&maxresults=2&marker=b"),
        &[],
        b"",
    )
    .await;
    let doc = crate::xml::parse(&r.body).unwrap();
    assert_eq!(
        doc.child("Blobs").unwrap().children_named("Blob").count(),
        2
    );
    assert_eq!(doc.child_text("NextMarker").unwrap_or_default(), "");
}

#[tokio::test]
async fn unknown_account_is_404() {
    let t = TestApp::new().await;
    let r = call(&t, "GET", "/azure/otheraccount?comp=list", &[], b"").await;
    assert_eq!(r.status, 404);
}

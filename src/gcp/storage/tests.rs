//! Cloud Storage tests driven through HTTP, shaped like google-cloud-storage requests.

use base64::Engine;
use serde_json::{json, Value};

use crate::testutil::{raw, rest, TestApp, TestResponse};

async fn create_bucket(t: &TestApp, name: &str) -> TestResponse {
    t.call(rest(
        "POST",
        "/storage/v1/b?project=p",
        Some(json!({"name": name})),
    ))
    .await
}

async fn media_upload(t: &TestApp, bucket: &str, name: &str, data: &[u8]) -> TestResponse {
    let uri = format!("/upload/storage/v1/b/{bucket}/o?uploadType=media&name={name}");
    t.call(raw(
        "POST",
        &uri,
        &[("content-type", "text/plain")],
        data.to_vec(),
    ))
    .await
}

fn names(v: &Value) -> Vec<String> {
    v["items"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|o| o["name"].as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn bucket_lifecycle_and_errors() {
    let t = TestApp::new().await;
    let r = create_bucket(&t, "bk").await;
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(r.json()["kind"], "storage#bucket");
    assert!(r.json()["timeCreated"].as_str().unwrap().ends_with('Z'));
    assert_eq!(create_bucket(&t, "bk").await.status, 409);

    let r = t.call(rest("GET", "/storage/v1/b?project=p", None)).await;
    assert_eq!(r.json()["items"][0]["name"], "bk");

    media_upload(&t, "bk", "x", b"1").await;
    assert_eq!(
        t.call(rest("DELETE", "/storage/v1/b/bk", None))
            .await
            .status,
        409
    );
    assert_eq!(
        t.call(rest("DELETE", "/storage/v1/b/bk/o/x", None))
            .await
            .status,
        204
    );
    assert_eq!(
        t.call(rest("DELETE", "/storage/v1/b/bk", None))
            .await
            .status,
        204
    );
    let r = t.call(rest("GET", "/storage/v1/b/bk", None)).await;
    assert_eq!(r.status, 404);
    assert_eq!(r.json()["error"]["code"], 404);
}

#[tokio::test]
async fn multipart_upload_stores_name_from_metadata_and_downloads() {
    let t = TestApp::new().await;
    create_bucket(&t, "mp").await;
    let data: Vec<u8> = (0u8..=255).collect();
    let mut body = b"--BND\r\ncontent-type: application/json; charset=UTF-8\r\n\r\n".to_vec();
    body.extend_from_slice(
        br#"{"name": "dir/a b.bin", "contentType": "application/x-bin", "metadata": {"k": "v"}}"#,
    );
    body.extend_from_slice(b"\r\n--BND\r\ncontent-type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(&data);
    body.extend_from_slice(b"\r\n--BND--");
    let r = t
        .call(raw(
            "POST",
            "/upload/storage/v1/b/mp/o?uploadType=multipart",
            &[
                ("content-type", "multipart/related; boundary=\"BND\""),
                ("host", "h:1"),
            ],
            body,
        ))
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
    let o = r.json();
    assert_eq!(o["name"], "dir/a b.bin");
    assert_eq!(o["size"], "256");
    assert_eq!(o["contentType"], "application/x-bin");
    assert_eq!(o["metadata"]["k"], "v");
    let md5 =
        base64::engine::general_purpose::STANDARD.encode(<md5::Md5 as md5::Digest>::digest(&data));
    assert_eq!(o["md5Hash"], md5);
    let media = o["mediaLink"].as_str().unwrap();
    assert!(
        media.starts_with("http://h:1/download/storage/v1/b/mp/o/dir%2Fa%20b.bin?"),
        "{media}"
    );

    // Download via the encoded name, both download and alt=media forms.
    for uri in [
        "/download/storage/v1/b/mp/o/dir%2Fa%20b.bin?alt=media",
        "/storage/v1/b/mp/o/dir%2Fa%20b.bin?alt=media",
    ] {
        let r = t.call(rest("GET", uri, None)).await;
        assert_eq!(r.status, 200, "{uri}");
        assert_eq!(r.body.to_vec(), data);
        assert!(r
            .header("x-goog-hash")
            .unwrap()
            .contains(&format!("md5={md5}")));
    }
    let r = t
        .call(raw(
            "GET",
            "/download/storage/v1/b/mp/o/dir%2Fa%20b.bin?alt=media",
            &[("range", "bytes=1-3")],
            "",
        ))
        .await;
    assert_eq!(r.status, 206);
    assert_eq!(r.body.to_vec(), vec![1, 2, 3]);
    assert_eq!(r.header("content-range").unwrap(), "bytes 1-3/256");

    let r = t
        .call(rest("GET", "/storage/v1/b/mp/o/missing?alt=media", None))
        .await;
    assert_eq!(r.status, 404);
}

#[tokio::test]
async fn resumable_upload_in_chunks() {
    let t = TestApp::new().await;
    create_bucket(&t, "rs").await;
    let r = t
        .call(raw(
            "POST",
            "/upload/storage/v1/b/rs/o?uploadType=resumable",
            &[
                ("content-type", "application/json"),
                ("host", "h:2"),
                ("x-upload-content-type", "text/csv"),
            ],
            json!({"name": "r.csv"}).to_string(),
        ))
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
    let location = r.header("location").unwrap();
    let path = location.strip_prefix("http://h:2").unwrap().to_string();

    let r = t
        .call(raw(
            "PUT",
            &path,
            &[("content-range", "bytes 0-2/*")],
            "abc",
        ))
        .await;
    assert_eq!(r.status, 308);
    assert_eq!(r.header("range").unwrap(), "bytes=0-2");
    let r = t
        .call(raw("PUT", &path, &[("content-range", "bytes 3-4/5")], "de"))
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(r.json()["size"], "5");
    assert_eq!(r.json()["contentType"], "text/csv");

    let r = t
        .call(rest(
            "GET",
            "/download/storage/v1/b/rs/o/r.csv?alt=media",
            None,
        ))
        .await;
    assert_eq!(r.text(), "abcde");
    // Session is consumed.
    assert_eq!(t.call(raw("PUT", &path, &[], "x")).await.status, 404);
}

#[tokio::test]
async fn list_with_prefix_delimiter_and_paging() {
    let t = TestApp::new().await;
    create_bucket(&t, "ls").await;
    for n in ["a/1", "a/2", "a/b/3", "a_x", "c"] {
        media_upload(&t, "ls", n, n.as_bytes()).await;
    }
    // `_` must not act as a LIKE wildcard.
    let r = t
        .call(rest("GET", "/storage/v1/b/ls/o?prefix=a_", None))
        .await;
    assert_eq!(names(&r.json()), vec!["a_x"]);

    let r = t
        .call(rest("GET", "/storage/v1/b/ls/o?delimiter=/", None))
        .await;
    assert_eq!(names(&r.json()), vec!["a_x", "c"]);
    assert_eq!(r.json()["prefixes"], json!(["a/"]));

    let r = t
        .call(rest(
            "GET",
            "/storage/v1/b/ls/o?prefix=a/&delimiter=/",
            None,
        ))
        .await;
    assert_eq!(names(&r.json()), vec!["a/1", "a/2"]);
    assert_eq!(r.json()["prefixes"], json!(["a/b/"]));

    let mut seen = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let uri = match &token {
            Some(tok) => format!("/storage/v1/b/ls/o?maxResults=2&pageToken={tok}"),
            None => "/storage/v1/b/ls/o?maxResults=2".to_string(),
        };
        let v = t.call(rest("GET", &uri, None)).await.json();
        seen.extend(names(&v));
        token = v["nextPageToken"].as_str().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    assert_eq!(seen, vec!["a/1", "a/2", "a/b/3", "a_x", "c"]);

    let r = t.call(rest("GET", "/storage/v1/b/nope/o", None)).await;
    assert_eq!(r.status, 404);
}

#[tokio::test]
async fn overwrite_bumps_generation_and_preconditions() {
    let t = TestApp::new().await;
    create_bucket(&t, "gen").await;
    let g1 = media_upload(&t, "gen", "o", b"v1").await.json()["generation"].clone();
    let g2 = media_upload(&t, "gen", "o", b"v2").await.json()["generation"].clone();
    assert_ne!(g1, g2);

    let r = t
        .call(raw(
            "POST",
            "/upload/storage/v1/b/gen/o?uploadType=media&name=o&ifGenerationMatch=0",
            &[],
            "v3",
        ))
        .await;
    assert_eq!(r.status, 412);

    let stale = format!(
        "/download/storage/v1/b/gen/o/o?alt=media&generation={}",
        g1.as_str().unwrap()
    );
    assert_eq!(t.call(rest("GET", &stale, None)).await.status, 404);
    let r = t
        .call(rest(
            "GET",
            "/download/storage/v1/b/gen/o/o?alt=media",
            None,
        ))
        .await;
    assert_eq!(r.text(), "v2");
    assert_eq!(
        t.call(rest("DELETE", "/storage/v1/b/gen/o/o", None))
            .await
            .status,
        204
    );
    assert_eq!(
        t.call(rest("DELETE", "/storage/v1/b/gen/o/o", None))
            .await
            .status,
        404
    );
}

#[tokio::test]
async fn patch_merges_metadata() {
    let t = TestApp::new().await;
    create_bucket(&t, "pt").await;
    media_upload(&t, "pt", "o", b"x").await;
    let r = t
        .call(rest(
            "PATCH",
            "/storage/v1/b/pt/o/o",
            Some(json!({"metadata": {"a": "1"}, "contentType": "text/html"})),
        ))
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
    t.call(rest(
        "PATCH",
        "/storage/v1/b/pt/o/o",
        Some(json!({"metadata": {"b": "2", "a": null}})),
    ))
    .await;
    let v = t
        .call(rest("GET", "/storage/v1/b/pt/o/o", None))
        .await
        .json();
    assert_eq!(v["metadata"], json!({"b": "2"}));
    assert_eq!(v["contentType"], "text/html");
}

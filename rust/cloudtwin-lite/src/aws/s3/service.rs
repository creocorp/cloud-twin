//! S3 domain logic backed by SQLite.
//!
//! The service owns bucket/object/version/multipart semantics and leaves HTTP
//! concerns to the handler modules. The current (visible) version of each key
//! lives in `s3_objects`; noncurrent versions and delete markers of versioned
//! buckets live in `s3_object_versions`.

use chrono::Utc;
use md5::{Digest, Md5};
use rusqlite::{params, Connection, OptionalExtension};

use super::errors::{S3Error, S3Result};
use super::models::*;
use crate::db::Database;

pub fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

pub fn md5_etag(content: &[u8]) -> String {
    format!("\"{}\"", hex::encode(Md5::digest(content)))
}

fn new_version_id() -> String {
    use rand::Rng;
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._";
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}

fn next_seq(conn: &Connection) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO s3_counters (name, value) VALUES ('seq', 1)
         ON CONFLICT(name) DO UPDATE SET value = value + 1",
        [],
    )?;
    conn.query_row(
        "SELECT value FROM s3_counters WHERE name = 'seq'",
        [],
        |r| r.get(0),
    )
}

fn bucket_exists(conn: &Connection, bucket: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT COUNT(*) FROM s3_buckets WHERE name = ?1",
        params![bucket],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
}

fn require_bucket(conn: &Connection, bucket: &str) -> S3Result<()> {
    if bucket_exists(conn, bucket)? {
        Ok(())
    } else {
        Err(S3Error::no_such_bucket(bucket))
    }
}

fn config_get(conn: &Connection, bucket: &str, kind: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT body FROM s3_bucket_configs WHERE bucket = ?1 AND kind = ?2",
        params![bucket, kind],
        |r| r.get(0),
    )
    .optional()
}

fn versioning_of(conn: &Connection, bucket: &str) -> S3Result<Versioning> {
    require_bucket(conn, bucket)?;
    Ok(Versioning::parse(
        &config_get(conn, bucket, "versioning")?.unwrap_or_default(),
    ))
}

const SSE_HEADER: &str = "x-amz-server-side-encryption";

/// Fill in bucket-default (or AES256) encryption when the request had none.
fn apply_default_sse(conn: &Connection, bucket: &str, meta: &mut ObjMeta) -> rusqlite::Result<()> {
    let has = meta
        .sse
        .iter()
        .any(|(k, _)| k == SSE_HEADER || k == "x-amz-server-side-encryption-customer-algorithm");
    if has {
        return Ok(());
    }
    let mut algo = "AES256".to_string();
    let mut extra = Vec::new();
    if let Some(body) = config_get(conn, bucket, "encryption")? {
        if let Ok(doc) = crate::xml::parse(body.as_bytes()) {
            if let Some(rule) = doc.child("Rule") {
                if let Some(def) = rule.child("ApplyServerSideEncryptionByDefault") {
                    if let Some(a) = def.child_text("SSEAlgorithm").filter(|a| !a.is_empty()) {
                        algo = a;
                    }
                    if algo.starts_with("aws:kms") {
                        if let Some(k) = def.child_text("KMSMasterKeyID").filter(|k| !k.is_empty())
                        {
                            extra.push((
                                "x-amz-server-side-encryption-aws-kms-key-id".to_string(),
                                k,
                            ));
                        }
                    }
                }
                if algo.starts_with("aws:kms")
                    && rule.child_text("BucketKeyEnabled").as_deref() == Some("true")
                {
                    extra.push((
                        "x-amz-server-side-encryption-bucket-key-enabled".to_string(),
                        "true".to_string(),
                    ));
                }
            }
        }
    }
    meta.sse.insert(0, (SSE_HEADER.to_string(), algo));
    meta.sse.extend(extra);
    Ok(())
}

const VCOLS: &str =
    "bucket, key, version_id, seq, content_type, size, etag, created_at, meta, tags";

fn info_from_row(r: &rusqlite::Row<'_>, marker: bool) -> rusqlite::Result<ObjectInfo> {
    Ok(ObjectInfo {
        bucket: r.get(0)?,
        key: r.get(1)?,
        version_id: r.get(2)?,
        seq: r.get(3)?,
        content_type: r.get(4)?,
        size: r.get(5)?,
        etag: r.get(6)?,
        created_at: r.get(7)?,
        meta: ObjMeta::from_json(&r.get::<_, String>(8)?),
        tags: tags_from_json(&r.get::<_, String>(9)?),
        is_delete_marker: marker,
        is_latest: false,
    })
}

fn current_info(
    conn: &Connection,
    bucket: &str,
    key: &str,
) -> rusqlite::Result<Option<ObjectInfo>> {
    conn.query_row(
        &format!("SELECT {VCOLS} FROM s3_objects WHERE bucket = ?1 AND key = ?2"),
        params![bucket, key],
        |r| info_from_row(r, false),
    )
    .optional()
    .map(|o| {
        o.map(|mut i| {
            i.is_latest = true;
            i
        })
    })
}

fn latest_archived(
    conn: &Connection,
    bucket: &str,
    key: &str,
) -> rusqlite::Result<Option<ObjectInfo>> {
    conn.query_row(
        &format!(
            "SELECT {VCOLS}, is_delete_marker FROM s3_object_versions
             WHERE bucket = ?1 AND key = ?2 ORDER BY seq DESC LIMIT 1"
        ),
        params![bucket, key],
        |r| {
            let m: i64 = r.get(10)?;
            info_from_row(r, m != 0)
        },
    )
    .optional()
}

fn archived_info(
    conn: &Connection,
    bucket: &str,
    key: &str,
    version_id: &str,
) -> rusqlite::Result<Option<ObjectInfo>> {
    conn.query_row(
        &format!(
            "SELECT {VCOLS}, is_delete_marker FROM s3_object_versions
             WHERE bucket = ?1 AND key = ?2 AND version_id = ?3"
        ),
        params![bucket, key, version_id],
        |r| {
            let m: i64 = r.get(10)?;
            info_from_row(r, m != 0)
        },
    )
    .optional()
}

/// Move the current version of `key` into the version archive.
fn archive_current(conn: &Connection, bucket: &str, key: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO s3_object_versions
           (bucket, key, version_id, seq, is_delete_marker, content, content_type, size, etag, created_at, meta, tags)
         SELECT bucket, key, version_id, seq, 0, content, content_type, size, etag, created_at, meta, tags
         FROM s3_objects WHERE bucket = ?1 AND key = ?2",
        params![bucket, key],
    )?;
    conn.execute(
        "DELETE FROM s3_objects WHERE bucket = ?1 AND key = ?2",
        params![bucket, key],
    )?;
    Ok(())
}

/// Make the newest archived version current again if it is a real object.
fn restore_latest(conn: &Connection, bucket: &str, key: &str) -> rusqlite::Result<()> {
    let cur = current_info(conn, bucket, key)?;
    let Some(latest) = latest_archived(conn, bucket, key)? else {
        return Ok(());
    };
    match cur {
        Some(c) if c.seq > latest.seq => return Ok(()),
        Some(_) => archive_current(conn, bucket, key)?,
        None => {}
    }
    if latest.is_delete_marker {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO s3_objects
           (bucket, key, version_id, seq, content, content_type, size, etag, created_at, meta, tags)
         SELECT bucket, key, version_id, seq, content, content_type, size, etag, created_at, meta, tags
         FROM s3_object_versions WHERE bucket = ?1 AND key = ?2 AND version_id = ?3",
        params![bucket, key, latest.version_id],
    )?;
    conn.execute(
        "DELETE FROM s3_object_versions WHERE bucket = ?1 AND key = ?2 AND version_id = ?3",
        params![bucket, key, latest.version_id],
    )?;
    Ok(())
}

/// Resolve the version addressed by a request (current when `version_id` is None).
fn resolve_version(
    conn: &Connection,
    bucket: &str,
    key: &str,
    version_id: Option<&str>,
) -> S3Result<ObjectInfo> {
    require_bucket(conn, bucket)?;
    match version_id {
        None => {
            if let Some(i) = current_info(conn, bucket, key)? {
                return Ok(i);
            }
            let mut err = S3Error::no_such_key(key);
            if let Some(l) = latest_archived(conn, bucket, key)? {
                if l.is_delete_marker {
                    err = err
                        .header("x-amz-delete-marker", "true")
                        .header("x-amz-version-id", l.version_id);
                }
            }
            Err(err)
        }
        Some(v) => {
            if let Some(i) = current_info(conn, bucket, key)? {
                if i.version_id == v {
                    return Ok(i);
                }
            }
            match archived_info(conn, bucket, key, v)? {
                Some(i) if i.is_delete_marker => Err(S3Error::new(
                    "MethodNotAllowed",
                    "The specified method is not allowed against this resource.",
                )
                .with("Method", "GET")
                .with("ResourceType", "DeleteMarker")
                .header("x-amz-delete-marker", "true")
                .header("x-amz-version-id", v)
                .header("allow", "DELETE")),
                Some(i) => Ok(i),
                None => Err(
                    S3Error::new("NoSuchVersion", "The specified version does not exist.")
                        .with("Key", key)
                        .with("VersionId", v),
                ),
            }
        }
    }
}

fn write_object_tx(
    conn: &mut Connection,
    bucket: &str,
    key: &str,
    mut obj: NewObject,
) -> S3Result<WriteResult> {
    let tx = conn.transaction()?;
    let versioning = versioning_of(&tx, bucket)?;
    apply_default_sse(&tx, bucket, &mut obj.meta)?;
    let seq = next_seq(&tx)?;
    let vid = if versioning == Versioning::Enabled {
        new_version_id()
    } else {
        "null".to_string()
    };
    if let Some(cur) = current_info(&tx, bucket, key)? {
        if cur.version_id == vid || versioning == Versioning::Unversioned {
            tx.execute(
                "DELETE FROM s3_objects WHERE bucket = ?1 AND key = ?2",
                params![bucket, key],
            )?;
        } else {
            archive_current(&tx, bucket, key)?;
        }
    }
    if vid == "null" {
        tx.execute(
            "DELETE FROM s3_object_versions WHERE bucket = ?1 AND key = ?2 AND version_id = 'null'",
            params![bucket, key],
        )?;
    }
    let now = now_iso();
    let size = obj.content.len() as i64;
    tx.execute(
        "INSERT INTO s3_objects
           (bucket, key, version_id, seq, content, content_type, size, etag, created_at, meta, tags)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            bucket,
            key,
            vid,
            seq,
            obj.content,
            obj.content_type,
            size,
            obj.etag,
            now,
            obj.meta.to_json(),
            tags_to_json(&obj.tags)
        ],
    )?;
    tx.commit()?;
    Ok(WriteResult {
        version_id: vid,
        seq,
        etag: obj.etag,
        size,
        created_at: now,
        versioning,
        meta: obj.meta,
    })
}

/// Parameters for a key listing.
#[derive(Debug, Clone, Default)]
pub struct ListParams {
    pub prefix: String,
    pub delimiter: Option<String>,
    /// Exclusive start key (V1 marker, V2 start-after / continuation token).
    pub after: Option<String>,
    pub max_keys: usize,
}

fn common_prefix(key: &str, prefix: &str, delimiter: Option<&str>) -> Option<String> {
    let d = delimiter?;
    let rest = &key[prefix.len()..];
    rest.find(d)
        .map(|i| key[..prefix.len() + i + d.len()].to_string())
}

/// Implements S3 bucket and object semantics on top of SQLite.
pub struct S3Service {
    db: Database,
}

impl S3Service {
    pub fn new(db: Database) -> Self {
        S3Service { db }
    }

    async fn run<T, F>(&self, f: F) -> S3Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> S3Result<T> + Send + 'static,
    {
        self.db.conn.call(move |c| Ok(f(c))).await?
    }

    // ── Buckets ───────────────────────────────────────────────────────────

    pub async fn list_buckets(&self) -> S3Result<Vec<Bucket>> {
        self.run(|conn| {
            let mut stmt = conn.prepare("SELECT name, created_at FROM s3_buckets ORDER BY name")?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(Bucket {
                        name: r.get(0)?,
                        created_at: r.get(1)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
    }

    /// Create a bucket; returns false when it already existed.
    pub async fn create_bucket(&self, name: &str) -> S3Result<bool> {
        let name = name.to_string();
        self.run(move |conn| {
            Ok(conn.execute(
                "INSERT OR IGNORE INTO s3_buckets (name, created_at) VALUES (?1, ?2)",
                params![name, now_iso()],
            )? > 0)
        })
        .await
    }

    pub async fn delete_bucket(&self, name: &str) -> S3Result<()> {
        let name = name.to_string();
        self.run(move |conn| {
            let tx = conn.transaction()?;
            require_bucket(&tx, &name)?;
            let n: i64 = tx.query_row(
                "SELECT (SELECT COUNT(*) FROM s3_objects WHERE bucket = ?1)
                      + (SELECT COUNT(*) FROM s3_object_versions WHERE bucket = ?1)",
                params![name],
                |r| r.get(0),
            )?;
            if n > 0 {
                return Err(S3Error::new(
                    "BucketNotEmpty",
                    "The bucket you tried to delete is not empty",
                )
                .with("BucketName", name.clone()));
            }
            tx.execute(
                "DELETE FROM s3_bucket_configs WHERE bucket = ?1",
                params![name],
            )?;
            tx.execute(
                "DELETE FROM s3_multipart_parts WHERE upload_id IN
                   (SELECT upload_id FROM s3_multipart_uploads WHERE bucket = ?1)",
                params![name],
            )?;
            tx.execute(
                "DELETE FROM s3_multipart_uploads WHERE bucket = ?1",
                params![name],
            )?;
            tx.execute("DELETE FROM s3_buckets WHERE name = ?1", params![name])?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    pub async fn bucket_exists(&self, name: &str) -> S3Result<bool> {
        let name = name.to_string();
        self.run(move |conn| Ok(bucket_exists(conn, &name)?)).await
    }

    // ── Bucket sub-resource documents ─────────────────────────────────────

    pub async fn get_config(&self, bucket: &str, kind: &str) -> S3Result<Option<String>> {
        let (bucket, kind) = (bucket.to_string(), kind.to_string());
        self.run(move |conn| {
            require_bucket(conn, &bucket)?;
            Ok(config_get(conn, &bucket, &kind)?)
        })
        .await
    }

    pub async fn put_config(&self, bucket: &str, kind: &str, body: &str) -> S3Result<()> {
        let (bucket, kind, body) = (bucket.to_string(), kind.to_string(), body.to_string());
        self.run(move |conn| {
            require_bucket(conn, &bucket)?;
            conn.execute(
                "INSERT INTO s3_bucket_configs (bucket, kind, body) VALUES (?1, ?2, ?3)
                 ON CONFLICT(bucket, kind) DO UPDATE SET body = excluded.body",
                params![bucket, kind, body],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn delete_config(&self, bucket: &str, kind: &str) -> S3Result<()> {
        let (bucket, kind) = (bucket.to_string(), kind.to_string());
        self.run(move |conn| {
            require_bucket(conn, &bucket)?;
            conn.execute(
                "DELETE FROM s3_bucket_configs WHERE bucket = ?1 AND kind = ?2",
                params![bucket, kind],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn versioning(&self, bucket: &str) -> S3Result<Versioning> {
        let bucket = bucket.to_string();
        self.run(move |conn| versioning_of(conn, &bucket)).await
    }

    // ── Listing ───────────────────────────────────────────────────────────

    pub async fn list_objects(&self, bucket: &str, p: ListParams) -> S3Result<ListPage> {
        let bucket = bucket.to_string();
        self.run(move |conn| {
            require_bucket(conn, &bucket)?;
            let mut page = ListPage::default();
            if p.max_keys == 0 {
                return Ok(page);
            }
            let start = std::cmp::max(p.prefix.clone(), p.after.clone().unwrap_or_default());
            let delim = p.delimiter.as_deref().filter(|d| !d.is_empty());
            let mut stmt = conn.prepare(
                "SELECT key, size, etag, created_at, meta FROM s3_objects
                 WHERE bucket = ?1 AND key >= ?2 ORDER BY key",
            )?;
            let mut rows = stmt.query(params![bucket, start])?;
            let mut count = 0usize;
            let mut last_cp: Option<String> = None;
            while let Some(row) = rows.next()? {
                let key: String = row.get(0)?;
                if !key.starts_with(&p.prefix) {
                    break;
                }
                if p.after.as_ref().is_some_and(|a| key <= *a) {
                    continue;
                }
                let cp = common_prefix(&key, &p.prefix, delim);
                if let Some(cp) = &cp {
                    if last_cp.as_ref() == Some(cp) || p.after.as_ref() == Some(cp) {
                        continue;
                    }
                }
                if count >= p.max_keys {
                    page.truncated = true;
                    break;
                }
                count += 1;
                match cp {
                    Some(cp) => {
                        page.common_prefixes.push(cp.clone());
                        page.last = Some(cp.clone());
                        last_cp = Some(cp);
                    }
                    None => {
                        let meta = ObjMeta::from_json(&row.get::<_, String>(4)?);
                        page.last = Some(key.clone());
                        page.contents.push(ListEntry {
                            key,
                            size: row.get(1)?,
                            etag: row.get(2)?,
                            created_at: row.get(3)?,
                            storage_class: meta.storage_class().to_string(),
                        });
                    }
                }
            }
            Ok(page)
        })
        .await
    }

    pub async fn list_versions(
        &self,
        bucket: &str,
        p: ListParams,
        version_marker: Option<String>,
    ) -> S3Result<VersionPage> {
        let bucket = bucket.to_string();
        self.run(move |conn| {
            require_bucket(conn, &bucket)?;
            let mut page = VersionPage::default();
            if p.max_keys == 0 {
                return Ok(page);
            }
            let key_marker = p.after.clone().filter(|k| !k.is_empty());
            let version_marker = version_marker.filter(|v| !v.is_empty() && key_marker.is_some());
            let start = std::cmp::max(p.prefix.clone(), key_marker.clone().unwrap_or_default());
            let delim = p.delimiter.as_deref().filter(|d| !d.is_empty());
            let mut stmt = conn.prepare(
                "SELECT key, version_id, seq, 0 AS m, size, etag, created_at, meta
                   FROM s3_objects WHERE bucket = ?1 AND key >= ?2
                 UNION ALL
                 SELECT key, version_id, seq, is_delete_marker, size, etag, created_at, meta
                   FROM s3_object_versions WHERE bucket = ?1 AND key >= ?2
                 ORDER BY 1, 3 DESC",
            )?;
            let mut rows = stmt.query(params![bucket, start])?;
            let mut count = 0usize;
            let mut last_cp: Option<String> = None;
            let mut prev_key: Option<String> = None;
            // When resuming inside key_marker, skip up to and including version_marker.
            let mut skipping_marker_versions = version_marker.is_some();
            while let Some(row) = rows.next()? {
                let key: String = row.get(0)?;
                if !key.starts_with(&p.prefix) {
                    break;
                }
                let vid: String = row.get(1)?;
                let is_latest = prev_key.as_deref() != Some(key.as_str());
                prev_key = Some(key.clone());
                if let Some(km) = &key_marker {
                    if version_marker.is_some() {
                        if key < *km {
                            continue;
                        }
                        if key == *km && skipping_marker_versions {
                            if Some(&vid) == version_marker.as_ref() {
                                skipping_marker_versions = false;
                            }
                            continue;
                        }
                    } else if key <= *km {
                        continue;
                    }
                }
                let cp = common_prefix(&key, &p.prefix, delim);
                if let Some(cp) = &cp {
                    if last_cp.as_ref() == Some(cp) || key_marker.as_ref() == Some(cp) {
                        continue;
                    }
                }
                if count >= p.max_keys {
                    page.truncated = true;
                    break;
                }
                count += 1;
                match cp {
                    Some(cp) => {
                        page.common_prefixes.push(cp.clone());
                        page.next_key_marker = Some(cp.clone());
                        page.next_version_marker = None;
                        last_cp = Some(cp);
                    }
                    None => {
                        let m: i64 = row.get(3)?;
                        let meta = ObjMeta::from_json(&row.get::<_, String>(7)?);
                        page.next_key_marker = Some(key.clone());
                        page.next_version_marker = Some(vid.clone());
                        page.entries.push(VersionEntry {
                            key,
                            version_id: vid,
                            is_delete_marker: m != 0,
                            is_latest,
                            size: row.get(4)?,
                            etag: row.get(5)?,
                            created_at: row.get(6)?,
                            storage_class: meta.storage_class().to_string(),
                        });
                    }
                }
            }
            if !page.truncated {
                page.next_key_marker = None;
                page.next_version_marker = None;
            }
            Ok(page)
        })
        .await
    }

    // ── Objects ───────────────────────────────────────────────────────────

    pub async fn write_object(
        &self,
        bucket: &str,
        key: &str,
        obj: NewObject,
    ) -> S3Result<WriteResult> {
        let (bucket, key) = (bucket.to_string(), key.to_string());
        self.run(move |conn| write_object_tx(conn, &bucket, &key, obj))
            .await
    }

    /// Convenience wrapper computing the MD5 ETag.
    pub async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        content: Vec<u8>,
        content_type: &str,
        meta: ObjMeta,
        tags: Tags,
    ) -> S3Result<WriteResult> {
        let etag = md5_etag(&content);
        self.write_object(
            bucket,
            key,
            NewObject {
                content,
                content_type: content_type.to_string(),
                etag,
                meta,
                tags,
            },
        )
        .await
    }

    pub async fn object_info(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
    ) -> S3Result<ObjectInfo> {
        let (bucket, key, vid) = (
            bucket.to_string(),
            key.to_string(),
            version_id.map(str::to_string),
        );
        self.run(move |conn| resolve_version(conn, &bucket, &key, vid.as_deref()))
            .await
    }

    /// Read `[start, start+len)` of a specific version, or all of it.
    /// Returns None when that version changed since `info` was read.
    pub async fn read_content(
        &self,
        info: &ObjectInfo,
        range: Option<(u64, u64)>,
    ) -> S3Result<Option<Vec<u8>>> {
        let (bucket, key, vid, seq) = (
            info.bucket.clone(),
            info.key.clone(),
            info.version_id.clone(),
            info.seq,
        );
        self.run(move |conn| {
            for table in ["s3_objects", "s3_object_versions"] {
                let filter = "WHERE bucket = ?1 AND key = ?2 AND version_id = ?3 AND seq = ?4";
                let blob = |r: &rusqlite::Row<'_>| -> rusqlite::Result<Vec<u8>> {
                    Ok(match r.get_ref(0)? {
                        rusqlite::types::ValueRef::Blob(b) => b.to_vec(),
                        rusqlite::types::ValueRef::Text(t) => t.to_vec(),
                        _ => Vec::new(),
                    })
                };
                let data = match range {
                    Some((start, len)) => conn
                        .query_row(
                            &format!("SELECT substr(content, ?5, ?6) FROM {table} {filter}"),
                            params![bucket, key, vid, seq, start as i64 + 1, len as i64],
                            blob,
                        )
                        .optional()?,
                    None => conn
                        .query_row(
                            &format!("SELECT content FROM {table} {filter}"),
                            params![bucket, key, vid, seq],
                            blob,
                        )
                        .optional()?,
                };
                if data.is_some() {
                    return Ok(data);
                }
            }
            Ok(None)
        })
        .await
    }

    /// Read the full content of the addressed version (CopyObject sources).
    pub async fn get_object_full(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
    ) -> S3Result<(ObjectInfo, Vec<u8>)> {
        for _ in 0..3 {
            let info = self.object_info(bucket, key, version_id).await?;
            if let Some(c) = self.read_content(&info, None).await? {
                return Ok((info, c));
            }
        }
        Err(S3Error::internal("object changed while reading"))
    }

    /// Delete the current object (creating a delete marker when versioned)
    /// or a specific version.
    pub async fn delete_object(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
    ) -> S3Result<DeleteOutcome> {
        let (bucket, key, vid) = (
            bucket.to_string(),
            key.to_string(),
            version_id.map(str::to_string),
        );
        self.run(move |conn| {
            let tx = conn.transaction()?;
            let versioning = versioning_of(&tx, &bucket)?;
            let out = match vid {
                Some(v) => {
                    let cur = current_info(&tx, &bucket, &key)?;
                    let mut marker = false;
                    let mut found = false;
                    if cur.as_ref().is_some_and(|c| c.version_id == v) {
                        tx.execute(
                            "DELETE FROM s3_objects WHERE bucket = ?1 AND key = ?2",
                            params![bucket, key],
                        )?;
                        found = true;
                    } else if let Some(a) = archived_info(&tx, &bucket, &key, &v)? {
                        marker = a.is_delete_marker;
                        tx.execute(
                            "DELETE FROM s3_object_versions WHERE bucket = ?1 AND key = ?2 AND version_id = ?3",
                            params![bucket, key, v],
                        )?;
                        found = true;
                    }
                    if found {
                        restore_latest(&tx, &bucket, &key)?;
                    }
                    DeleteOutcome {
                        version_id: Some(v),
                        delete_marker: marker,
                        seq: next_seq(&tx)?,
                        versioning,
                    }
                }
                None if versioning == Versioning::Unversioned => {
                    tx.execute(
                        "DELETE FROM s3_objects WHERE bucket = ?1 AND key = ?2",
                        params![bucket, key],
                    )?;
                    DeleteOutcome {
                        version_id: None,
                        delete_marker: false,
                        seq: next_seq(&tx)?,
                        versioning,
                    }
                }
                None => {
                    let mid = if versioning == Versioning::Enabled {
                        new_version_id()
                    } else {
                        "null".to_string()
                    };
                    if let Some(cur) = current_info(&tx, &bucket, &key)? {
                        if cur.version_id == mid {
                            tx.execute(
                                "DELETE FROM s3_objects WHERE bucket = ?1 AND key = ?2",
                                params![bucket, key],
                            )?;
                        } else {
                            archive_current(&tx, &bucket, &key)?;
                        }
                    }
                    if mid == "null" {
                        tx.execute(
                            "DELETE FROM s3_object_versions WHERE bucket = ?1 AND key = ?2 AND version_id = 'null'",
                            params![bucket, key],
                        )?;
                    }
                    let seq = next_seq(&tx)?;
                    tx.execute(
                        "INSERT INTO s3_object_versions
                           (bucket, key, version_id, seq, is_delete_marker, created_at)
                         VALUES (?1, ?2, ?3, ?4, 1, ?5)",
                        params![bucket, key, mid, seq, now_iso()],
                    )?;
                    DeleteOutcome {
                        version_id: Some(mid),
                        delete_marker: true,
                        seq,
                        versioning,
                    }
                }
            };
            tx.commit()?;
            Ok(out)
        })
        .await
    }

    /// Replace the tag set of a version; returns the version id touched.
    pub async fn set_object_tags(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
        tags: Tags,
    ) -> S3Result<ObjectInfo> {
        let (bucket, key, vid) = (
            bucket.to_string(),
            key.to_string(),
            version_id.map(str::to_string),
        );
        self.run(move |conn| {
            let info = resolve_version(conn, &bucket, &key, vid.as_deref())?;
            let json = tags_to_json(&tags);
            for table in ["s3_objects", "s3_object_versions"] {
                conn.execute(
                    &format!(
                        "UPDATE {table} SET tags = ?4 WHERE bucket = ?1 AND key = ?2 AND version_id = ?3 AND seq = ?5"
                    ),
                    params![bucket, key, info.version_id, json, info.seq],
                )?;
            }
            Ok(info)
        })
        .await
    }

    // ── Multipart uploads ─────────────────────────────────────────────────

    pub async fn create_upload(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        meta: ObjMeta,
        tags: Tags,
    ) -> S3Result<Upload> {
        let (bucket, key, ct) = (
            bucket.to_string(),
            key.to_string(),
            content_type.to_string(),
        );
        self.run(move |conn| {
            require_bucket(conn, &bucket)?;
            let mut meta = meta;
            apply_default_sse(conn, &bucket, &mut meta)?;
            let upload = Upload {
                upload_id: uuid::Uuid::new_v4().simple().to_string(),
                bucket,
                key,
                content_type: ct,
                meta,
                tags,
                created_at: now_iso(),
            };
            conn.execute(
                "INSERT INTO s3_multipart_uploads (upload_id, bucket, key, content_type, meta, tags, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    upload.upload_id,
                    upload.bucket,
                    upload.key,
                    upload.content_type,
                    upload.meta.to_json(),
                    tags_to_json(&upload.tags),
                    upload.created_at
                ],
            )?;
            Ok(upload)
        })
        .await
    }

    pub async fn get_upload(&self, bucket: &str, key: &str, upload_id: &str) -> S3Result<Upload> {
        let (bucket, key, id) = (bucket.to_string(), key.to_string(), upload_id.to_string());
        self.run(move |conn| upload_row(conn, &bucket, &key, &id))
            .await
    }

    pub async fn upload_part(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        part_number: i64,
        content: Vec<u8>,
        meta: ObjMeta,
    ) -> S3Result<String> {
        if !(1..=10000).contains(&part_number) {
            return Err(S3Error::invalid_argument(
                "Part number must be an integer between 1 and 10000, inclusive",
            )
            .with("ArgumentName", "partNumber")
            .with("ArgumentValue", part_number.to_string()));
        }
        let (bucket, key, id) = (bucket.to_string(), key.to_string(), upload_id.to_string());
        let etag = md5_etag(&content);
        let etag2 = etag.clone();
        self.run(move |conn| {
            upload_row(conn, &bucket, &key, &id)?;
            let size = content.len() as i64;
            conn.execute(
                "INSERT OR REPLACE INTO s3_multipart_parts
                   (upload_id, part_number, content, size, etag, meta, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    part_number,
                    content,
                    size,
                    etag2,
                    meta.to_json(),
                    now_iso()
                ],
            )?;
            Ok(())
        })
        .await?;
        Ok(etag)
    }

    pub async fn list_parts(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
    ) -> S3Result<(Upload, Vec<Part>)> {
        let (bucket, key, id) = (bucket.to_string(), key.to_string(), upload_id.to_string());
        self.run(move |conn| {
            let up = upload_row(conn, &bucket, &key, &id)?;
            let mut stmt = conn.prepare(
                "SELECT part_number, size, etag, created_at, meta FROM s3_multipart_parts
                 WHERE upload_id = ?1 ORDER BY part_number",
            )?;
            let parts = stmt
                .query_map(params![id], |r| {
                    Ok(Part {
                        part_number: r.get(0)?,
                        size: r.get(1)?,
                        etag: r.get(2)?,
                        created_at: r.get(3)?,
                        meta: ObjMeta::from_json(&r.get::<_, String>(4)?),
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok((up, parts))
        })
        .await
    }

    /// Assemble the requested parts and write the final object.
    pub async fn complete_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        requested: Vec<(i64, String)>,
    ) -> S3Result<WriteResult> {
        if requested.is_empty() {
            return Err(S3Error::malformed_xml());
        }
        for w in requested.windows(2) {
            if w[1].0 <= w[0].0 {
                return Err(S3Error::new(
                    "InvalidPartOrder",
                    "The list of parts was not in ascending order. Parts must be ordered by part number.",
                )
                .with("UploadId", upload_id));
            }
        }
        let (bucket, key, id) = (bucket.to_string(), key.to_string(), upload_id.to_string());
        self.run(move |conn| {
            let up = upload_row(conn, &bucket, &key, &id)?;
            let mut content = Vec::new();
            let mut md5s = Vec::new();
            let mut sizes = Vec::new();
            for (num, etag) in &requested {
                let row: Option<(Vec<u8>, String)> = conn
                    .query_row(
                        "SELECT content, etag FROM s3_multipart_parts WHERE upload_id = ?1 AND part_number = ?2",
                        params![id, num],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                let want = etag.trim().trim_matches('"');
                match row {
                    Some((data, stored)) if stored.trim_matches('"') == want => {
                        md5s.extend_from_slice(&hex::decode(stored.trim_matches('"')).unwrap_or_default());
                        sizes.push(data.len() as i64);
                        content.extend_from_slice(&data);
                    }
                    _ => {
                        return Err(S3Error::new(
                            "InvalidPart",
                            "One or more of the specified parts could not be found.  The part may not have been uploaded, or the specified entity tag may not have matched the part's entity tag.",
                        )
                        .with("UploadId", id.clone())
                        .with("PartNumber", num.to_string())
                        .with("ETag", etag.clone()))
                    }
                }
            }
            let etag = format!("\"{}-{}\"", hex::encode(Md5::digest(&md5s)), requested.len());
            let mut meta = up.meta.clone();
            meta.parts = sizes;
            meta.checksums.clear();
            let result = write_object_tx(
                conn,
                &bucket,
                &key,
                NewObject {
                    content,
                    content_type: up.content_type.clone(),
                    etag,
                    meta,
                    tags: up.tags.clone(),
                },
            )?;
            conn.execute("DELETE FROM s3_multipart_parts WHERE upload_id = ?1", params![id])?;
            conn.execute("DELETE FROM s3_multipart_uploads WHERE upload_id = ?1", params![id])?;
            Ok(result)
        })
        .await
    }

    pub async fn abort_upload(&self, bucket: &str, key: &str, upload_id: &str) -> S3Result<()> {
        let (bucket, key, id) = (bucket.to_string(), key.to_string(), upload_id.to_string());
        self.run(move |conn| {
            upload_row(conn, &bucket, &key, &id)?;
            conn.execute(
                "DELETE FROM s3_multipart_parts WHERE upload_id = ?1",
                params![id],
            )?;
            conn.execute(
                "DELETE FROM s3_multipart_uploads WHERE upload_id = ?1",
                params![id],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn list_uploads(&self, bucket: &str, prefix: &str) -> S3Result<Vec<Upload>> {
        let (bucket, prefix) = (bucket.to_string(), prefix.to_string());
        self.run(move |conn| {
            require_bucket(conn, &bucket)?;
            let mut stmt = conn.prepare(
                "SELECT upload_id, bucket, key, content_type, meta, tags, created_at
                 FROM s3_multipart_uploads WHERE bucket = ?1 ORDER BY key, created_at",
            )?;
            let rows = stmt
                .query_map(params![bucket], upload_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows
                .into_iter()
                .filter(|u| u.key.starts_with(&prefix))
                .collect())
        })
        .await
    }
}

fn upload_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Upload> {
    Ok(Upload {
        upload_id: r.get(0)?,
        bucket: r.get(1)?,
        key: r.get(2)?,
        content_type: r.get(3)?,
        meta: ObjMeta::from_json(&r.get::<_, String>(4)?),
        tags: tags_from_json(&r.get::<_, String>(5)?),
        created_at: r.get(6)?,
    })
}

fn upload_row(conn: &Connection, bucket: &str, key: &str, id: &str) -> S3Result<Upload> {
    require_bucket(conn, bucket)?;
    conn.query_row(
        "SELECT upload_id, bucket, key, content_type, meta, tags, created_at
         FROM s3_multipart_uploads WHERE upload_id = ?1 AND bucket = ?2 AND key = ?3",
        params![id, bucket, key],
        upload_from_row,
    )
    .optional()?
    .ok_or_else(|| S3Error::no_such_upload(id))
}

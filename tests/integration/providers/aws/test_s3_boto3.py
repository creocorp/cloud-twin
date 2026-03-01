"""
Integration tests: AWS S3 via boto3.

Tests exercise the full HTTP + XML stack against a real uvicorn server.
Bucket names are unique per test to avoid cross-test state conflicts
(all tests share a single in-memory server via the session fixture).
"""

from __future__ import annotations

import json

import pytest
from botocore.exceptions import ClientError

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _bucket(suffix: str) -> str:
    """Generate a deterministic unique bucket name per test."""
    return f"integ-{suffix}"


# ---------------------------------------------------------------------------
# CreateBucket / ListBuckets
# ---------------------------------------------------------------------------


def test_create_bucket(s3):
    resp = s3.create_bucket(Bucket=_bucket("create-basic"))
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200


def test_list_buckets_contains_created(s3):
    name = _bucket("list-check")
    s3.create_bucket(Bucket=name)
    resp = s3.list_buckets()
    names = [b["Name"] for b in resp.get("Buckets", [])]
    assert name in names


def test_list_buckets_owner_present(s3):
    resp = s3.list_buckets()
    assert "Owner" in resp
    assert resp["Owner"]["ID"] == "cloudtwin"


def test_create_multiple_buckets_all_appear_in_list(s3):
    buckets = [_bucket(f"multi-{i}") for i in range(3)]
    for b in buckets:
        s3.create_bucket(Bucket=b)

    listed = {b["Name"] for b in s3.list_buckets().get("Buckets", [])}
    assert set(buckets).issubset(listed)


# ---------------------------------------------------------------------------
# PutObject / GetObject
# ---------------------------------------------------------------------------


def test_put_and_get_text_object(s3):
    bucket = _bucket("put-get-text")
    s3.create_bucket(Bucket=bucket)

    body = b"hello from boto3"
    s3.put_object(Bucket=bucket, Key="hello.txt", Body=body, ContentType="text/plain")

    resp = s3.get_object(Bucket=bucket, Key="hello.txt")
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    assert resp["Body"].read() == body
    assert resp["ContentType"] == "text/plain"


def test_put_and_get_binary_object(s3):
    bucket = _bucket("put-get-bin")
    s3.create_bucket(Bucket=bucket)

    data = bytes(range(256))
    s3.put_object(Bucket=bucket, Key="binary.bin", Body=data)

    resp = s3.get_object(Bucket=bucket, Key="binary.bin")
    assert resp["Body"].read() == data


def test_put_and_get_json_object(s3):
    bucket = _bucket("put-get-json")
    s3.create_bucket(Bucket=bucket)

    payload = json.dumps({"service": "cloudtwin", "version": "0.1"}).encode()
    s3.put_object(
        Bucket=bucket, Key="config.json", Body=payload, ContentType="application/json"
    )

    resp = s3.get_object(Bucket=bucket, Key="config.json")
    assert json.loads(resp["Body"].read()) == {"service": "cloudtwin", "version": "0.1"}


def test_put_and_get_nested_key(s3):
    bucket = _bucket("nested-key")
    s3.create_bucket(Bucket=bucket)

    s3.put_object(Bucket=bucket, Key="a/b/c/deep.txt", Body=b"deep")
    resp = s3.get_object(Bucket=bucket, Key="a/b/c/deep.txt")
    assert resp["Body"].read() == b"deep"


def test_put_object_overwrites(s3):
    bucket = _bucket("overwrite")
    s3.create_bucket(Bucket=bucket)

    s3.put_object(Bucket=bucket, Key="file.txt", Body=b"v1")
    s3.put_object(Bucket=bucket, Key="file.txt", Body=b"v2")

    resp = s3.get_object(Bucket=bucket, Key="file.txt")
    assert resp["Body"].read() == b"v2"


def test_put_object_returns_etag(s3):
    bucket = _bucket("etag-check")
    s3.create_bucket(Bucket=bucket)

    resp = s3.put_object(Bucket=bucket, Key="etag.txt", Body=b"etag me")
    assert "ETag" in resp


def test_get_object_not_found_raises(s3):
    bucket = _bucket("missing-key")
    s3.create_bucket(Bucket=bucket)

    with pytest.raises(ClientError) as exc_info:
        s3.get_object(Bucket=bucket, Key="does-not-exist.txt")
    assert exc_info.value.response["Error"]["Code"] == "NoSuchKey"


def test_get_object_bucket_not_found_raises(s3):
    with pytest.raises(ClientError):
        s3.get_object(Bucket=_bucket("no-such-bucket-xyz"), Key="file.txt")


# ---------------------------------------------------------------------------
# DeleteObject
# ---------------------------------------------------------------------------


def test_delete_object(s3):
    bucket = _bucket("delete-obj")
    s3.create_bucket(Bucket=bucket)
    s3.put_object(Bucket=bucket, Key="to-delete.txt", Body=b"bye")

    s3.delete_object(Bucket=bucket, Key="to-delete.txt")

    with pytest.raises(ClientError) as exc_info:
        s3.get_object(Bucket=bucket, Key="to-delete.txt")
    assert exc_info.value.response["Error"]["Code"] == "NoSuchKey"


def test_delete_object_idempotent(s3):
    """Deleting a non-existent key should not raise."""
    bucket = _bucket("delete-idempotent")
    s3.create_bucket(Bucket=bucket)
    s3.delete_object(Bucket=bucket, Key="ghost.txt")  # no error expected


# ---------------------------------------------------------------------------
# ListObjectsV2
# ---------------------------------------------------------------------------


def test_list_objects_v2_empty_bucket(s3):
    bucket = _bucket("list-empty")
    s3.create_bucket(Bucket=bucket)

    resp = s3.list_objects_v2(Bucket=bucket)
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    assert resp.get("KeyCount", 0) == 0 or resp.get("Contents") is None


def test_list_objects_v2_returns_keys(s3):
    bucket = _bucket("list-keys")
    s3.create_bucket(Bucket=bucket)
    s3.put_object(Bucket=bucket, Key="a.txt", Body=b"a")
    s3.put_object(Bucket=bucket, Key="b.txt", Body=b"b")
    s3.put_object(Bucket=bucket, Key="c.txt", Body=b"c")

    resp = s3.list_objects_v2(Bucket=bucket)
    keys = {obj["Key"] for obj in resp.get("Contents", [])}
    assert keys == {"a.txt", "b.txt", "c.txt"}


def test_list_objects_v2_prefix_filter(s3):
    bucket = _bucket("list-prefix")
    s3.create_bucket(Bucket=bucket)
    s3.put_object(Bucket=bucket, Key="logs/jan.txt", Body=b"jan")
    s3.put_object(Bucket=bucket, Key="logs/feb.txt", Body=b"feb")
    s3.put_object(Bucket=bucket, Key="data/file.txt", Body=b"data")

    resp = s3.list_objects_v2(Bucket=bucket, Prefix="logs/")
    keys = [obj["Key"] for obj in resp.get("Contents", [])]
    assert set(keys) == {"logs/jan.txt", "logs/feb.txt"}
    assert all(k.startswith("logs/") for k in keys)


def test_list_objects_v2_each_entry_has_size(s3):
    bucket = _bucket("list-size")
    s3.create_bucket(Bucket=bucket)
    s3.put_object(Bucket=bucket, Key="sized.txt", Body=b"12345")

    resp = s3.list_objects_v2(Bucket=bucket)
    contents = resp.get("Contents", [])
    assert len(contents) == 1
    assert contents[0]["Size"] == 5


def test_list_objects_v2_bucket_not_found_raises(s3):
    with pytest.raises(ClientError) as exc_info:
        s3.list_objects_v2(Bucket=_bucket("no-such-list-bucket"))
    assert exc_info.value.response["Error"]["Code"] == "NoSuchBucket"

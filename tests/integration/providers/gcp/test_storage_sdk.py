"""GCP Cloud Storage integration tests using the google-cloud-storage SDK."""

from __future__ import annotations

import io
import uuid

_PROJECT = "cloudtwin-local"


def _name() -> str:
    return f"ct-bucket-{uuid.uuid4().hex[:8]}"


def _obj() -> str:
    return f"obj-{uuid.uuid4().hex[:8]}.txt"


class TestBuckets:
    def test_create_bucket(self, gcs):
        bucket = gcs.create_bucket(_name())
        assert bucket.name is not None

    def test_list_buckets(self, gcs):
        name = _name()
        gcs.create_bucket(name)
        names = [b.name for b in gcs.list_buckets()]
        assert name in names

    def test_get_bucket(self, gcs):
        name = _name()
        gcs.create_bucket(name)
        b = gcs.get_bucket(name)
        assert b.name == name

    def test_delete_bucket(self, gcs):
        name = _name()
        gcs.create_bucket(name)
        gcs.get_bucket(name).delete()
        from google.cloud.exceptions import NotFound
        try:
            gcs.get_bucket(name)
            assert False, "Expected NotFound"
        except NotFound:
            pass


class TestObjects:
    def test_upload_and_download(self, gcs):
        bucket_name = _name()
        gcs.create_bucket(bucket_name)
        bucket = gcs.bucket(bucket_name)

        blob = bucket.blob(_obj())
        content = b"Hello, GCP CloudTwin!"
        blob.upload_from_string(content, content_type="text/plain")

        downloaded = blob.download_as_bytes()
        assert downloaded == content

    def test_list_blobs(self, gcs):
        bucket_name = _name()
        gcs.create_bucket(bucket_name)
        bucket = gcs.bucket(bucket_name)

        names = [f"file-{i}.txt" for i in range(3)]
        for n in names:
            bucket.blob(n).upload_from_string(f"content {n}".encode())

        blobs = list(gcs.list_blobs(bucket_name))
        blob_names = [b.name for b in blobs]
        for n in names:
            assert n in blob_names

    def test_upload_binary(self, gcs):
        bucket_name = _name()
        gcs.create_bucket(bucket_name)
        bucket = gcs.bucket(bucket_name)

        data = bytes(range(256))
        blob = bucket.blob("binary.bin")
        blob.upload_from_string(data, content_type="application/octet-stream")
        assert blob.download_as_bytes() == data

    def test_upload_from_file(self, gcs):
        bucket_name = _name()
        gcs.create_bucket(bucket_name)
        blob = gcs.bucket(bucket_name).blob("fromfile.txt")
        blob.upload_from_file(io.BytesIO(b"from file"), content_type="text/plain")
        assert blob.download_as_bytes() == b"from file"

    def test_overwrite_object(self, gcs):
        bucket_name = _name()
        gcs.create_bucket(bucket_name)
        blob = gcs.bucket(bucket_name).blob("overwrite.txt")
        blob.upload_from_string(b"v1")
        blob.upload_from_string(b"v2")
        assert blob.download_as_bytes() == b"v2"

    def test_delete_object(self, gcs):
        bucket_name = _name()
        gcs.create_bucket(bucket_name)
        blob = gcs.bucket(bucket_name).blob("todelete.txt")
        blob.upload_from_string(b"bye")
        blob.delete()
        from google.cloud.exceptions import NotFound
        try:
            blob.download_as_bytes()
            assert False, "Expected NotFound"
        except NotFound:
            pass

    def test_prefix_listing(self, gcs):
        bucket_name = _name()
        gcs.create_bucket(bucket_name)
        bucket = gcs.bucket(bucket_name)
        bucket.blob("prefix/a.txt").upload_from_string(b"a")
        bucket.blob("prefix/b.txt").upload_from_string(b"b")
        bucket.blob("other/c.txt").upload_from_string(b"c")

        blobs = list(gcs.list_blobs(bucket_name, prefix="prefix/"))
        names = [b.name for b in blobs]
        assert "prefix/a.txt" in names
        assert "prefix/b.txt" in names
        assert "other/c.txt" not in names

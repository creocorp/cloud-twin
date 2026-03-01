"""Azure Blob Storage integration tests using the azure-storage-blob SDK."""

from __future__ import annotations

import uuid


def _name() -> str:
    return f"ct-{uuid.uuid4().hex[:8]}"


class TestContainers:
    def test_create_container(self, blob_client):
        container_name = _name()
        cc = blob_client.get_container_client(container_name)
        cc.create_container()

    def test_list_containers(self, blob_client):
        container_name = _name()
        blob_client.get_container_client(container_name).create_container()
        containers = [c["name"] for c in blob_client.list_containers()]
        assert container_name in containers

    def test_container_exists(self, blob_client):
        container_name = _name()
        cc = blob_client.get_container_client(container_name)
        cc.create_container()
        assert cc.exists()

    def test_delete_container(self, blob_client):
        container_name = _name()
        cc = blob_client.get_container_client(container_name)
        cc.create_container()
        cc.delete_container()
        assert not cc.exists()


class TestBlobs:
    def test_upload_and_download_blob(self, blob_client):
        container = _name()
        blob_client.get_container_client(container).create_container()
        bc = blob_client.get_blob_client(container=container, blob="hello.txt")
        bc.upload_blob(b"Hello, CloudTwin!", overwrite=True)
        downloaded = bc.download_blob().readall()
        assert downloaded == b"Hello, CloudTwin!"

    def test_list_blobs(self, blob_client):
        container = _name()
        blob_client.get_container_client(container).create_container()
        for i in range(3):
            blob_client.get_blob_client(
                container=container, blob=f"file-{i}.txt"
            ).upload_blob(f"content {i}".encode(), overwrite=True)
        names = [
            b["name"] for b in blob_client.get_container_client(container).list_blobs()
        ]
        assert "file-0.txt" in names
        assert "file-1.txt" in names
        assert "file-2.txt" in names

    def test_overwrite_blob(self, blob_client):
        container = _name()
        blob_client.get_container_client(container).create_container()
        bc = blob_client.get_blob_client(container=container, blob="data.bin")
        bc.upload_blob(b"v1", overwrite=True)
        bc.upload_blob(b"v2", overwrite=True)
        assert bc.download_blob().readall() == b"v2"

    def test_delete_blob(self, blob_client):
        container = _name()
        blob_client.get_container_client(container).create_container()
        bc = blob_client.get_blob_client(container=container, blob="todel.txt")
        bc.upload_blob(b"bye", overwrite=True)
        bc.delete_blob()
        from azure.core.exceptions import ResourceNotFoundError

        try:
            bc.download_blob().readall()
            assert False, "Expected ResourceNotFoundError"
        except ResourceNotFoundError:
            pass

    def test_blob_properties(self, blob_client):
        container = _name()
        blob_client.get_container_client(container).create_container()
        bc = blob_client.get_blob_client(container=container, blob="props.txt")
        bc.upload_blob(b"test content", overwrite=True)
        props = bc.get_blob_properties()
        assert props["size"] == len(b"test content")

    def test_binary_blob(self, blob_client):
        container = _name()
        blob_client.get_container_client(container).create_container()
        data = bytes(range(256))
        bc = blob_client.get_blob_client(container=container, blob="binary.bin")
        bc.upload_blob(data, overwrite=True)
        assert bc.download_blob().readall() == data

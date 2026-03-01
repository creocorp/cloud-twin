"""Integration tests for Azure Queue Storage (httpx)."""

from __future__ import annotations

import pytest

_ACCOUNT = "testaccount"


class TestCreateQueue:
    def test_create_returns_201(self, azure_http):
        r = azure_http.put(f"/azure/queue/{_ACCOUNT}/test-queue-create")
        assert r.status_code == 201

    def test_create_idempotent(self, azure_http):
        azure_http.put(f"/azure/queue/{_ACCOUNT}/idem-queue")
        r = azure_http.put(f"/azure/queue/{_ACCOUNT}/idem-queue")
        assert r.status_code in (200, 201, 409)

    def test_list_queues_contains_created(self, azure_http):
        azure_http.put(f"/azure/queue/{_ACCOUNT}/list-queue-1")
        azure_http.put(f"/azure/queue/{_ACCOUNT}/list-queue-2")
        r = azure_http.get(f"/azure/queue/{_ACCOUNT}")
        assert r.status_code == 200
        names = [q["Name"] for q in r.json()["QueueItems"]]
        assert "list-queue-1" in names
        assert "list-queue-2" in names

    def test_delete_queue(self, azure_http):
        azure_http.put(f"/azure/queue/{_ACCOUNT}/delete-me-queue")
        r = azure_http.delete(f"/azure/queue/{_ACCOUNT}/delete-me-queue")
        assert r.status_code == 204

    def test_delete_nonexistent_returns_404(self, azure_http):
        r = azure_http.delete(f"/azure/queue/{_ACCOUNT}/no-such-queue-xyz")
        assert r.status_code == 404


class TestMessages:
    def test_send_message_returns_201(self, azure_http):
        azure_http.put(f"/azure/queue/{_ACCOUNT}/msg-queue")
        r = azure_http.post(
            f"/azure/queue/{_ACCOUNT}/msg-queue/messages", content=b"hello"
        )
        assert r.status_code == 201
        body = r.json()
        assert "MessageId" in body

    def test_receive_message(self, azure_http):
        azure_http.put(f"/azure/queue/{_ACCOUNT}/recv-queue")
        azure_http.post(
            f"/azure/queue/{_ACCOUNT}/recv-queue/messages", content=b"world"
        )
        r = azure_http.get(f"/azure/queue/{_ACCOUNT}/recv-queue/messages")
        assert r.status_code == 200
        msgs = r.json()["QueueMessagesList"]
        assert len(msgs) >= 1
        assert msgs[0]["MessageText"] == "world"

    def test_peek_message(self, azure_http):
        azure_http.put(f"/azure/queue/{_ACCOUNT}/peek-queue")
        azure_http.post(
            f"/azure/queue/{_ACCOUNT}/peek-queue/messages", content=b"peek-me"
        )
        r = azure_http.get(f"/azure/queue/{_ACCOUNT}/peek-queue/messages/peek")
        assert r.status_code == 200
        msgs = r.json()["QueueMessagesList"]
        assert any(m["MessageText"] == "peek-me" for m in msgs)

    def test_delete_message(self, azure_http):
        azure_http.put(f"/azure/queue/{_ACCOUNT}/delmsg-queue")
        send_r = azure_http.post(
            f"/azure/queue/{_ACCOUNT}/delmsg-queue/messages", content=b"bye"
        )
        mid = send_r.json()["MessageId"]
        r = azure_http.delete(f"/azure/queue/{_ACCOUNT}/delmsg-queue/messages/{mid}")
        assert r.status_code == 204

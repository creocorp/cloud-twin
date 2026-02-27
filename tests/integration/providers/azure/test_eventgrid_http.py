"""Integration tests for Azure Event Grid (httpx)."""

from __future__ import annotations

import pytest


class TestTopics:
    def test_create_topic_returns_201(self, azure_http):
        r = azure_http.put(
            "/azure/eventgrid/topics/eg-topic-create",
            json={"endpoint": "https://eg-topic-create.eventgrid.azure.net/api/events"},
        )
        assert r.status_code == 201
        body = r.json()
        assert body["name"] == "eg-topic-create"

    def test_create_topic_idempotent(self, azure_http):
        azure_http.put("/azure/eventgrid/topics/eg-idem", json={})
        r = azure_http.put("/azure/eventgrid/topics/eg-idem", json={})
        assert r.status_code in (200, 201, 409)

    def test_list_topics(self, azure_http):
        azure_http.put("/azure/eventgrid/topics/eg-list-1", json={})
        azure_http.put("/azure/eventgrid/topics/eg-list-2", json={})
        r = azure_http.get("/azure/eventgrid/topics")
        assert r.status_code == 200
        names = [t["name"] for t in r.json()["value"]]
        assert "eg-list-1" in names
        assert "eg-list-2" in names

    def test_delete_topic(self, azure_http):
        azure_http.put("/azure/eventgrid/topics/eg-delete-me", json={})
        r = azure_http.delete("/azure/eventgrid/topics/eg-delete-me")
        assert r.status_code == 204

    def test_delete_nonexistent_returns_404(self, azure_http):
        r = azure_http.delete("/azure/eventgrid/topics/eg-no-such-topic")
        assert r.status_code == 404


class TestEvents:
    def test_publish_returns_published_count(self, azure_http):
        azure_http.put("/azure/eventgrid/topics/eg-pub-topic", json={})
        events = [
            {"id": "e1", "eventType": "TestEvent", "subject": "sub/1", "data": {"key": "v1"}},
            {"id": "e2", "eventType": "TestEvent", "subject": "sub/2", "data": {"key": "v2"}},
        ]
        r = azure_http.post("/azure/eventgrid/topics/eg-pub-topic/events", json=events)
        assert r.status_code == 200
        assert r.json()["published"] == 2

    def test_list_events(self, azure_http):
        azure_http.put("/azure/eventgrid/topics/eg-list-events", json={})
        azure_http.post(
            "/azure/eventgrid/topics/eg-list-events/events",
            json=[{"id": "x1", "eventType": "T", "subject": "s", "data": {}}],
        )
        r = azure_http.get("/azure/eventgrid/topics/eg-list-events/events")
        assert r.status_code == 200
        assert len(r.json()["value"]) >= 1

    def test_publish_to_nonexistent_topic_returns_404(self, azure_http):
        r = azure_http.post(
            "/azure/eventgrid/topics/eg-no-topic-xyz/events",
            json=[{"id": "x", "eventType": "T", "subject": "s", "data": {}}],
        )
        assert r.status_code == 404

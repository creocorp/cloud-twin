"""Integration tests for GCP Cloud Tasks (httpx)."""

from __future__ import annotations

_PROJECT = "cloudtwin-local"
_LOCATION = "us-central1"


def _queues_path() -> str:
    return f"/v2/projects/{_PROJECT}/locations/{_LOCATION}/queues"


def _queue_path(name: str) -> str:
    return f"{_queues_path()}/{name}"


def _tasks_path(queue: str) -> str:
    return f"{_queue_path(queue)}/tasks"


class TestQueues:
    def test_create_queue(self, gcp_http):
        r = gcp_http.post(
            _queues_path(),
            json={"name": f"projects/{_PROJECT}/locations/{_LOCATION}/queues/ct-q-create"},
        )
        assert r.status_code == 200
        assert "ct-q-create" in r.json()["name"]

    def test_list_queues(self, gcp_http):
        gcp_http.post(_queues_path(), json={"name": f"projects/{_PROJECT}/locations/{_LOCATION}/queues/ct-q-list-1"})
        gcp_http.post(_queues_path(), json={"name": f"projects/{_PROJECT}/locations/{_LOCATION}/queues/ct-q-list-2"})
        r = gcp_http.get(_queues_path())
        assert r.status_code == 200
        names = [q["name"] for q in r.json()["queues"]]
        assert any("ct-q-list-1" in n for n in names)
        assert any("ct-q-list-2" in n for n in names)

    def test_get_queue(self, gcp_http):
        gcp_http.post(_queues_path(), json={"name": f"projects/{_PROJECT}/locations/{_LOCATION}/queues/ct-q-get"})
        r = gcp_http.get(_queue_path("ct-q-get"))
        assert r.status_code == 200
        assert "ct-q-get" in r.json()["name"]

    def test_get_nonexistent_returns_404(self, gcp_http):
        r = gcp_http.get(_queue_path("ct-q-no-such"))
        assert r.status_code == 404

    def test_delete_queue(self, gcp_http):
        gcp_http.post(_queues_path(), json={"name": f"projects/{_PROJECT}/locations/{_LOCATION}/queues/ct-q-del"})
        r = gcp_http.delete(_queue_path("ct-q-del"))
        assert r.status_code == 200

    def test_delete_nonexistent_returns_404(self, gcp_http):
        r = gcp_http.delete(_queue_path("ct-q-no-del"))
        assert r.status_code == 404


class TestTasks:
    def _ensure_queue(self, gcp_http, name):
        gcp_http.post(
            _queues_path(),
            json={"name": f"projects/{_PROJECT}/locations/{_LOCATION}/queues/{name}"},
        )

    def test_create_task(self, gcp_http):
        self._ensure_queue(gcp_http, "ct-task-q")
        r = gcp_http.post(_tasks_path("ct-task-q"), json={"task": {"httpRequest": {"url": "http://example.com"}}})
        assert r.status_code == 200
        assert "name" in r.json()

    def test_list_tasks(self, gcp_http):
        self._ensure_queue(gcp_http, "ct-list-q")
        gcp_http.post(_tasks_path("ct-list-q"), json={"task": {}})
        gcp_http.post(_tasks_path("ct-list-q"), json={"task": {}})
        r = gcp_http.get(_tasks_path("ct-list-q"))
        assert r.status_code == 200
        assert len(r.json()["tasks"]) >= 2

    def test_delete_task(self, gcp_http):
        self._ensure_queue(gcp_http, "ct-del-task-q")
        create_r = gcp_http.post(_tasks_path("ct-del-task-q"), json={"task": {}})
        task_id = create_r.json()["name"]
        r = gcp_http.delete(f"{_tasks_path('ct-del-task-q')}/{task_id}")
        assert r.status_code == 200

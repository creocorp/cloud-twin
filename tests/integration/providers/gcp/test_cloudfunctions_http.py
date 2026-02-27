"""Integration tests for GCP Cloud Functions (httpx)."""

from __future__ import annotations

_PROJECT = "cloudtwin-local"
_LOCATION = "us-central1"


def _functions_path() -> str:
    return f"/v2/projects/{_PROJECT}/locations/{_LOCATION}/functions"


def _function_path(name: str) -> str:
    return f"{_functions_path()}/{name}"


class TestFunctions:
    def test_create_function(self, gcp_http):
        r = gcp_http.post(
            _functions_path(),
            json={
                "name": f"projects/{_PROJECT}/locations/{_LOCATION}/functions/gcf-create",
                "buildConfig": {"runtime": "python311", "entryPoint": "main"},
            },
        )
        assert r.status_code == 200
        body = r.json()
        assert "gcf-create" in body["name"]
        assert body["runtime"] == "python311"

    def test_create_via_query_param(self, gcp_http):
        r = gcp_http.post(
            f"{_functions_path()}?functionId=gcf-qp",
            json={"buildConfig": {"runtime": "nodejs18"}},
        )
        assert r.status_code == 200
        assert "gcf-qp" in r.json()["name"]

    def test_list_functions(self, gcp_http):
        gcp_http.post(
            _functions_path(),
            json={"name": f"projects/{_PROJECT}/locations/{_LOCATION}/functions/gcf-list-1"},
        )
        gcp_http.post(
            _functions_path(),
            json={"name": f"projects/{_PROJECT}/locations/{_LOCATION}/functions/gcf-list-2"},
        )
        r = gcp_http.get(_functions_path())
        assert r.status_code == 200
        names = [f["name"] for f in r.json()["functions"]]
        assert any("gcf-list-1" in n for n in names)
        assert any("gcf-list-2" in n for n in names)

    def test_get_function(self, gcp_http):
        gcp_http.post(
            _functions_path(),
            json={"name": f"projects/{_PROJECT}/locations/{_LOCATION}/functions/gcf-get-me"},
        )
        r = gcp_http.get(_function_path("gcf-get-me"))
        assert r.status_code == 200
        assert "gcf-get-me" in r.json()["name"]

    def test_get_nonexistent_returns_404(self, gcp_http):
        r = gcp_http.get(_function_path("gcf-no-such"))
        assert r.status_code == 404

    def test_delete_function(self, gcp_http):
        gcp_http.post(
            _functions_path(),
            json={"name": f"projects/{_PROJECT}/locations/{_LOCATION}/functions/gcf-delete-me"},
        )
        r = gcp_http.delete(_function_path("gcf-delete-me"))
        assert r.status_code == 200

    def test_delete_then_get_returns_404(self, gcp_http):
        gcp_http.post(
            _functions_path(),
            json={"name": f"projects/{_PROJECT}/locations/{_LOCATION}/functions/gcf-del-get"},
        )
        gcp_http.delete(_function_path("gcf-del-get"))
        r = gcp_http.get(_function_path("gcf-del-get"))
        assert r.status_code == 404

    def test_invoke_function(self, gcp_http):
        gcp_http.post(
            _functions_path(),
            json={"name": f"projects/{_PROJECT}/locations/{_LOCATION}/functions/gcf-invoke"},
        )
        r = gcp_http.post(
            f"{_function_path('gcf-invoke')}:call",
            content=b'{"key": "value"}',
        )
        assert r.status_code == 200
        assert "result" in r.json()

    def test_invoke_nonexistent_returns_404(self, gcp_http):
        r = gcp_http.post(
            f"{_function_path('gcf-no-invoke')}:call",
            content=b"{}",
        )
        assert r.status_code == 404

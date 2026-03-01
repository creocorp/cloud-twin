"""Integration tests for Azure Functions (httpx)."""

from __future__ import annotations

_APP = "cloudtwin-fn-app"


class TestFunctions:
    def test_create_returns_201(self, azure_http):
        r = azure_http.put(
            f"/azure/functions/{_APP}/functions/fn-create",
            json={"code": "def main(): return 'ok'"},
        )
        assert r.status_code == 201
        body = r.json()
        assert body["name"] == "fn-create"

    def test_list_functions(self, azure_http):
        azure_http.put(
            f"/azure/functions/{_APP}/functions/fn-list-1", json={"code": ""}
        )
        azure_http.put(
            f"/azure/functions/{_APP}/functions/fn-list-2", json={"code": ""}
        )
        r = azure_http.get(f"/azure/functions/{_APP}/functions")
        assert r.status_code == 200
        names = [f["name"] for f in r.json()["value"]]
        assert "fn-list-1" in names
        assert "fn-list-2" in names

    def test_get_function(self, azure_http):
        azure_http.put(
            f"/azure/functions/{_APP}/functions/fn-get-me", json={"code": ""}
        )
        r = azure_http.get(f"/azure/functions/{_APP}/functions/fn-get-me")
        assert r.status_code == 200
        assert r.json()["name"] == "fn-get-me"

    def test_get_nonexistent_returns_404(self, azure_http):
        r = azure_http.get(f"/azure/functions/{_APP}/functions/fn-no-such")
        assert r.status_code == 404

    def test_delete_function(self, azure_http):
        azure_http.put(
            f"/azure/functions/{_APP}/functions/fn-delete-me", json={"code": ""}
        )
        r = azure_http.delete(f"/azure/functions/{_APP}/functions/fn-delete-me")
        assert r.status_code == 204

    def test_delete_then_get_returns_404(self, azure_http):
        azure_http.put(
            f"/azure/functions/{_APP}/functions/fn-del-get", json={"code": ""}
        )
        azure_http.delete(f"/azure/functions/{_APP}/functions/fn-del-get")
        r = azure_http.get(f"/azure/functions/{_APP}/functions/fn-del-get")
        assert r.status_code == 404

    def test_invoke_returns_result(self, azure_http):
        azure_http.put(
            f"/azure/functions/{_APP}/functions/fn-invoke",
            json={"code": "def main(): return 42"},
        )
        r = azure_http.post(
            f"/azure/functions/{_APP}/functions/fn-invoke/invoke",
            content=b'{"input": "data"}',
        )
        assert r.status_code == 200
        assert "result" in r.json()

    def test_invoke_nonexistent_returns_404(self, azure_http):
        r = azure_http.post(
            f"/azure/functions/{_APP}/functions/fn-no-invoke/invoke",
            content=b"{}",
        )
        assert r.status_code == 404

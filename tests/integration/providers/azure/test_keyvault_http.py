"""Integration tests for Azure Key Vault (httpx)."""

from __future__ import annotations

_VAULT = "cloudtwin-vault"


class TestSecrets:
    def test_set_secret_returns_200(self, azure_http):
        r = azure_http.put(
            f"/azure/keyvault/{_VAULT}/secrets/kv-secret-1",
            json={"value": "s3cr3t"},
        )
        assert r.status_code == 200
        body = r.json()
        assert body["value"] == "s3cr3t"
        assert "id" in body

    def test_get_secret(self, azure_http):
        azure_http.put(
            f"/azure/keyvault/{_VAULT}/secrets/kv-get-me", json={"value": "myval"}
        )
        r = azure_http.get(f"/azure/keyvault/{_VAULT}/secrets/kv-get-me")
        assert r.status_code == 200
        assert r.json()["value"] == "myval"

    def test_get_nonexistent_returns_404(self, azure_http):
        r = azure_http.get(f"/azure/keyvault/{_VAULT}/secrets/kv-no-such-secret")
        assert r.status_code == 404

    def test_list_secrets(self, azure_http):
        azure_http.put(
            f"/azure/keyvault/{_VAULT}/secrets/kv-list-a", json={"value": "a"}
        )
        azure_http.put(
            f"/azure/keyvault/{_VAULT}/secrets/kv-list-b", json={"value": "b"}
        )
        r = azure_http.get(f"/azure/keyvault/{_VAULT}/secrets")
        assert r.status_code == 200
        ids = [s["id"] for s in r.json()["value"]]
        assert any("kv-list-a" in i for i in ids)
        assert any("kv-list-b" in i for i in ids)

    def test_update_secret(self, azure_http):
        azure_http.put(
            f"/azure/keyvault/{_VAULT}/secrets/kv-update", json={"value": "old"}
        )
        azure_http.put(
            f"/azure/keyvault/{_VAULT}/secrets/kv-update", json={"value": "new"}
        )
        r = azure_http.get(f"/azure/keyvault/{_VAULT}/secrets/kv-update")
        assert r.status_code == 200
        assert r.json()["value"] == "new"

    def test_delete_secret(self, azure_http):
        azure_http.put(
            f"/azure/keyvault/{_VAULT}/secrets/kv-delete-me", json={"value": "bye"}
        )
        r = azure_http.delete(f"/azure/keyvault/{_VAULT}/secrets/kv-delete-me")
        assert r.status_code == 204

    def test_delete_then_get_returns_404(self, azure_http):
        azure_http.put(
            f"/azure/keyvault/{_VAULT}/secrets/kv-del-get", json={"value": "x"}
        )
        azure_http.delete(f"/azure/keyvault/{_VAULT}/secrets/kv-del-get")
        r = azure_http.get(f"/azure/keyvault/{_VAULT}/secrets/kv-del-get")
        assert r.status_code == 404

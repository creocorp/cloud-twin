"""Integration tests for GCP Secret Manager (httpx)."""

from __future__ import annotations

import base64

_PROJECT = "cloudtwin-local"


def _secrets_path() -> str:
    return f"/v1/projects/{_PROJECT}/secrets"


def _secret_path(name: str) -> str:
    return f"{_secrets_path()}/{name}"


class TestSecrets:
    def test_create_secret(self, gcp_http):
        r = gcp_http.post(_secrets_path(), json={"secretId": "sm-create-secret"})
        assert r.status_code == 200
        assert "sm-create-secret" in r.json()["name"]

    def test_create_secret_via_query_param(self, gcp_http):
        r = gcp_http.post(f"{_secrets_path()}?secretId=sm-qp-secret", json={})
        assert r.status_code == 200
        assert "sm-qp-secret" in r.json()["name"]

    def test_list_secrets(self, gcp_http):
        gcp_http.post(_secrets_path(), json={"secretId": "sm-list-1"})
        gcp_http.post(_secrets_path(), json={"secretId": "sm-list-2"})
        r = gcp_http.get(_secrets_path())
        assert r.status_code == 200
        names = [s["name"] for s in r.json()["secrets"]]
        assert any("sm-list-1" in n for n in names)
        assert any("sm-list-2" in n for n in names)

    def test_delete_secret(self, gcp_http):
        gcp_http.post(_secrets_path(), json={"secretId": "sm-delete-me"})
        r = gcp_http.delete(_secret_path("sm-delete-me"))
        assert r.status_code == 200

    def test_delete_nonexistent_returns_404(self, gcp_http):
        r = gcp_http.delete(_secret_path("sm-no-such"))
        assert r.status_code == 404


class TestSecretVersions:
    def _ensure_secret(self, gcp_http: object, name: str) -> None:
        gcp_http.post(_secrets_path(), json={"secretId": name})

    def test_add_version(self, gcp_http):
        self._ensure_secret(gcp_http, "sm-version-secret")
        payload = base64.b64encode(b"my-secret-value").decode()
        r = gcp_http.post(
            f"{_secret_path('sm-version-secret')}:addVersion",
            json={"payload": {"data": payload}},
        )
        assert r.status_code == 200
        assert "versions" in r.json()["name"]

    def test_access_version(self, gcp_http):
        self._ensure_secret(gcp_http, "sm-access-secret")
        raw = b"super-secret-data"
        gcp_http.post(
            f"{_secret_path('sm-access-secret')}:addVersion",
            json={"payload": {"data": base64.b64encode(raw).decode()}},
        )
        r = gcp_http.get(f"{_secret_path('sm-access-secret')}/versions/latest:access")
        assert r.status_code == 200
        decoded = base64.b64decode(r.json()["payload"]["data"])
        assert decoded == raw

    def test_access_nonexistent_secret_returns_404(self, gcp_http):
        r = gcp_http.get(f"{_secret_path('sm-no-secret')}/versions/latest:access")
        assert r.status_code == 404

    def test_add_version_to_nonexistent_returns_404(self, gcp_http):
        r = gcp_http.post(
            f"{_secret_path('sm-no-vers-secret')}:addVersion",
            json={"payload": {"data": base64.b64encode(b"x").decode()}},
        )
        assert r.status_code == 404

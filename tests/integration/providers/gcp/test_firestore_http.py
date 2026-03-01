"""Integration tests for GCP Firestore (httpx)."""

from __future__ import annotations

_PROJECT = "cloudtwin-local"
_COL = "test-collection"


def _doc_path(doc_id: str) -> str:
    return f"/v1/projects/{_PROJECT}/databases/(default)/documents/{_COL}/{doc_id}"


class TestDocuments:
    def test_set_document_returns_document(self, gcp_http):
        r = gcp_http.patch(
            _doc_path("doc-1"), json={"fields": {"name": {"stringValue": "Alice"}}}
        )
        assert r.status_code == 200
        body = r.json()
        assert "doc-1" in body["name"]
        assert "name" in body["fields"]

    def test_get_document(self, gcp_http):
        gcp_http.patch(
            _doc_path("doc-get"), json={"fields": {"x": {"integerValue": "7"}}}
        )
        r = gcp_http.get(_doc_path("doc-get"))
        assert r.status_code == 200
        assert "doc-get" in r.json()["name"]

    def test_get_nonexistent_returns_404(self, gcp_http):
        r = gcp_http.get(_doc_path("doc-no-such-xyz"))
        assert r.status_code == 404

    def test_list_documents(self, gcp_http):
        gcp_http.patch(_doc_path("doc-list-1"), json={"fields": {}})
        gcp_http.patch(_doc_path("doc-list-2"), json={"fields": {}})
        r = gcp_http.get(
            f"/v1/projects/{_PROJECT}/databases/(default)/documents/{_COL}"
        )
        assert r.status_code == 200
        names = [d["name"] for d in r.json()["documents"]]
        assert any("doc-list-1" in n for n in names)
        assert any("doc-list-2" in n for n in names)

    def test_delete_document(self, gcp_http):
        gcp_http.patch(_doc_path("doc-delete"), json={"fields": {}})
        r = gcp_http.delete(_doc_path("doc-delete"))
        assert r.status_code == 200

    def test_delete_then_get_returns_404(self, gcp_http):
        gcp_http.patch(_doc_path("doc-del-get"), json={"fields": {}})
        gcp_http.delete(_doc_path("doc-del-get"))
        r = gcp_http.get(_doc_path("doc-del-get"))
        assert r.status_code == 404

    def test_query_documents(self, gcp_http):
        col = "query-col"
        gcp_http.patch(
            f"/v1/projects/{_PROJECT}/databases/(default)/documents/{col}/qd-1",
            json={"fields": {"status": {"stringValue": "active"}}},
        )
        gcp_http.patch(
            f"/v1/projects/{_PROJECT}/databases/(default)/documents/{col}/qd-2",
            json={"fields": {"status": {"stringValue": "inactive"}}},
        )
        r = gcp_http.post(
            f"/v1/projects/{_PROJECT}/databases/(default)/documents/{col}:runQuery",
            json={
                "structuredQuery": {
                    "where": {
                        "fieldFilter": {
                            "field": {"fieldPath": "status"},
                            "op": "==",
                            "value": {"stringValue": "active"},
                        }
                    }
                }
            },
        )
        assert r.status_code == 200
        docs = r.json()["documents"]
        assert any("qd-1" in d["name"] for d in docs)
        assert not any("qd-2" in d["name"] for d in docs)

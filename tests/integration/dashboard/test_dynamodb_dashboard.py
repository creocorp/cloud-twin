"""
Dashboard endpoint tests: GET /api/dashboard/aws/dynamodb

These tests run over real HTTP against whichever backend is active:
- Python (FastAPI/uvicorn) — default, started in-process by conftest.py
- Rust  (cloudtwin-lite)  — set CLOUDTWIN_TEST_URL=http://localhost:4793

The test expectations are identical for both backends.
"""

from __future__ import annotations

import httpx
import pytest


@pytest.fixture(scope="module")
def dynamodb_resp(dashboard_url):
    """Fetch /api/dashboard/aws/dynamodb once and share across all tests."""
    return httpx.get(f"{dashboard_url}/api/dashboard/aws/dynamodb", timeout=5.0)


def test_dynamodb_status_ok(dynamodb_resp):
    assert dynamodb_resp.status_code == 200


def test_dynamodb_has_tables_key(dynamodb_resp):
    assert "tables" in dynamodb_resp.json()


def test_dynamodb_tables_is_list(dynamodb_resp):
    assert isinstance(dynamodb_resp.json()["tables"], list)

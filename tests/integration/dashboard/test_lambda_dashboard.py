"""
Dashboard endpoint tests: GET /api/dashboard/aws/lambda

These tests run over real HTTP against whichever backend is active:
- Python (FastAPI/uvicorn) — default, started in-process by conftest.py
- Rust  (cloudtwin-lite)  — set CLOUDTWIN_TEST_URL=http://localhost:4793

The test expectations are identical for both backends.
"""

from __future__ import annotations

import httpx
import pytest


@pytest.fixture(scope="module")
def lambda_resp(dashboard_url):
    """Fetch /api/dashboard/aws/lambda once and share across all tests."""
    return httpx.get(f"{dashboard_url}/api/dashboard/aws/lambda", timeout=5.0)


def test_lambda_status_ok(lambda_resp):
    assert lambda_resp.status_code == 200


def test_lambda_has_functions_key(lambda_resp):
    assert "functions" in lambda_resp.json()


def test_lambda_functions_is_list(lambda_resp):
    assert isinstance(lambda_resp.json()["functions"], list)

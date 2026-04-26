"""
Dashboard endpoint tests: GET /api/dashboard/aws/secretsmanager

These tests run over real HTTP against whichever backend is active:
- Python (FastAPI/uvicorn) — default, started in-process by conftest.py
- Rust  (cloudtwin-lite)  — set CLOUDTWIN_TEST_URL=http://localhost:4793

The test expectations are identical for both backends.
"""

from __future__ import annotations

import httpx
import pytest


@pytest.fixture(scope="module")
def secretsmanager_resp(dashboard_url):
    """Fetch /api/dashboard/aws/secretsmanager once and share across all tests."""
    return httpx.get(f"{dashboard_url}/api/dashboard/aws/secretsmanager", timeout=5.0)


def test_secretsmanager_status_ok(secretsmanager_resp):
    assert secretsmanager_resp.status_code == 200


def test_secretsmanager_has_secrets_key(secretsmanager_resp):
    assert "secrets" in secretsmanager_resp.json()


def test_secretsmanager_secrets_is_list(secretsmanager_resp):
    assert isinstance(secretsmanager_resp.json()["secrets"], list)

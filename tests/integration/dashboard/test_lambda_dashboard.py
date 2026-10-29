"""
Dashboard endpoint tests: GET /api/dashboard/aws/lambda

These tests run over real HTTP against the CloudTwin binary started by
tests/conftest.py (or the server at CLOUDTWIN_TEST_URL).
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

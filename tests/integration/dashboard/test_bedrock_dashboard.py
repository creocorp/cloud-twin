"""
Dashboard endpoint tests: GET /api/dashboard/aws/bedrock

These tests run over real HTTP against whichever backend is active:
- Python (FastAPI/uvicorn) — default, started in-process by conftest.py
- Rust  (cloudtwin-lite)  — set CLOUDTWIN_TEST_URL=http://localhost:4793

The test expectations are identical for both backends.
"""

from __future__ import annotations

import httpx
import pytest


@pytest.fixture(scope="module")
def bedrock_resp(dashboard_url):
    """Fetch /api/dashboard/aws/bedrock once and share across all tests."""
    r = httpx.get(f"{dashboard_url}/api/dashboard/aws/bedrock", timeout=5.0)
    return r


def test_bedrock_status_ok(bedrock_resp):
    assert bedrock_resp.status_code == 200


def test_bedrock_has_models_key(bedrock_resp):
    assert "models" in bedrock_resp.json()


def test_bedrock_returns_foundation_models(bedrock_resp):
    """Both backends expose at least 5 foundation models."""
    models = bedrock_resp.json()["models"]
    assert len(models) >= 5, f"expected at least 5 models, got {len(models)}"


def test_bedrock_model_ids_are_unique(bedrock_resp):
    """Each model_id must appear exactly once."""
    ids = [m["model_id"] for m in bedrock_resp.json()["models"]]
    assert len(ids) == len(set(ids)), f"duplicate model_ids: {ids}"


def test_bedrock_model_shape(bedrock_resp):
    """Each model entry must carry the four required fields with correct types."""
    for m in bedrock_resp.json()["models"]:
        assert isinstance(m.get("model_id"), str),      f"model_id wrong: {m}"
        assert isinstance(m.get("model_name"), str),    f"model_name wrong: {m}"
        assert isinstance(m.get("provider"), str),      f"provider wrong: {m}"
        assert isinstance(m.get("request_count"), int), f"request_count wrong: {m}"


def test_bedrock_request_count_non_negative(bedrock_resp):
    for m in bedrock_resp.json()["models"]:
        assert m["request_count"] >= 0, (
            f"negative request_count for {m['model_id']}: {m['request_count']}"
        )

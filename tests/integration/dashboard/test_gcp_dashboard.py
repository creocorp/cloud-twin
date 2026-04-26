"""
Dashboard endpoint tests: GCP services

  GET /api/dashboard/gcp/cloudfunctions
  GET /api/dashboard/gcp/cloudtasks
  GET /api/dashboard/gcp/firestore
  GET /api/dashboard/gcp/secretmanager

These tests run over real HTTP against whichever backend is active:
- Python (FastAPI/uvicorn) — default, started in-process by conftest.py
- Rust  (cloudtwin-lite)  — set CLOUDTWIN_TEST_URL=http://localhost:4793

The test expectations are identical for both backends.
"""

from __future__ import annotations

import httpx
import pytest


# ── Cloud Functions ───────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def cloudfunctions_resp(dashboard_url):
    return httpx.get(f"{dashboard_url}/api/dashboard/gcp/cloudfunctions", timeout=5.0)


def test_cloudfunctions_status_ok(cloudfunctions_resp):
    assert cloudfunctions_resp.status_code == 200


def test_cloudfunctions_has_functions_key(cloudfunctions_resp):
    assert "functions" in cloudfunctions_resp.json()


def test_cloudfunctions_is_list(cloudfunctions_resp):
    assert isinstance(cloudfunctions_resp.json()["functions"], list)


# ── Cloud Tasks ───────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def cloudtasks_resp(dashboard_url):
    return httpx.get(f"{dashboard_url}/api/dashboard/gcp/cloudtasks", timeout=5.0)


def test_cloudtasks_status_ok(cloudtasks_resp):
    assert cloudtasks_resp.status_code == 200


def test_cloudtasks_has_queues_key(cloudtasks_resp):
    assert "queues" in cloudtasks_resp.json()


def test_cloudtasks_queues_is_list(cloudtasks_resp):
    assert isinstance(cloudtasks_resp.json()["queues"], list)


# ── Firestore ─────────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def firestore_resp(dashboard_url):
    return httpx.get(f"{dashboard_url}/api/dashboard/gcp/firestore", timeout=5.0)


def test_firestore_status_ok(firestore_resp):
    assert firestore_resp.status_code == 200


def test_firestore_has_collections_key(firestore_resp):
    assert "collections" in firestore_resp.json()


def test_firestore_collections_is_list(firestore_resp):
    assert isinstance(firestore_resp.json()["collections"], list)


# ── Secret Manager ────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def secretmanager_resp(dashboard_url):
    return httpx.get(f"{dashboard_url}/api/dashboard/gcp/secretmanager", timeout=5.0)


def test_secretmanager_status_ok(secretmanager_resp):
    assert secretmanager_resp.status_code == 200


def test_secretmanager_has_secrets_key(secretmanager_resp):
    assert "secrets" in secretmanager_resp.json()


def test_secretmanager_secrets_is_list(secretmanager_resp):
    assert isinstance(secretmanager_resp.json()["secrets"], list)

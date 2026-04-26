"""
Dashboard endpoint tests: Azure services

  GET /api/dashboard/azure/eventgrid
  GET /api/dashboard/azure/functions
  GET /api/dashboard/azure/keyvault
  GET /api/dashboard/azure/queue

These tests run over real HTTP against whichever backend is active:
- Python (FastAPI/uvicorn) — default, started in-process by conftest.py
- Rust  (cloudtwin-lite)  — set CLOUDTWIN_TEST_URL=http://localhost:4793

The test expectations are identical for both backends.
"""

from __future__ import annotations

import httpx
import pytest


# ── Event Grid ────────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def eventgrid_resp(dashboard_url):
    return httpx.get(f"{dashboard_url}/api/dashboard/azure/eventgrid", timeout=5.0)


def test_eventgrid_status_ok(eventgrid_resp):
    assert eventgrid_resp.status_code == 200


def test_eventgrid_has_topics_key(eventgrid_resp):
    assert "topics" in eventgrid_resp.json()


def test_eventgrid_topics_is_list(eventgrid_resp):
    assert isinstance(eventgrid_resp.json()["topics"], list)


# ── Functions ─────────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def functions_resp(dashboard_url):
    return httpx.get(f"{dashboard_url}/api/dashboard/azure/functions", timeout=5.0)


def test_functions_status_ok(functions_resp):
    assert functions_resp.status_code == 200


def test_functions_has_functions_key(functions_resp):
    assert "functions" in functions_resp.json()


def test_functions_is_list(functions_resp):
    assert isinstance(functions_resp.json()["functions"], list)


# ── Key Vault ─────────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def keyvault_resp(dashboard_url):
    return httpx.get(f"{dashboard_url}/api/dashboard/azure/keyvault", timeout=5.0)


def test_keyvault_status_ok(keyvault_resp):
    assert keyvault_resp.status_code == 200


def test_keyvault_has_secrets_key(keyvault_resp):
    assert "secrets" in keyvault_resp.json()


def test_keyvault_secrets_is_list(keyvault_resp):
    assert isinstance(keyvault_resp.json()["secrets"], list)


# ── Queue Storage ─────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def queue_resp(dashboard_url):
    return httpx.get(f"{dashboard_url}/api/dashboard/azure/queue", timeout=5.0)


def test_queue_status_ok(queue_resp):
    assert queue_resp.status_code == 200


def test_queue_has_queues_key(queue_resp):
    assert "queues" in queue_resp.json()


def test_queue_queues_is_list(queue_resp):
    assert isinstance(queue_resp.json()["queues"], list)

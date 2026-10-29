"""
Fixtures for dashboard API integration tests (``/api/dashboard/*``).

Uses the shared CloudTwin server from ``tests/conftest.py``.
"""

from __future__ import annotations

import pytest


@pytest.fixture(scope="session")
def dashboard_url(cloudtwin_url):
    return cloudtwin_url

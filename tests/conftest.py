"""
Shared pytest fixtures for CloudTwin tests.

Uses in-memory storage mode so tests are fast and isolated –
no SQLite file is created or left behind.
"""

from __future__ import annotations

import pytest_asyncio
from httpx import ASGITransport, AsyncClient

from cloudtwin.app import create_app
from cloudtwin.config import (
    AwsConfig,
    Config,
    DashboardConfig,
    LoggingConfig,
    ProvidersConfig,
    SesConfig,
    SmtpConfig,
    StorageConfig,
)


def _test_config(**overrides) -> Config:
    """Return a minimal in-memory Config, with optional overrides."""
    ses_config = overrides.pop(
        "ses_config", SesConfig(strict_verification=False, smtp=SmtpConfig())
    )
    return Config(
        storage=StorageConfig(mode="memory"),
        providers=ProvidersConfig(
            aws=AwsConfig(services=["ses", "s3"], ses=ses_config)
        ),
        dashboard=DashboardConfig(enabled=False),
        logging=LoggingConfig(level="warning"),
        **overrides,
    )


@pytest_asyncio.fixture
async def client():
    """Async HTTP client wired to a fresh in-memory CloudTwin app."""
    app = create_app(_test_config())
    async with AsyncClient(
        transport=ASGITransport(app=app), base_url="http://test"
    ) as c:
        # Trigger lifespan manually
        async with app.router.lifespan_context(app):
            yield c


@pytest_asyncio.fixture
async def strict_client():
    """Client with strict identity verification enabled."""
    app = create_app(
        _test_config(ses_config=SesConfig(strict_verification=True, smtp=SmtpConfig()))
    )
    async with AsyncClient(
        transport=ASGITransport(app=app), base_url="http://test"
    ) as c:
        async with app.router.lifespan_context(app):
            yield c

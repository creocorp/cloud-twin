"""
Integration test fixtures for the AWS provider.

Starts a real uvicorn server on a random port with in-memory storage.
boto3 clients are pointed at that server so every test exercises the
full HTTP + XML stack instead of mocking anything.
"""

from __future__ import annotations

import socket
import threading
import time

import httpx
import pytest
import uvicorn
from botocore.config import Config as BotocoreConfig

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

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

_FAKE_CREDS = dict(
    aws_access_key_id="test",
    aws_secret_access_key="test",
    region_name="us-east-1",
)


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _wait_ready(base_url: str, attempts: int = 60, delay: float = 0.1) -> None:
    for _ in range(attempts):
        try:
            r = httpx.get(f"{base_url}/_health", timeout=1.0)
            if r.status_code == 200:
                return
        except Exception:
            pass
        time.sleep(delay)
    raise RuntimeError(f"CloudTwin never became ready at {base_url}")


# ---------------------------------------------------------------------------
# Session-scoped server  (starts once, shared by all integration tests)
# ---------------------------------------------------------------------------


@pytest.fixture(scope="session")
def server_url():
    port = _free_port()

    config = Config(
        storage=StorageConfig(mode="memory"),
        providers=ProvidersConfig(
            aws=AwsConfig(
                services=["ses", "s3", "sns", "sqs"],
                ses=SesConfig(strict_verification=False, smtp=SmtpConfig()),
            )
        ),
        dashboard=DashboardConfig(enabled=False),
        logging=LoggingConfig(level="warning"),
        api_port=port,
    )
    app = create_app(config)

    uv = uvicorn.Server(
        uvicorn.Config(app, host="127.0.0.1", port=port, log_level="warning")
    )
    t = threading.Thread(target=uv.run, daemon=True)
    t.start()

    base_url = f"http://127.0.0.1:{port}"
    _wait_ready(base_url)

    yield base_url

    uv.should_exit = True
    t.join(timeout=5)


# ---------------------------------------------------------------------------
# boto3 clients (session-scoped – same server, tests use unique bucket/domain names)
# ---------------------------------------------------------------------------


@pytest.fixture(scope="session")
def ses(server_url):
    import boto3

    return boto3.client("ses", endpoint_url=server_url, **_FAKE_CREDS)


@pytest.fixture(scope="session")
def sesv2(server_url):
    import boto3

    return boto3.client("sesv2", endpoint_url=server_url, **_FAKE_CREDS)


@pytest.fixture(scope="session")
def s3(server_url):
    import boto3

    return boto3.client(
        "s3",
        endpoint_url=server_url,
        config=BotocoreConfig(s3={"addressing_style": "path"}),
        **_FAKE_CREDS,
    )


@pytest.fixture(scope="session")
def sns(server_url):
    import boto3

    return boto3.client("sns", endpoint_url=server_url, **_FAKE_CREDS)


@pytest.fixture(scope="session")
def sqs(server_url):
    import boto3

    return boto3.client("sqs", endpoint_url=server_url, **_FAKE_CREDS)

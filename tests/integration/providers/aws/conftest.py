"""
Integration test fixtures for the AWS provider.

Starts a real uvicorn server on a random port with in-memory storage.
boto3 clients are pointed at that server so every test exercises the
full HTTP + XML stack instead of mocking anything.
"""

from __future__ import annotations

import os
import socket
import tempfile
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


def _storage_config() -> StorageConfig:
    mode = os.getenv("CLOUDTWIN_STORAGE_MODE", "memory")
    if mode == "sqlite":
        db_dir = tempfile.mkdtemp(prefix="cloudtwin_test_")
        return StorageConfig(mode="sqlite", path=f"{db_dir}/test.db")
    return StorageConfig(mode="memory")


@pytest.fixture(scope="session")
def server_url():
    # Allow running tests against an external backend (e.g. cloudtwin-lite).
    # Set CLOUDTWIN_TEST_URL=http://host:port to skip spawning the Python server.
    external_url = os.getenv("CLOUDTWIN_TEST_URL")
    if external_url:
        _wait_ready(external_url)
        yield external_url
        return

    port = _free_port()

    config = Config(
        storage=_storage_config(),
        providers=ProvidersConfig(
            aws=AwsConfig(
                services=[
                    "ses",
                    "sns",
                    "sqs",
                    "lambda",
                    "dynamodb",
                    "secretsmanager",
                    # bedrock must come before s3 to avoid route shadowing
                    "bedrock",
                    "s3",
                ],
                ses=SesConfig(strict_verification=False, smtp=SmtpConfig()),
            )
        ),
        dashboard=DashboardConfig(enabled=False),
        logging=LoggingConfig(level="warning"),
        api_port=port,
        bedrock={
            "defaults": {"mode": "text"},
            "models": {
                "test.text": {"mode": "text"},
                "test.schema": {
                    "mode": "schema",
                    "schema": {
                        "type": "object",
                        "properties": {
                            "answer": {"type": "string"},
                            "score": {"type": "number"},
                            "tags": {
                                "type": "array",
                                "items": {"type": "string"},
                            },
                        },
                    },
                },
                "test.static": {
                    "mode": "static",
                    "static": {"result": "fixed", "value": 42},
                },
                "test.sequence": {
                    "sequence": {
                        "mode": "sequence",
                        "responses": [
                            {"static": {"answer": "first"}},
                            {"static": {"answer": "second"}},
                        ],
                    }
                },
                "test.cycle": {
                    "sequence": {
                        "mode": "cycle",
                        "responses": [
                            {"static": {"answer": "a"}},
                            {"static": {"answer": "b"}},
                        ],
                    }
                },
                "test.rules": {
                    "rules": [
                        {
                            "contains": "sentiment",
                            "response": {"static": {"sentiment": "positive", "score": 0.9}},
                        },
                        {
                            "contains": "fail",
                            "error": {"type": "ThrottlingException", "message": "Rule-triggered throttle"},
                        },
                    ],
                    "mode": "text",
                },
                "test.inject": {
                    "mode": "text",
                    "errors": [{"every": 3, "type": "ThrottlingException", "message": "Every 3rd request fails"}],
                },
                "test.stream": {
                    "mode": "text",
                    "streaming": {
                        "enabled": True,
                        "chunk_mode": "word",
                        "chunk_delay_ms": 0,
                    },
                },
            },
        },
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


@pytest.fixture(scope="session")
def lambda_client(server_url):
    import boto3

    return boto3.client("lambda", endpoint_url=server_url, **_FAKE_CREDS)


@pytest.fixture(scope="session")
def dynamodb(server_url):
    import boto3

    return boto3.client("dynamodb", endpoint_url=server_url, **_FAKE_CREDS)


@pytest.fixture(scope="session")
def secretsmanager(server_url):
    import boto3

    return boto3.client("secretsmanager", endpoint_url=server_url, **_FAKE_CREDS)


@pytest.fixture(scope="session")
def bedrock_runtime(server_url):
    import boto3

    # Disable automatic botocore retries so that simulated ThrottlingException
    # errors injected by the scenario engine are not silently retried away.
    # ``total_max_attempts=1`` means: 1 attempt, no retries.
    return boto3.client(
        "bedrock-runtime",
        endpoint_url=server_url,
        config=BotocoreConfig(retries={"total_max_attempts": 1, "mode": "standard"}),
        **_FAKE_CREDS,
    )


@pytest.fixture(scope="session")
def bedrock_mgmt(server_url):
    import boto3

    return boto3.client("bedrock", endpoint_url=server_url, **_FAKE_CREDS)

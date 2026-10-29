"""
Integration test fixtures for the AWS provider.

Uses the shared CloudTwin server from ``tests/conftest.py``.
boto3 clients are pointed at that server so every test exercises the
full HTTP + XML stack instead of mocking anything.
"""

from __future__ import annotations

import pytest
from botocore.config import Config as BotocoreConfig

_FAKE_CREDS = dict(
    aws_access_key_id="test",
    aws_secret_access_key="test",
    region_name="us-east-1",
)


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

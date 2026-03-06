"""
Integration test fixtures for Azure provider.

Starts a real uvicorn server on a random port with in-memory storage.
azure-storage-blob client is pointed at the server for Blob tests;
httpx is used directly for Service Bus tests (SDK uses AMQP, not HTTP).
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

from cloudtwin.app import create_app
from cloudtwin.config import (
    AwsConfig,
    AzureBlobConfig,
    AzureConfig,
    AzureServiceBusConfig,
    Config,
    DashboardConfig,
    GcpConfig,
    LoggingConfig,
    ProvidersConfig,
    StorageConfig,
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


_ACCOUNT_NAME = "devstoreaccount1"
_ACCOUNT_KEY = "Eby8vdM02xNOcqFlJdE1SWKvW4GS0IEJSVDMuoFSSjM4="
_NAMESPACE = "cloudtwin-test"


def _storage_config() -> StorageConfig:
    mode = os.getenv("CLOUDTWIN_STORAGE_MODE", "memory")
    if mode == "sqlite":
        db_dir = tempfile.mkdtemp(prefix="cloudtwin_test_")
        return StorageConfig(mode="sqlite", path=f"{db_dir}/test.db")
    return StorageConfig(mode="memory")


@pytest.fixture(scope="session")
def azure_server_url():
    # Allow running tests against an external backend.
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
            aws=AwsConfig(services=[]),
            azure=AzureConfig(
                services=[
                    "blob",
                    "servicebus",
                    "queue",
                    "eventgrid",
                    "keyvault",
                    "functions",
                ],
                blob=AzureBlobConfig(
                    account_name=_ACCOUNT_NAME, account_key=_ACCOUNT_KEY
                ),
                servicebus=AzureServiceBusConfig(namespace=_NAMESPACE),
            ),
            gcp=GcpConfig(services=[]),
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


@pytest.fixture(scope="session")
def blob_client(azure_server_url):
    from azure.core.credentials import AzureNamedKeyCredential
    from azure.storage.blob import BlobServiceClient

    account_url = f"{azure_server_url}/{_ACCOUNT_NAME}"
    credential = AzureNamedKeyCredential(_ACCOUNT_NAME, _ACCOUNT_KEY)
    return BlobServiceClient(account_url=account_url, credential=credential)


@pytest.fixture(scope="session")
def asb_http(azure_server_url):
    """httpx client pre-configured for the Service Bus namespace."""
    return httpx.Client(base_url=azure_server_url, timeout=10.0)


@pytest.fixture(scope="session")
def azure_http(azure_server_url):
    """httpx client for all new Azure service REST endpoints."""
    return httpx.Client(base_url=azure_server_url, timeout=10.0)

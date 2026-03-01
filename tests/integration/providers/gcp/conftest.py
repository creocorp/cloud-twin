"""
Integration test fixtures for GCP provider.

Starts a real uvicorn server on a random port with in-memory storage.
google-cloud-storage and google-cloud-pubsub (REST transport) clients
are pointed at the local server.
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
from google.auth.credentials import AnonymousCredentials

from cloudtwin.app import create_app
from cloudtwin.config import (
    AwsConfig,
    AzureConfig,
    Config,
    DashboardConfig,
    GcpConfig,
    LoggingConfig,
    ProvidersConfig,
    StorageConfig,
)

_PROJECT = "cloudtwin-local"


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


def _storage_config() -> StorageConfig:
    mode = os.getenv("CLOUDTWIN_STORAGE_MODE", "memory")
    if mode == "sqlite":
        db_dir = tempfile.mkdtemp(prefix="cloudtwin_test_")
        return StorageConfig(mode="sqlite", path=f"{db_dir}/test.db")
    return StorageConfig(mode="memory")


@pytest.fixture(scope="session")
def gcp_server_url():
    port = _free_port()

    config = Config(
        storage=_storage_config(),
        providers=ProvidersConfig(
            aws=AwsConfig(services=[]),
            azure=AzureConfig(services=[]),
            gcp=GcpConfig(
                project=_PROJECT,
                services=[
                    "storage",
                    "pubsub",
                    "firestore",
                    "cloudtasks",
                    "secretmanager",
                    "cloudfunctions",
                ],
            ),
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
def gcs(gcp_server_url):
    from google.cloud import storage

    return storage.Client(
        project=_PROJECT,
        credentials=AnonymousCredentials(),
        client_options={"api_endpoint": gcp_server_url},
    )


@pytest.fixture(scope="session")
def pubsub_publisher(gcp_server_url):
    from google.pubsub_v1.services.publisher import PublisherClient
    from google.pubsub_v1.services.publisher.transports.rest import (
        PublisherRestTransport,
    )

    transport = PublisherRestTransport(
        host=gcp_server_url,
        credentials=AnonymousCredentials(),
    )
    return PublisherClient(transport=transport)


@pytest.fixture(scope="session")
def pubsub_subscriber(gcp_server_url):
    from google.pubsub_v1.services.subscriber import SubscriberClient
    from google.pubsub_v1.services.subscriber.transports.rest import (
        SubscriberRestTransport,
    )

    transport = SubscriberRestTransport(
        host=gcp_server_url,
        credentials=AnonymousCredentials(),
    )
    return SubscriberClient(transport=transport)


@pytest.fixture(scope="session")
def gcp_http(gcp_server_url):
    """httpx client for all new GCP service REST endpoints."""
    return httpx.Client(base_url=gcp_server_url, timeout=10.0)

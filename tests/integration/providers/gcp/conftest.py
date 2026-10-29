"""
Integration test fixtures for GCP provider.

Uses the shared CloudTwin server from ``tests/conftest.py``.
google-cloud-storage and google-cloud-pubsub (REST transport) clients
are pointed at the local server.
"""

from __future__ import annotations

import httpx
import pytest
from google.auth.credentials import AnonymousCredentials

_PROJECT = "cloudtwin-local"


@pytest.fixture(scope="session")
def gcp_server_url(cloudtwin_url):
    return cloudtwin_url


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

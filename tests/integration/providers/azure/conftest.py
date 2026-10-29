"""
Integration test fixtures for Azure provider.

Uses the shared CloudTwin server from ``tests/conftest.py``.
azure-storage-blob client is pointed at the server for Blob tests;
httpx is used directly for Service Bus tests (SDK uses AMQP, not HTTP).
"""

from __future__ import annotations

import httpx
import pytest

_ACCOUNT_NAME = "devstoreaccount1"

_ACCOUNT_KEY = "Eby8vdM02xNOcqFlJdE1SWKvW4GS0IEJSVDMuoFSSjM4="

_NAMESPACE = "cloudtwin-test"


@pytest.fixture(scope="session")
def azure_server_url(cloudtwin_url):
    return cloudtwin_url


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

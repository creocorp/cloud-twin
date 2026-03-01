"""Azure Blob Storage service package."""

from __future__ import annotations

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.azure.blob.handlers import make_router
from cloudtwin.providers.azure.blob.service import BlobService


def register(
    app: FastAPI, config: Config, repos: dict, telemetry: TelemetryEngine
) -> None:
    account_name = config.providers.azure.blob.account_name
    service = BlobService(
        account_name=account_name,
        container_repo=repos["azure_container"],
        blob_repo=repos["azure_blob"],
        telemetry=telemetry,
    )
    router = make_router(service)
    app.include_router(router, prefix=f"/{account_name}")

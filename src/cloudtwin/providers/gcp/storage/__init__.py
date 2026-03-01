"""GCP Cloud Storage service package."""

from __future__ import annotations

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.gcp.storage.handlers import make_router
from cloudtwin.providers.gcp.storage.service import StorageService


def register(
    app: FastAPI, config: Config, repos: dict, telemetry: TelemetryEngine
) -> None:
    service = StorageService(
        project=config.providers.gcp.project,
        bucket_repo=repos["gcs_bucket"],
        object_repo=repos["gcs_object"],
        telemetry=telemetry,
    )
    router = make_router(service)
    app.include_router(router)

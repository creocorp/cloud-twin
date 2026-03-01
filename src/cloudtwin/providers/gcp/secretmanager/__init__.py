"""GCP Secret Manager — service registration entry point."""

from __future__ import annotations

import logging

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.gcp.secretmanager.handlers import make_router
from cloudtwin.providers.gcp.secretmanager.service import GcpSecretManagerService

log = logging.getLogger("cloudtwin.gcp.secretmanager")


def register(
    app: FastAPI, config: Config, repos: dict, telemetry: TelemetryEngine, **kwargs
) -> None:
    service = GcpSecretManagerService(
        secret_repo=repos["gcp_secret"],
        version_repo=repos["gcp_secret_version"],
        telemetry=telemetry,
    )
    router = make_router(service)
    app.include_router(router)
    log.info("Registered GCP service: secretmanager")

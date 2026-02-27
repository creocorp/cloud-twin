"""GCP Cloud Functions — service registration entry point."""

from __future__ import annotations

import logging

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.gcp.cloudfunctions.handlers import make_router
from cloudtwin.providers.gcp.cloudfunctions.service import GcpCloudFunctionsService

log = logging.getLogger("cloudtwin.gcp.cloudfunctions")


def register(app: FastAPI, config: Config, repos: dict, telemetry: TelemetryEngine, **kwargs) -> None:
    service = GcpCloudFunctionsService(
        function_repo=repos["gcp_function"],
        invocation_repo=repos["gcp_function_invocation"],
        telemetry=telemetry,
    )
    router = make_router(service)
    app.include_router(router)
    log.info("Registered GCP service: cloudfunctions")

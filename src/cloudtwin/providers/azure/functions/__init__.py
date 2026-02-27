"""Azure Functions — service registration entry point."""

from __future__ import annotations

import logging

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.azure.functions.handlers import make_router
from cloudtwin.providers.azure.functions.service import AzureFunctionsService

log = logging.getLogger("cloudtwin.azure.functions")


def register(app: FastAPI, config: Config, repos: dict, telemetry: TelemetryEngine, **kwargs) -> None:
    service = AzureFunctionsService(
        function_repo=repos["azure_function"],
        invocation_repo=repos["azure_function_invocation"],
        telemetry=telemetry,
    )
    router = make_router(service)
    app.include_router(router)
    log.info("Registered Azure service: functions")

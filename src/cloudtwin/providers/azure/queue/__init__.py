"""Azure Queue Storage — service registration entry point."""

from __future__ import annotations

import logging

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.azure.queue.handlers import make_router
from cloudtwin.providers.azure.queue.service import AzureQueueService

log = logging.getLogger("cloudtwin.azure.queue")


def register(
    app: FastAPI, config: Config, repos: dict, telemetry: TelemetryEngine, **kwargs
) -> None:
    service = AzureQueueService(
        queue_repo=repos["azure_storage_queue"],
        message_repo=repos["azure_queue_message"],
        telemetry=telemetry,
    )
    router = make_router(service)
    app.include_router(router)
    log.info("Registered Azure service: queue")

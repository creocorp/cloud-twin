"""Azure Event Grid — service registration entry point."""

from __future__ import annotations

import logging

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.azure.eventgrid.handlers import make_router
from cloudtwin.providers.azure.eventgrid.service import EventGridService

log = logging.getLogger("cloudtwin.azure.eventgrid")


def register(app: FastAPI, config: Config, repos: dict, telemetry: TelemetryEngine, **kwargs) -> None:
    service = EventGridService(
        topic_repo=repos["eg_topic"],
        event_repo=repos["eg_event"],
        telemetry=telemetry,
    )
    router = make_router(service)
    app.include_router(router)
    log.info("Registered Azure service: eventgrid")

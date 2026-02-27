"""GCP Firestore — service registration entry point."""

from __future__ import annotations

import logging

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.gcp.firestore.handlers import make_router
from cloudtwin.providers.gcp.firestore.service import FirestoreService

log = logging.getLogger("cloudtwin.gcp.firestore")


def register(app: FastAPI, config: Config, repos: dict, telemetry: TelemetryEngine, **kwargs) -> None:
    service = FirestoreService(
        repo=repos["firestore_document"],
        telemetry=telemetry,
    )
    router = make_router(service)
    app.include_router(router)
    log.info("Registered GCP service: firestore")

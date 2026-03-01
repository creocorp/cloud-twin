"""Azure Key Vault — service registration entry point."""

from __future__ import annotations

import logging

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.azure.keyvault.handlers import make_router
from cloudtwin.providers.azure.keyvault.service import KeyVaultService

log = logging.getLogger("cloudtwin.azure.keyvault")


def register(
    app: FastAPI, config: Config, repos: dict, telemetry: TelemetryEngine, **kwargs
) -> None:
    service = KeyVaultService(
        repo=repos["kv_secret"],
        telemetry=telemetry,
    )
    router = make_router(service)
    app.include_router(router)
    log.info("Registered Azure service: keyvault")

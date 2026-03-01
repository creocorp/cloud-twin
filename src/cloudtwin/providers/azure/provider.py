"""
Azure Provider.

Discovers enabled Azure services and calls each service's register() function.

Service packages must expose:
    register(app, config, repos, telemetry) -> None
"""

from __future__ import annotations

import importlib
import logging

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.repositories import make_repositories

log = logging.getLogger("cloudtwin.azure")

_SERVICE_REGISTRY: dict[str, str] = {
    "blob": "cloudtwin.providers.azure.blob",
    "servicebus": "cloudtwin.providers.azure.servicebus",
    "queue": "cloudtwin.providers.azure.queue",
    "eventgrid": "cloudtwin.providers.azure.eventgrid",
    "keyvault": "cloudtwin.providers.azure.keyvault",
    "functions": "cloudtwin.providers.azure.functions",
}


class AzureProvider:
    def __init__(self, config: Config, db, repos: dict | None = None):
        self._config = config
        self._db = db
        self._repos = (
            repos
            if repos is not None
            else make_repositories(db, mode=config.storage.mode)
        )
        self._telemetry = TelemetryEngine(self._repos.get("event"))

    def register(self, app: FastAPI) -> None:
        for service_name in self._config.providers.azure.services:
            module_path = _SERVICE_REGISTRY.get(service_name)
            if module_path is None:
                log.warning("Unknown Azure service %r – skipping", service_name)
                continue
            module = importlib.import_module(module_path)
            module.register(app, self._config, self._repos, self._telemetry)
            log.info("Registered Azure service: %s", service_name)

"""
GCP Provider.

Discovers enabled GCP services and calls each service's register() function.

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

log = logging.getLogger("cloudtwin.gcp")

_SERVICE_REGISTRY: dict[str, str] = {
    "storage": "cloudtwin.providers.gcp.storage",
    "pubsub": "cloudtwin.providers.gcp.pubsub",
}


class GcpProvider:
    def __init__(self, config: Config, db):
        self._config = config
        self._db = db
        self._repos = make_repositories(db, mode=config.storage.mode)
        self._telemetry = TelemetryEngine(self._repos.get("event"))

    def register(self, app: FastAPI) -> None:
        for service_name in self._config.providers.gcp.services:
            module_path = _SERVICE_REGISTRY.get(service_name)
            if module_path is None:
                log.warning("Unknown GCP service %r – skipping", service_name)
                continue
            module = importlib.import_module(module_path)
            module.register(app, self._config, self._repos, self._telemetry)
            log.info("Registered GCP service: %s", service_name)

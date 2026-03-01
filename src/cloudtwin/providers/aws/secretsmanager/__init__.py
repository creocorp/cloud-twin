"""AWS Secrets Manager — service registration entry point."""

from __future__ import annotations

import logging

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.aws.secretsmanager.handlers import (
    register_secretsmanager_handlers,
)
from cloudtwin.providers.aws.secretsmanager.service import SecretsManagerService

log = logging.getLogger("cloudtwin.aws.secretsmanager")


def register(
    app: FastAPI,
    config: Config,
    repos: dict,
    telemetry: TelemetryEngine,
    *,
    query_router=None,
    json_router=None,
) -> None:
    service = SecretsManagerService(
        secret_repo=repos["sm_secret"],
        version_repo=repos["sm_secret_version"],
        telemetry=telemetry,
    )
    register_secretsmanager_handlers(json_router, service)
    log.info("Registered AWS service: secretsmanager")

"""AWS Lambda — service registration entry point."""

from __future__ import annotations

import logging

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.aws.lambda_.handlers import make_router
from cloudtwin.providers.aws.lambda_.service import LambdaService

log = logging.getLogger("cloudtwin.aws.lambda_")


def register(app: FastAPI, config: Config, repos: dict, telemetry: TelemetryEngine, **kwargs) -> None:
    service = LambdaService(
        function_repo=repos["lambda_function"],
        invocation_repo=repos["lambda_invocation"],
        telemetry=telemetry,
    )
    router = make_router(service)
    app.include_router(router)
    log.info("Registered AWS service: lambda")

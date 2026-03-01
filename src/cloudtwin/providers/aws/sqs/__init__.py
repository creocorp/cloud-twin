"""
SQS service package entry point.

Exposes register(app, config, repos, telemetry, *, json_router) which
registers all SQS JSON-protocol actions into the shared router.
"""

from __future__ import annotations

import logging

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.aws.sqs.handlers import register_sqs_handlers
from cloudtwin.providers.aws.sqs.service import SqsService

log = logging.getLogger("cloudtwin.sqs")


def register(
    app: FastAPI,
    config: Config,
    repos: dict,
    telemetry: TelemetryEngine,
    *,
    query_router=None,
    json_router=None,
) -> None:
    """Register SQS with the application using the shared json_router."""
    base_url = f"http://localhost:{config.api_port}"
    service = SqsService(
        base_url=base_url,
        queue_repo=repos["sqs_queue"],
        message_repo=repos["sqs_message"],
        telemetry=telemetry,
    )
    register_sqs_handlers(json_router, service)
    log.info(
        "SQS handlers registered (CreateQueue, SendMessage, ReceiveMessage, DeleteMessage)"
    )

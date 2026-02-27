"""AWS DynamoDB — service registration entry point."""

from __future__ import annotations

import logging

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.aws.dynamodb.handlers import register_dynamodb_handlers
from cloudtwin.providers.aws.dynamodb.service import DynamoDBService

log = logging.getLogger("cloudtwin.aws.dynamodb")


def register(
    app: FastAPI,
    config: Config,
    repos: dict,
    telemetry: TelemetryEngine,
    *,
    query_router=None,
    json_router=None,
) -> None:
    service = DynamoDBService(
        table_repo=repos["dynamo_table"],
        item_repo=repos["dynamo_item"],
        telemetry=telemetry,
    )
    register_dynamodb_handlers(json_router, service)
    log.info("Registered AWS service: dynamodb")

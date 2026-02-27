"""
SNS service package entry point.

Exposes register(app, config, repos, telemetry, *, query_router) which
registers all SNS Query-protocol actions into the shared router.
"""

from __future__ import annotations

import logging

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.aws.sns.handlers import register_sns_handlers
from cloudtwin.providers.aws.sns.service import SnsService

log = logging.getLogger("cloudtwin.sns")


def register(
    app: FastAPI,
    config: Config,
    repos: dict,
    telemetry: TelemetryEngine,
    *,
    query_router=None,
    json_router=None,
) -> None:
    """Register SNS with the application using the shared query_router."""
    service = SnsService(
        topic_repo=repos["sns_topic"],
        subscription_repo=repos["sns_subscription"],
        message_repo=repos["sns_message"],
        telemetry=telemetry,
    )
    register_sns_handlers(query_router, service)
    log.info("SNS handlers registered (CreateTopic, ListTopics, Subscribe, Publish)")

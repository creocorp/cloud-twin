"""GCP Pub/Sub service package."""

from __future__ import annotations

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.gcp.pubsub.handlers import make_router
from cloudtwin.providers.gcp.pubsub.service import PubsubService


def register(
    app: FastAPI, config: Config, repos: dict, telemetry: TelemetryEngine
) -> None:
    service = PubsubService(
        project=config.providers.gcp.project,
        topic_repo=repos["pubsub_topic"],
        subscription_repo=repos["pubsub_subscription"],
        message_repo=repos["pubsub_message"],
        ackable_repo=repos["pubsub_ackable"],
        telemetry=telemetry,
    )
    router = make_router(service)
    app.include_router(router)

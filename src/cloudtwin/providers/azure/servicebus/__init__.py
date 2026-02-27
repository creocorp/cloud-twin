"""Azure Service Bus service package."""

from __future__ import annotations

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.azure.servicebus.handlers import make_router
from cloudtwin.providers.azure.servicebus.service import ServiceBusService


def register(app: FastAPI, config: Config, repos: dict, telemetry: TelemetryEngine) -> None:
    namespace = config.providers.azure.servicebus.namespace
    service = ServiceBusService(
        namespace=namespace,
        queue_repo=repos["asb_queue"],
        topic_repo=repos["asb_topic"],
        subscription_repo=repos["asb_subscription"],
        message_repo=repos["asb_message"],
        telemetry=telemetry,
    )
    router = make_router(service, namespace=namespace)
    app.include_router(router)

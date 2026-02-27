"""AWS S3 service package."""

from __future__ import annotations

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.aws.s3.handlers import make_s3_router
from cloudtwin.providers.aws.s3.service import S3Service


def register(
    app: FastAPI,
    config: Config,
    repos: dict,
    telemetry: TelemetryEngine,
) -> None:
    """Mount S3 REST endpoints onto app."""
    service = S3Service(
        bucket_repo=repos["s3_bucket"],
        object_repo=repos["s3_object"],
        telemetry=telemetry,
    )
    app.include_router(make_s3_router(service), prefix="")


__all__ = ["register", "S3Service"]

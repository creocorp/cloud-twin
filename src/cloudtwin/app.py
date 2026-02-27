"""
CloudTwin – ASGI application factory.
"""

from __future__ import annotations

import logging
from contextlib import asynccontextmanager

from fastapi import FastAPI
from fastapi.responses import JSONResponse

from cloudtwin.config import Config, load_config
from cloudtwin.core.errors import CloudTwinError
from cloudtwin.persistence.db import Database


def create_app(config: Config | None = None) -> FastAPI:
    if config is None:
        config = load_config()

    logging.basicConfig(level=config.logging.level.upper())
    log = logging.getLogger("cloudtwin")

    db = Database(config.storage)

    @asynccontextmanager
    async def lifespan(app: FastAPI):
        log.info("CloudTwin starting …")
        await db.connect()

        # Register Azure and GCP BEFORE AWS so that their fixed-prefix routes
        # (e.g. /devstoreaccount1/..., /storage/v1/...) take precedence over
        # S3's wildcard /{bucket}/... routes.

        from cloudtwin.providers.azure.provider import AzureProvider

        azure = AzureProvider(config, db)
        azure.register(app)
        log.info("Azure provider registered (services: %s)", config.providers.azure.services)

        from cloudtwin.providers.gcp.provider import GcpProvider

        gcp = GcpProvider(config, db)
        gcp.register(app)
        log.info("GCP provider registered (services: %s)", config.providers.gcp.services)

        from cloudtwin.providers.aws.provider import AwsProvider

        aws = AwsProvider(config, db)
        aws.register(app)
        log.info("AWS provider registered (services: %s)", config.providers.aws.services)

        log.info("CloudTwin ready on port %s", config.api_port)
        yield
        await db.disconnect()
        log.info("CloudTwin stopped.")

    app = FastAPI(title="CloudTwin", version="0.1.0", lifespan=lifespan)
    app.state.config = config
    app.state.db = db

    # -----------------------------------------------------------------------
    # Global error handler
    # -----------------------------------------------------------------------

    @app.exception_handler(CloudTwinError)
    async def cloudtwin_error_handler(request, exc: CloudTwinError):
        return JSONResponse(status_code=exc.http_status, content={"error": exc.message})

    # -----------------------------------------------------------------------
    # Health
    # -----------------------------------------------------------------------

    @app.get("/_health")
    async def health():
        return {"status": "ok", "service": "cloudtwin"}

    return app

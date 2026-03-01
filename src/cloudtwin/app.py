"""
CloudTwin – ASGI application factory.
"""

from __future__ import annotations

import logging
from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI
from fastapi.responses import FileResponse, JSONResponse
from fastapi.staticfiles import StaticFiles

from cloudtwin.config import Config, load_config
from cloudtwin.core.errors import CloudTwinError
from cloudtwin.persistence.db import Database
from cloudtwin.persistence.repositories import make_repositories


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

        # Create one shared repos dict so all providers and the dashboard
        # API read from the same in-memory stores (critical in "memory" mode).
        repos = make_repositories(db, mode=config.storage.mode)
        app.state.repos = repos

        # Register Azure and GCP BEFORE AWS so that their fixed-prefix routes
        # (e.g. /devstoreaccount1/..., /storage/v1/...) take precedence over
        # S3's wildcard /{bucket}/... routes.

        from cloudtwin.providers.azure.provider import AzureProvider

        azure = AzureProvider(config, db, repos=repos)
        azure.register(app)
        log.info(
            "Azure provider registered (services: %s)", config.providers.azure.services
        )

        from cloudtwin.providers.gcp.provider import GcpProvider

        gcp = GcpProvider(config, db, repos=repos)
        gcp.register(app)
        log.info(
            "GCP provider registered (services: %s)", config.providers.gcp.services
        )

        from cloudtwin.providers.aws.provider import AwsProvider

        aws = AwsProvider(config, db, repos=repos)
        aws.register(app)
        log.info(
            "AWS provider registered (services: %s)", config.providers.aws.services
        )

        log.info("CloudTwin ready on port %s", config.api_port)
        yield
        await db.disconnect()
        log.info("CloudTwin stopped.")

    app = FastAPI(title="CloudTwin", version="0.1.0", lifespan=lifespan)
    app.state.config = config
    app.state.db = db

    # -----------------------------------------------------------------------
    # Dashboard API (always registered so the dev server proxy works too)
    # -----------------------------------------------------------------------

    from cloudtwin.api.dashboard import make_dashboard_router

    app.include_router(make_dashboard_router())

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

    # -----------------------------------------------------------------------
    # Dashboard (opt-in)
    # -----------------------------------------------------------------------

    if config.dashboard.enabled:
        _mount_dashboard(app, config)

    return app


def _mount_dashboard(app: FastAPI, config) -> None:
    """Serve the pre-built Vite dashboard as a SPA."""
    log = logging.getLogger("cloudtwin")
    dist = Path(__file__).parent.parent.parent / "dashboard" / "dist"
    if not dist.is_dir():
        log.warning(
            "Dashboard enabled but dist/ not found at %s — run: cd dashboard && npm run build",
            dist,
        )
        return

    # Serve /assets/* as static files
    app.mount(
        "/assets", StaticFiles(directory=dist / "assets"), name="dashboard-assets"
    )

    # Catch-all: serve index.html for all non-API paths (SPA routing)
    @app.get("/dashboard", include_in_schema=False)
    @app.get("/dashboard/{path:path}", include_in_schema=False)
    async def dashboard_spa(path: str = ""):
        return FileResponse(dist / "index.html")

    log.info("Dashboard enabled — open http://localhost:%s/dashboard", config.api_port)

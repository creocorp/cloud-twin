"""Dashboard API package — exports make_dashboard_router()."""

from __future__ import annotations

from fastapi import APIRouter

from cloudtwin.api.dashboard import aws, azure, gcp
from cloudtwin.api.dashboard.events import router as events_router
from cloudtwin.api.dashboard.health import router as health_router


def make_dashboard_router() -> APIRouter:
    router = APIRouter(prefix="/api/dashboard", tags=["dashboard"])
    router.include_router(health_router)
    router.include_router(events_router)
    for r in aws.routers + azure.routers + gcp.routers:
        router.include_router(r)
    return router

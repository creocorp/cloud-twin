"""Dashboard — /api/dashboard/health"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/health")
async def health(request: Request):
    config = request.app.state.config
    services: dict[str, bool] = {}
    for svc in config.providers.aws.services:
        services[f"aws/{svc}"] = True
    for svc in config.providers.azure.services:
        services[f"azure/{svc}"] = True
    for svc in config.providers.gcp.services:
        services[f"gcp/{svc}"] = True
    return {
        "status": "ok",
        "storage_mode": config.storage.mode,
        "services": services,
    }

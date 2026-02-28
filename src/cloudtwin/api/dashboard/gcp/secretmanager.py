"""Dashboard — /api/dashboard/gcp/secretmanager"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/gcp/secretmanager")
async def gcp_secretmanager(request: Request):
    config = request.app.state.config
    repos = request.app.state.repos
    project = config.providers.gcp.project
    secrets = await repos["gcp_secret"].list_by_project(project)
    return {
        "secrets": [
            {
                "name": s.name,
                "full_name": s.full_name,
                "created_at": s.created_at,
            }
            for s in secrets
        ]
    }

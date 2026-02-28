"""Dashboard — /api/dashboard/gcp/cloudfunctions"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/gcp/cloudfunctions")
async def gcp_cloudfunctions(request: Request):
    config = request.app.state.config
    repos = request.app.state.repos
    project = config.providers.gcp.project
    functions = await repos["gcp_function"].list_by_project(project)
    return {
        "functions": [
            {
                "name": f.name,
                "full_name": f.full_name,
                "runtime": f.runtime,
                "created_at": f.created_at,
            }
            for f in functions
        ]
    }

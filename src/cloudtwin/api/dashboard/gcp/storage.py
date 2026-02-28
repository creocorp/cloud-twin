"""Dashboard — /api/dashboard/gcp/storage"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/gcp/storage")
async def gcp_storage(request: Request):
    config = request.app.state.config
    repos = request.app.state.repos
    project = config.providers.gcp.project
    buckets = await repos["gcs_bucket"].list_by_project(project)
    result = []
    for b in buckets:
        objects = (
            await repos["gcs_object"].list_by_bucket(b.id) if b.id is not None else []
        )
        result.append(
            {
                "name": b.name,
                "location": b.location,
                "object_count": len(objects),
                "created_at": b.created_at,
            }
        )
    return {"buckets": result}

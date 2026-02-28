"""Dashboard — /api/dashboard/azure/blob"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/azure/blob")
async def azure_blob(request: Request):
    config = request.app.state.config
    repos = request.app.state.repos
    account = config.providers.azure.blob.account_name
    containers = await repos["container"].list_by_account(account)
    result = []
    for c in containers:
        blobs = await repos["blob"].list_by_container(c.id) if c.id is not None else []
        result.append(
            {
                "name": c.name,
                "blob_count": len(blobs),
                "created_at": c.created_at,
            }
        )
    return {"containers": result}

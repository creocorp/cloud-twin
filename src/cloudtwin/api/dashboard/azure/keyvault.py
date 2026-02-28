"""Dashboard — /api/dashboard/azure/keyvault"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/azure/keyvault")
async def azure_keyvault(request: Request):
    repos = request.app.state.repos
    secrets = await repos["kv_secret"].list_all()
    return {
        "secrets": [
            {
                "vault": s.vault,
                "name": s.name,
                "version": s.version,
                "created_at": s.created_at,
            }
            for s in secrets
        ]
    }

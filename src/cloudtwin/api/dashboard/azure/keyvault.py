"""Dashboard — /api/dashboard/azure/keyvault"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/azure/keyvault")
async def azure_keyvault(request: Request):
    db = request.app.state.db
    async with db.conn.execute(
        "SELECT vault, name, version, created_at FROM kv_secrets ORDER BY created_at DESC"
    ) as cur:
        rows = await cur.fetchall()
    return {
        "secrets": [
            {
                "vault": r["vault"],
                "name": r["name"],
                "version": r["version"],
                "created_at": r["created_at"],
            }
            for r in rows
        ]
    }

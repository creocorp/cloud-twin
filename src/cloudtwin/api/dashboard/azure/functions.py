"""Dashboard — /api/dashboard/azure/functions"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/azure/functions")
async def azure_functions(request: Request):
    db = request.app.state.db
    async with db.conn.execute(
        "SELECT app, name, created_at FROM azure_functions ORDER BY created_at DESC"
    ) as cur:
        rows = await cur.fetchall()
    return {
        "functions": [
            {
                "app": r["app"],
                "name": r["name"],
                "created_at": r["created_at"],
            }
            for r in rows
        ]
    }

"""Dashboard — /api/dashboard/gcp/firestore"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/gcp/firestore")
async def gcp_firestore(request: Request):
    config = request.app.state.config
    db = request.app.state.db
    project = config.providers.gcp.project
    async with db.conn.execute(
        "SELECT collection, COUNT(*) as cnt FROM firestore_documents "
        "WHERE project = ? GROUP BY collection ORDER BY collection",
        (project,),
    ) as cur:
        rows = await cur.fetchall()
    return {
        "collections": [
            {"name": r["collection"], "document_count": r["cnt"]} for r in rows
        ]
    }

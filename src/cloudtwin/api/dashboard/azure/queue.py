"""Dashboard — /api/dashboard/azure/queue"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/azure/queue")
async def azure_queue(request: Request):
    repos = request.app.state.repos
    queues = await repos["azure_queue"].list_all()
    result = []
    for q in queues:
        msgs = (
            await repos["azure_queue_message"].list_by_queue(q.id)
            if q.id is not None
            else []
        )
        result.append(
            {
                "account": q.account,
                "name": q.name,
                "message_count": len(msgs),
                "created_at": q.created_at,
            }
        )
    return {"queues": result}

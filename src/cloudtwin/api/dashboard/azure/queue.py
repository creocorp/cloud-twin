"""Dashboard — /api/dashboard/azure/queue"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/azure/queue")
async def azure_queue(request: Request):
    config = request.app.state.config
    repos = request.app.state.repos
    account = config.providers.azure.blob.account_name
    queues = await repos["azure_storage_queue"].list_by_account(account)
    result = []
    for q in queues:
        # Count visible messages via peek (cheap, returns up to 32)
        msgs = (
            await repos["azure_queue_message"].peek(q.id, 32)
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

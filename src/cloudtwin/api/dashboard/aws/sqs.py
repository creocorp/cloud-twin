"""Dashboard — /api/dashboard/aws/sqs"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/aws/sqs")
async def aws_sqs(request: Request):
    repos = request.app.state.repos
    queues = await repos["sqs_queue"].list_all()
    result = []
    for q in queues:
        count = await repos["sqs_message"].count_all(q.id) if q.id is not None else 0
        result.append(
            {
                "name": q.name,
                "url": q.url,
                "message_count": count,
                "created_at": q.created_at,
            }
        )
    return {"queues": result}

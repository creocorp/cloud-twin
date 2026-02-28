"""Dashboard — /api/dashboard/azure/eventgrid"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/azure/eventgrid")
async def azure_eventgrid(request: Request):
    repos = request.app.state.repos
    topics = await repos["eg_topic"].list_all()
    result = []
    for t in topics:
        events = await repos["eg_event"].list_by_topic(t.name) if t.name else []
        result.append(
            {
                "name": t.name,
                "endpoint": t.endpoint,
                "event_count": len(events),
                "created_at": t.created_at,
            }
        )
    return {"topics": result}

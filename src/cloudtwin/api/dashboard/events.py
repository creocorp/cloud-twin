"""Dashboard — /api/dashboard/events"""

from __future__ import annotations

import json

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/events")
async def events(request: Request, limit: int = 50):
    repos = request.app.state.repos
    all_events = await repos["event"].list_all()
    recent = all_events[-limit:] if len(all_events) > limit else all_events
    return {
        "events": [
            {
                "id": e.id,
                "provider": e.provider,
                "service": e.service,
                "action": e.action,
                "payload": _parse_json(e.payload),
                "created_at": e.created_at,
            }
            for e in reversed(recent)
        ]
    }


def _parse_json(value: str | None) -> object:
    if not value:
        return {}
    try:
        return json.loads(value)
    except (json.JSONDecodeError, TypeError):
        return {"raw": value}

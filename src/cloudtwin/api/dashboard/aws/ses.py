"""Dashboard — /api/dashboard/aws/ses"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/aws/ses")
async def aws_ses(request: Request):
    repos = request.app.state.repos
    identities = await repos["ses_identity"].list_all()
    messages = await repos["ses_message"].list_all()
    return {
        "identities": [
            {
                "identity": i.identity,
                "type": i.type,
                "verified": i.verified,
                "created_at": i.created_at,
            }
            for i in identities
        ],
        "messages": [
            {
                "id": m.id,
                "source": m.source,
                "destination": ", ".join(m.destinations) if m.destinations else "",
                "subject": m.subject or "",
                "created_at": m.created_at,
            }
            for m in messages
        ],
    }

"""Dashboard — /api/dashboard/gcp/pubsub"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/gcp/pubsub")
async def gcp_pubsub(request: Request):
    config = request.app.state.config
    repos = request.app.state.repos
    project = config.providers.gcp.project
    topics = await repos["pubsub_topic"].list_by_project(project)
    subscriptions = await repos["pubsub_subscription"].list_by_project(project)

    topic_out = []
    for t in topics:
        subs = await repos["pubsub_subscription"].list_by_topic(t.full_name)
        topic_out.append(
            {
                "name": t.name,
                "subscription_count": len(subs),
                "created_at": t.created_at,
            }
        )

    sub_out = []
    for s in subscriptions:
        pending = await repos["pubsub_ackable"].get_pending(s.full_name)
        sub_out.append(
            {
                "name": s.name,
                "topic": s.topic_full_name,
                "message_count": len(pending),
                "created_at": s.created_at,
            }
        )

    return {"topics": topic_out, "subscriptions": sub_out}

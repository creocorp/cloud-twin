"""Dashboard — /api/dashboard/azure/servicebus"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/azure/servicebus")
async def azure_servicebus(request: Request):
    config = request.app.state.config
    repos = request.app.state.repos
    namespace = config.providers.azure.servicebus.namespace
    queues = await repos["asb_queue"].list_by_namespace(namespace)
    topics = await repos["asb_topic"].list_by_namespace(namespace)

    queue_out = []
    for q in queues:
        msgs = (
            await repos["asb_message"].get_active(q.id, "queue", limit=10000)
            if q.id is not None
            else []
        )
        queue_out.append(
            {
                "name": q.name,
                "message_count": len(msgs),
                "created_at": q.created_at,
            }
        )

    topic_out = []
    for t in topics:
        subs = (
            await repos["asb_subscription"].list_by_topic(t.id)
            if t.id is not None
            else []
        )
        topic_out.append(
            {
                "name": t.name,
                "subscription_count": len(subs),
                "created_at": t.created_at,
            }
        )

    return {"queues": queue_out, "topics": topic_out}

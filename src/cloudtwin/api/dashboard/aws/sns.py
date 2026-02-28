"""Dashboard — /api/dashboard/aws/sns"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/aws/sns")
async def aws_sns(request: Request):
    repos = request.app.state.repos
    topics = await repos["sns_topic"].list_all()
    subscriptions = await repos["sns_subscription"].list_all()
    return {
        "topics": [
            {
                "arn": t.arn,
                "name": t.name,
                "created_at": t.created_at,
            }
            for t in topics
        ],
        "subscriptions": [
            {
                "arn": s.subscription_arn,
                "topic_arn": s.topic_arn,
                "protocol": s.protocol,
                "endpoint": s.endpoint,
            }
            for s in subscriptions
        ],
    }

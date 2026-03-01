"""
GCP Pub/Sub HTTP handlers – Pub/Sub REST API v1.

Routes match what PublisherRestTransport and SubscriberRestTransport send:

  Topics:
    PUT    /v1/projects/{project}/topics/{topic}     → create topic
    GET    /v1/projects/{project}/topics/{topic}     → get topic
    GET    /v1/projects/{project}/topics             → list topics
    DELETE /v1/projects/{project}/topics/{topic}     → delete topic
    POST   /v1/projects/{project}/topics/{topic}:publish  → publish

  Subscriptions:
    PUT    /v1/projects/{project}/subscriptions/{sub}     → create subscription
    GET    /v1/projects/{project}/subscriptions/{sub}     → get subscription
    GET    /v1/projects/{project}/subscriptions           → list subscriptions
    DELETE /v1/projects/{project}/subscriptions/{sub}     → delete subscription
    POST   /v1/projects/{project}/subscriptions/{sub}:pull          → pull
    POST   /v1/projects/{project}/subscriptions/{sub}:acknowledge   → acknowledge
"""

from __future__ import annotations

import logging

from fastapi import APIRouter, Request, Response
from fastapi.responses import JSONResponse

from cloudtwin.core.errors import NotFoundError
from cloudtwin.providers.gcp.pubsub.service import PubsubService

log = logging.getLogger("cloudtwin.gcp.pubsub")


def _topic_json(t) -> dict:
    return {"name": t.full_name}


def _sub_json(s) -> dict:
    return {
        "name": s.full_name,
        "topic": s.topic_full_name,
        "ackDeadlineSeconds": s.ack_deadline_seconds,
        "messageRetentionDuration": "604800s",
    }


def make_router(service: PubsubService) -> APIRouter:
    router = APIRouter()

    # ------------------------------------------------------------------
    # Topics
    # ------------------------------------------------------------------

    @router.put("/v1/projects/{project}/topics/{topic}")
    async def create_topic(project: str, topic: str) -> JSONResponse:
        full_name = f"projects/{project}/topics/{topic}"
        t = await service.create_topic(full_name)
        return JSONResponse(_topic_json(t), status_code=200)

    @router.get("/v1/projects/{project}/topics/{topic}")
    async def get_topic(project: str, topic: str) -> JSONResponse:
        full_name = f"projects/{project}/topics/{topic}"
        try:
            t = await service.get_topic(full_name)
        except NotFoundError:
            return JSONResponse(
                {"error": {"code": 404, "message": f"Topic {full_name!r} not found"}},
                status_code=404,
            )
        return JSONResponse(_topic_json(t))

    @router.get("/v1/projects/{project}/topics")
    async def list_topics(project: str) -> JSONResponse:
        topics = await service.list_topics(project=project)
        return JSONResponse({"topics": [_topic_json(t) for t in topics]})

    @router.delete("/v1/projects/{project}/topics/{topic}")
    async def delete_topic(project: str, topic: str) -> Response:
        full_name = f"projects/{project}/topics/{topic}"
        try:
            await service.delete_topic(full_name)
        except NotFoundError:
            return JSONResponse(
                {"error": {"code": 404, "message": f"Topic {full_name!r} not found"}},
                status_code=404,
            )
        return JSONResponse({})

    @router.post("/v1/projects/{project}/topics/{topic}:publish")
    async def publish(project: str, topic: str, request: Request) -> JSONResponse:
        full_name = f"projects/{project}/topics/{topic}"
        body = await request.json()
        messages = body.get("messages", [])
        try:
            message_ids = await service.publish(full_name, messages)
        except NotFoundError:
            return JSONResponse(
                {"error": {"code": 404, "message": f"Topic {full_name!r} not found"}},
                status_code=404,
            )
        return JSONResponse({"messageIds": message_ids})

    # ------------------------------------------------------------------
    # Subscriptions
    # ------------------------------------------------------------------

    @router.put("/v1/projects/{project}/subscriptions/{sub}")
    async def create_subscription(
        project: str, sub: str, request: Request
    ) -> JSONResponse:
        full_name = f"projects/{project}/subscriptions/{sub}"
        body = await request.json()
        topic_full_name = body.get("topic", "")
        ack_deadline = body.get("ackDeadlineSeconds", 10)
        try:
            s = await service.create_subscription(
                full_name, topic_full_name, ack_deadline_seconds=ack_deadline
            )
        except NotFoundError as exc:
            return JSONResponse(
                {"error": {"code": 404, "message": exc.message}}, status_code=404
            )
        return JSONResponse(_sub_json(s), status_code=200)

    @router.get("/v1/projects/{project}/subscriptions/{sub}")
    async def get_subscription(project: str, sub: str) -> JSONResponse:
        full_name = f"projects/{project}/subscriptions/{sub}"
        try:
            s = await service.get_subscription(full_name)
        except NotFoundError:
            return JSONResponse(
                {
                    "error": {
                        "code": 404,
                        "message": f"Subscription {full_name!r} not found",
                    }
                },
                status_code=404,
            )
        return JSONResponse(_sub_json(s))

    @router.get("/v1/projects/{project}/subscriptions")
    async def list_subscriptions(project: str) -> JSONResponse:
        subs = await service.list_subscriptions(project=project)
        return JSONResponse({"subscriptions": [_sub_json(s) for s in subs]})

    @router.delete("/v1/projects/{project}/subscriptions/{sub}")
    async def delete_subscription(project: str, sub: str) -> Response:
        full_name = f"projects/{project}/subscriptions/{sub}"
        try:
            await service.delete_subscription(full_name)
        except NotFoundError:
            return JSONResponse(
                {
                    "error": {
                        "code": 404,
                        "message": f"Subscription {full_name!r} not found",
                    }
                },
                status_code=404,
            )
        return JSONResponse({})

    @router.post("/v1/projects/{project}/subscriptions/{sub}:pull")
    async def pull(project: str, sub: str, request: Request) -> JSONResponse:
        full_name = f"projects/{project}/subscriptions/{sub}"
        body = await request.json()
        max_messages = body.get("maxMessages", 10)
        try:
            received = await service.pull(full_name, max_messages=max_messages)
        except NotFoundError:
            return JSONResponse(
                {
                    "error": {
                        "code": 404,
                        "message": f"Subscription {full_name!r} not found",
                    }
                },
                status_code=404,
            )
        return JSONResponse({"receivedMessages": received})

    @router.post("/v1/projects/{project}/subscriptions/{sub}:acknowledge")
    async def acknowledge(project: str, sub: str, request: Request) -> Response:
        full_name = f"projects/{project}/subscriptions/{sub}"
        body = await request.json()
        ack_ids = body.get("ackIds", [])
        await service.acknowledge(full_name, ack_ids)
        return JSONResponse({})

    return router

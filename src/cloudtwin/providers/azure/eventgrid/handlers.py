"""Azure Event Grid — HTTP handlers (REST/JSON)."""

from __future__ import annotations

import logging

from fastapi import APIRouter, Request
from fastapi.responses import JSONResponse, Response

from cloudtwin.core.errors import CloudTwinError, NotFoundError
from cloudtwin.providers.azure.eventgrid.service import EventGridService

log = logging.getLogger("cloudtwin.azure.eventgrid")


def make_router(service: EventGridService) -> APIRouter:
    router = APIRouter()

    @router.put("/azure/eventgrid/topics/{topic_name}")
    async def create_topic(topic_name: str, request: Request) -> JSONResponse:
        try:
            body = await request.json()
        except Exception:
            body = {}
        try:
            topic = await service.create_topic(
                topic_name, endpoint=body.get("endpoint", "")
            )
            return JSONResponse(
                {"name": topic.name, "endpoint": topic.endpoint}, status_code=201
            )
        except CloudTwinError as exc:
            return JSONResponse({"error": exc.message}, status_code=exc.http_status)

    @router.delete("/azure/eventgrid/topics/{topic_name}")
    async def delete_topic(topic_name: str) -> Response:
        try:
            await service.delete_topic(topic_name)
            return Response(status_code=204)
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    @router.get("/azure/eventgrid/topics")
    async def list_topics() -> JSONResponse:
        topics = await service.list_topics()
        return JSONResponse(
            {"value": [{"name": t.name, "endpoint": t.endpoint} for t in topics]}
        )

    @router.post("/azure/eventgrid/topics/{topic_name}/events")
    async def publish_events(topic_name: str, request: Request) -> JSONResponse:
        try:
            body = await request.json()
        except Exception:
            body = []
        events = body if isinstance(body, list) else [body]
        try:
            count = await service.publish_events(topic_name, events)
            return JSONResponse({"published": count})
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    @router.get("/azure/eventgrid/topics/{topic_name}/events")
    async def list_events(topic_name: str) -> JSONResponse:
        try:
            events = await service.list_events(topic_name)
            return JSONResponse(
                {
                    "value": [
                        {
                            "id": e.event_id,
                            "eventType": e.event_type,
                            "subject": e.subject,
                            "data": e.data,
                        }
                        for e in events
                    ]
                }
            )
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    return router

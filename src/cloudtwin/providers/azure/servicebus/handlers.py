"""
Azure Service Bus HTTP handlers – JSON REST API.

Since the azure-servicebus SDK uses AMQP (not HTTP) for message operations,
this implements a custom JSON REST API that can be tested directly with httpx:

  Queues:
    PUT    /{ns}/queues/{queue}                        → create queue
    GET    /{ns}/queues                                → list queues
    GET    /{ns}/queues/{queue}                        → get queue
    DELETE /{ns}/queues/{queue}                        → delete queue
    POST   /{ns}/queues/{queue}/messages               → send message
    GET    /{ns}/queues/{queue}/messages               → receive messages (?limit=)
    DELETE /{ns}/queues/{queue}/messages/{lock_token}  → complete/delete message
    POST   /{ns}/queues/{queue}/messages/{lock_token}/abandon     → abandon
    POST   /{ns}/queues/{queue}/messages/{lock_token}/deadletter  → dead-letter

  Topics + Subscriptions:
    PUT    /{ns}/topics/{topic}                                  → create topic
    GET    /{ns}/topics                                          → list topics
    DELETE /{ns}/topics/{topic}                                  → delete topic
    PUT    /{ns}/topics/{topic}/subscriptions/{sub}             → create subscription
    GET    /{ns}/topics/{topic}/subscriptions                   → list subscriptions
    POST   /{ns}/topics/{topic}/messages                        → publish to topic
    GET    /{ns}/topics/{topic}/subscriptions/{sub}/messages    → receive from subscription
"""

from __future__ import annotations

import dataclasses
import logging
from typing import Optional

from fastapi import APIRouter, Query, Request
from fastapi.responses import JSONResponse

from cloudtwin.core.errors import NotFoundError
from cloudtwin.providers.azure.servicebus.service import ServiceBusService

log = logging.getLogger("cloudtwin.azure.servicebus")


def _msg_dict(msg) -> dict:
    return {
        "message_id": msg.message_id,
        "lock_token": msg.lock_token,
        "body": msg.body,
        "content_type": msg.content_type,
        "state": msg.state,
        "delivery_count": msg.delivery_count,
        "created_at": msg.created_at,
    }


def make_router(service: ServiceBusService, namespace: str) -> APIRouter:
    router = APIRouter()

    # ------------------------------------------------------------------
    # Queues
    # ------------------------------------------------------------------

    @router.put(f"/{namespace}/queues/{{queue_name}}")
    async def create_queue(queue_name: str) -> JSONResponse:
        queue = await service.create_queue(queue_name)
        return JSONResponse(
            {
                "name": queue.name,
                "namespace": queue.namespace,
                "created_at": queue.created_at,
            },
            status_code=201,
        )

    @router.get(f"/{namespace}/queues")
    async def list_queues() -> JSONResponse:
        queues = await service.list_queues()
        return JSONResponse(
            {"queues": [{"name": q.name, "created_at": q.created_at} for q in queues]}
        )

    @router.get(f"/{namespace}/queues/{{queue_name}}")
    async def get_queue(queue_name: str) -> JSONResponse:
        try:
            queue = await service.get_queue(queue_name)
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)
        return JSONResponse(
            {
                "name": queue.name,
                "namespace": queue.namespace,
                "created_at": queue.created_at,
            }
        )

    @router.delete(f"/{namespace}/queues/{{queue_name}}")
    async def delete_queue(queue_name: str) -> JSONResponse:
        try:
            await service.delete_queue(queue_name)
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)
        return JSONResponse({}, status_code=204)

    @router.post(f"/{namespace}/queues/{{queue_name}}/messages")
    async def send_queue_message(queue_name: str, request: Request) -> JSONResponse:
        body_bytes = await request.body()
        content_type = request.headers.get("content-type", "text/plain")
        try:
            msg = await service.send_to_queue(
                queue_name, body_bytes.decode(), content_type
            )
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)
        return JSONResponse(_msg_dict(msg), status_code=201)

    @router.get(f"/{namespace}/queues/{{queue_name}}/messages")
    async def receive_queue_messages(
        queue_name: str, limit: int = Query(default=1)
    ) -> JSONResponse:
        try:
            messages = await service.receive_from_queue(queue_name, limit=limit)
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)
        return JSONResponse({"messages": [_msg_dict(m) for m in messages]})

    @router.delete(f"/{namespace}/queues/{{queue_name}}/messages/{{lock_token}}")
    async def complete_queue_message(queue_name: str, lock_token: str) -> JSONResponse:
        try:
            await service.complete_message(lock_token)
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)
        return JSONResponse({}, status_code=204)

    @router.post(f"/{namespace}/queues/{{queue_name}}/messages/{{lock_token}}/abandon")
    async def abandon_queue_message(queue_name: str, lock_token: str) -> JSONResponse:
        try:
            await service.abandon_message(lock_token)
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)
        return JSONResponse({})

    @router.post(
        f"/{namespace}/queues/{{queue_name}}/messages/{{lock_token}}/deadletter"
    )
    async def deadletter_queue_message(
        queue_name: str, lock_token: str
    ) -> JSONResponse:
        try:
            await service.deadletter_message(lock_token)
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)
        return JSONResponse({})

    # ------------------------------------------------------------------
    # Topics
    # ------------------------------------------------------------------

    @router.put(f"/{namespace}/topics/{{topic_name}}")
    async def create_topic(topic_name: str) -> JSONResponse:
        topic = await service.create_topic(topic_name)
        return JSONResponse(
            {
                "name": topic.name,
                "namespace": topic.namespace,
                "created_at": topic.created_at,
            },
            status_code=201,
        )

    @router.get(f"/{namespace}/topics")
    async def list_topics() -> JSONResponse:
        topics = await service.list_topics()
        return JSONResponse(
            {"topics": [{"name": t.name, "created_at": t.created_at} for t in topics]}
        )

    @router.delete(f"/{namespace}/topics/{{topic_name}}")
    async def delete_topic(topic_name: str) -> JSONResponse:
        try:
            await service.delete_topic(topic_name)
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)
        return JSONResponse({}, status_code=204)

    # ------------------------------------------------------------------
    # Subscriptions
    # ------------------------------------------------------------------

    @router.put(f"/{namespace}/topics/{{topic_name}}/subscriptions/{{sub_name}}")
    async def create_subscription(topic_name: str, sub_name: str) -> JSONResponse:
        try:
            sub = await service.create_subscription(topic_name, sub_name)
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)
        return JSONResponse(
            {"name": sub.name, "topic_id": sub.topic_id, "created_at": sub.created_at},
            status_code=201,
        )

    @router.get(f"/{namespace}/topics/{{topic_name}}/subscriptions")
    async def list_subscriptions(topic_name: str) -> JSONResponse:
        try:
            subs = await service.list_subscriptions(topic_name)
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)
        return JSONResponse(
            {
                "subscriptions": [
                    {"name": s.name, "created_at": s.created_at} for s in subs
                ]
            }
        )

    @router.post(f"/{namespace}/topics/{{topic_name}}/messages")
    async def publish_to_topic(topic_name: str, request: Request) -> JSONResponse:
        body_bytes = await request.body()
        content_type = request.headers.get("content-type", "text/plain")
        try:
            msgs = await service.send_to_topic(
                topic_name, body_bytes.decode(), content_type
            )
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)
        return JSONResponse({"fan_out": len(msgs)}, status_code=201)

    @router.get(
        f"/{namespace}/topics/{{topic_name}}/subscriptions/{{sub_name}}/messages"
    )
    async def receive_from_subscription(
        topic_name: str, sub_name: str, limit: int = Query(default=1)
    ) -> JSONResponse:
        try:
            messages = await service.receive_from_subscription(
                topic_name, sub_name, limit=limit
            )
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)
        return JSONResponse({"messages": [_msg_dict(m) for m in messages]})

    @router.delete(
        f"/{namespace}/topics/{{topic_name}}/subscriptions/{{sub_name}}/messages/{{lock_token}}"
    )
    async def complete_sub_message(
        topic_name: str, sub_name: str, lock_token: str
    ) -> JSONResponse:
        try:
            await service.complete_message(lock_token)
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)
        return JSONResponse({}, status_code=204)

    return router

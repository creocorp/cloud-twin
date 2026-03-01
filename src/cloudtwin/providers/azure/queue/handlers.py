"""Azure Queue Storage — HTTP handlers (REST)."""

from __future__ import annotations

import logging

from fastapi import APIRouter, Request
from fastapi.responses import JSONResponse, Response

from cloudtwin.core.errors import CloudTwinError, NotFoundError
from cloudtwin.providers.azure.queue.service import AzureQueueService

log = logging.getLogger("cloudtwin.azure.queue")


def make_router(service: AzureQueueService) -> APIRouter:
    router = APIRouter()

    # Queue lifecycle
    @router.put("/azure/queue/{account}/{queue_name}")
    async def create_queue(account: str, queue_name: str) -> Response:
        try:
            await service.create_queue(account, queue_name)
            return Response(status_code=201)
        except CloudTwinError as exc:
            return JSONResponse({"message": exc.message}, status_code=exc.http_status)

    @router.delete("/azure/queue/{account}/{queue_name}")
    async def delete_queue(account: str, queue_name: str) -> Response:
        try:
            await service.delete_queue(account, queue_name)
            return Response(status_code=204)
        except NotFoundError as exc:
            return JSONResponse({"message": exc.message}, status_code=404)

    @router.get("/azure/queue/{account}")
    async def list_queues(account: str) -> JSONResponse:
        queues = await service.list_queues(account)
        return JSONResponse({"QueueItems": [{"Name": q.name} for q in queues]})

    # Messages
    @router.post("/azure/queue/{account}/{queue_name}/messages")
    async def send_message(
        account: str, queue_name: str, request: Request
    ) -> JSONResponse:
        try:
            body = await request.body()
            text = body.decode()
        except Exception:
            text = ""
        try:
            msg = await service.send_message(account, queue_name, text)
            return JSONResponse(
                {"MessageId": msg.message_id, "PopReceipt": msg.pop_receipt},
                status_code=201,
            )
        except NotFoundError as exc:
            return JSONResponse({"message": exc.message}, status_code=404)

    @router.get("/azure/queue/{account}/{queue_name}/messages")
    async def receive_messages(
        account: str, queue_name: str, numMessages: int = 1
    ) -> JSONResponse:
        try:
            msgs = await service.receive_messages(account, queue_name, numMessages)
            return JSONResponse(
                {
                    "QueueMessagesList": [
                        {
                            "MessageId": m.message_id,
                            "PopReceipt": m.pop_receipt,
                            "MessageText": m.body,
                        }
                        for m in msgs
                    ]
                }
            )
        except NotFoundError as exc:
            return JSONResponse({"message": exc.message}, status_code=404)

    @router.get("/azure/queue/{account}/{queue_name}/messages/peek")
    async def peek_messages(
        account: str, queue_name: str, numMessages: int = 1
    ) -> JSONResponse:
        try:
            msgs = await service.peek_messages(account, queue_name, numMessages)
            return JSONResponse(
                {
                    "QueueMessagesList": [
                        {"MessageId": m.message_id, "MessageText": m.body} for m in msgs
                    ]
                }
            )
        except NotFoundError as exc:
            return JSONResponse({"message": exc.message}, status_code=404)

    @router.delete("/azure/queue/{account}/{queue_name}/messages/{message_id}")
    async def delete_message(
        account: str, queue_name: str, message_id: str, popreceipt: str = ""
    ) -> Response:
        try:
            await service.delete_message(account, queue_name, popreceipt or message_id)
            return Response(status_code=204)
        except CloudTwinError as exc:
            return JSONResponse({"message": exc.message}, status_code=exc.http_status)

    return router

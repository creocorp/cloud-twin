"""
SQS HTTP handlers (AWS JSON / application/x-amz-json-1.0 protocol).

All SQS operations arrive as POST / with:
  Content-Type: application/x-amz-json-1.0
  X-Amz-Target: AmazonSQS.{OperationName}
"""

from __future__ import annotations

from fastapi import Request, Response
from fastapi.responses import JSONResponse

from cloudtwin.core.errors import CloudTwinError, NotFoundError
from cloudtwin.providers.aws.protocols.json_protocol import JsonProtocolRouter
from cloudtwin.providers.aws.sqs.service import SqsService


def _error(code: str, message: str, status: int = 400) -> JSONResponse:
    return JSONResponse({"__type": code, "message": message}, status_code=status)


def register_sqs_handlers(router: JsonProtocolRouter, service: SqsService) -> None:
    """Register all SQS JSON-protocol action handlers into the shared router."""

    async def create_queue(request: Request, body: dict) -> Response:
        name = body.get("QueueName", "")
        if not name:
            return _error("InvalidParameterValue", "QueueName is required")
        try:
            url = await service.create_queue(name)
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({"QueueUrl": url})

    async def list_queues(request: Request, body: dict) -> Response:
        prefix = body.get("QueueNamePrefix", "")
        try:
            urls = await service.list_queues(prefix=prefix)
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({"QueueUrls": urls})

    async def get_queue_url(request: Request, body: dict) -> Response:
        name = body.get("QueueName", "")
        if not name:
            return _error("InvalidParameterValue", "QueueName is required")
        try:
            url = await service.get_queue_url(name)
        except NotFoundError:
            return _error(
                "AWS.SimpleQueueService.NonExistentQueue",
                f"The specified queue {name!r} does not exist.",
                404,
            )
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({"QueueUrl": url})

    async def send_message(request: Request, body: dict) -> Response:
        queue_url = body.get("QueueUrl", "")
        message_body = body.get("MessageBody", "")
        if not queue_url or not message_body:
            return _error("InvalidParameterValue", "QueueUrl and MessageBody are required")
        try:
            result = await service.send_message(queue_url, message_body)
        except NotFoundError:
            return _error(
                "AWS.SimpleQueueService.NonExistentQueue",
                "The specified queue does not exist.",
                404,
            )
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse(result)

    async def receive_message(request: Request, body: dict) -> Response:
        queue_url = body.get("QueueUrl", "")
        max_number = int(body.get("MaxNumberOfMessages", 1))
        if not queue_url:
            return _error("InvalidParameterValue", "QueueUrl is required")
        try:
            messages = await service.receive_messages(queue_url, max_number)
        except NotFoundError:
            return _error(
                "AWS.SimpleQueueService.NonExistentQueue",
                "The specified queue does not exist.",
                404,
            )
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({"Messages": messages})

    async def delete_message(request: Request, body: dict) -> Response:
        queue_url = body.get("QueueUrl", "")
        receipt_handle = body.get("ReceiptHandle", "")
        if not queue_url or not receipt_handle:
            return _error("InvalidParameterValue", "QueueUrl and ReceiptHandle are required")
        try:
            await service.delete_message(queue_url, receipt_handle)
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({})

    async def delete_queue(request: Request, body: dict) -> Response:
        queue_url = body.get("QueueUrl", "")
        if not queue_url:
            return _error("InvalidParameterValue", "QueueUrl is required")
        try:
            await service.delete_queue(queue_url)
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({})

    router.register("AmazonSQS.CreateQueue", create_queue)
    router.register("AmazonSQS.ListQueues", list_queues)
    router.register("AmazonSQS.GetQueueUrl", get_queue_url)
    router.register("AmazonSQS.SendMessage", send_message)
    router.register("AmazonSQS.ReceiveMessage", receive_message)
    router.register("AmazonSQS.DeleteMessage", delete_message)
    router.register("AmazonSQS.DeleteQueue", delete_queue)

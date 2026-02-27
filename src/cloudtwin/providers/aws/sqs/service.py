"""
SQS domain service.

Business logic for SQS operations. Has no knowledge of HTTP or JSON.
Depends on repository interfaces only.
"""

from __future__ import annotations

import hashlib
import uuid
from datetime import datetime, timezone

from cloudtwin.core.errors import NotFoundError, ValidationError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models import SqsMessage, SqsQueue
from cloudtwin.persistence.repositories import SqsMessageRepository, SqsQueueRepository

_ACCOUNT_ID = "000000000000"


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _md5(text: str) -> str:
    return hashlib.md5(text.encode()).hexdigest()  # noqa: S324


class SqsService:
    def __init__(
        self,
        base_url: str,
        queue_repo: SqsQueueRepository,
        message_repo: SqsMessageRepository,
        telemetry: TelemetryEngine,
    ):
        self._base_url = base_url.rstrip("/")
        self._queue_repo = queue_repo
        self._message_repo = message_repo
        self._telemetry = telemetry

    def _queue_url(self, name: str) -> str:
        return f"{self._base_url}/{_ACCOUNT_ID}/{name}"

    def _name_from_url(self, url: str) -> str:
        """Extract queue name from a queue URL."""
        return url.rstrip("/").rsplit("/", 1)[-1]

    # -------------------------------------------------------------------
    # Queue management
    # -------------------------------------------------------------------

    async def create_queue(self, name: str) -> str:
        """Idempotent – returns QueueUrl, creating the queue if it doesn't exist."""
        existing = await self._queue_repo.get(name)
        if existing:
            return existing.url

        url = self._queue_url(name)
        queue = SqsQueue(id=None, name=name, url=url, created_at=_now())
        await self._queue_repo.save(queue)
        await self._telemetry.emit("aws", "sqs", "create_queue", {"name": name, "url": url})
        return url

    async def list_queues(self, prefix: str = "") -> list[str]:
        """Return all queue URLs, optionally filtered by prefix."""
        queues = await self._queue_repo.list_all()
        return [q.url for q in queues if q.name.startswith(prefix)]

    async def get_queue_url(self, name: str) -> str:
        """Return the URL for an existing queue."""
        queue = await self._queue_repo.get(name)
        if queue is None:
            raise NotFoundError(f"Queue not found: {name}")
        return queue.url

    async def delete_queue(self, queue_url: str) -> None:
        name = self._name_from_url(queue_url)
        await self._queue_repo.delete(name)
        await self._telemetry.emit("aws", "sqs", "delete_queue", {"name": name})

    # -------------------------------------------------------------------
    # Messages
    # -------------------------------------------------------------------

    async def send_message(self, queue_url: str, message_body: str) -> dict:
        """Enqueue a message. Returns MessageId and MD5OfMessageBody."""
        name = self._name_from_url(queue_url)
        queue = await self._queue_repo.get(name)
        if queue is None:
            raise NotFoundError(f"Queue not found: {name}")

        message_id = str(uuid.uuid4())
        receipt_handle = str(uuid.uuid4())
        msg = SqsMessage(
            id=None,
            message_id=message_id,
            queue_id=queue.id,
            body=message_body,
            receipt_handle=receipt_handle,
            visible=True,
            created_at=_now(),
        )
        await self._message_repo.save(msg)
        await self._telemetry.emit("aws", "sqs", "send_message", {"queue": name, "message_id": message_id})
        return {"MessageId": message_id, "MD5OfMessageBody": _md5(message_body)}

    async def receive_messages(self, queue_url: str, max_number: int = 1) -> list[dict]:
        """
        Receive up to max_number visible messages from the queue.
        Messages are marked invisible after being received.
        """
        name = self._name_from_url(queue_url)
        queue = await self._queue_repo.get(name)
        if queue is None:
            raise NotFoundError(f"Queue not found: {name}")

        max_number = min(max(1, max_number), 10)
        messages = await self._message_repo.get_visible(queue.id, limit=max_number)

        result = []
        for msg in messages:
            await self._message_repo.mark_invisible(msg.receipt_handle)
            result.append({
                "MessageId": msg.message_id,
                "ReceiptHandle": msg.receipt_handle,
                "MD5OfBody": _md5(msg.body),
                "Body": msg.body,
            })
        await self._telemetry.emit("aws", "sqs", "receive_message", {"queue": name, "count": len(result)})
        return result

    async def delete_message(self, queue_url: str, receipt_handle: str) -> None:
        """Delete a message by receipt handle. Idempotent."""
        await self._message_repo.delete(receipt_handle)
        name = self._name_from_url(queue_url)
        await self._telemetry.emit("aws", "sqs", "delete_message", {"queue": name})

    async def change_message_visibility(self, queue_url: str, receipt_handle: str, visibility_timeout: int) -> None:
        """Make a message visible again (timeout=0) or invisible. Idempotent."""
        name = self._name_from_url(queue_url)
        queue = await self._queue_repo.get(name)
        if queue is None:
            raise NotFoundError(f"Queue not found: {name}")
        if visibility_timeout == 0:
            await self._message_repo.make_visible(receipt_handle)
        else:
            await self._message_repo.mark_invisible(receipt_handle)
        await self._telemetry.emit("aws", "sqs", "change_message_visibility", {"queue": name})

    async def get_queue_attributes(self, queue_url: str) -> dict:
        """Return basic queue attributes."""
        name = self._name_from_url(queue_url)
        queue = await self._queue_repo.get(name)
        if queue is None:
            raise NotFoundError(f"Queue not found: {name}")
        total = await self._message_repo.count_all(queue.id)
        not_visible = await self._message_repo.count_not_visible(queue.id)
        return {
            "ApproximateNumberOfMessages": str(total - not_visible),
            "ApproximateNumberOfMessagesNotVisible": str(not_visible),
            "QueueArn": f"arn:aws:sqs:us-east-1:{_ACCOUNT_ID}:{name}",
            "CreatedTimestamp": queue.created_at,
        }

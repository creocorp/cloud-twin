"""Azure Queue Storage — pure business logic."""

from __future__ import annotations

import logging
import uuid
from datetime import datetime, timezone

from cloudtwin.core.errors import NotFoundError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models.azure.queue import AzureQueueMessage, AzureStorageQueue
from cloudtwin.persistence.repositories.azure.queue import (
    AzureQueueMessageRepository,
    AzureStorageQueueRepository,
)

log = logging.getLogger("cloudtwin.azure.queue")


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class AzureQueueService:
    def __init__(
        self,
        queue_repo: AzureStorageQueueRepository,
        message_repo: AzureQueueMessageRepository,
        telemetry: TelemetryEngine,
    ):
        self._queues = queue_repo
        self._messages = message_repo
        self._telemetry = telemetry

    async def create_queue(self, account: str, name: str) -> AzureStorageQueue:
        existing = await self._queues.get(account, name)
        if existing:
            return existing
        queue = AzureStorageQueue(account=account, name=name, created_at=_now())
        saved = await self._queues.save(queue)
        await self._telemetry.emit("azure", "queue", "create_queue", {"account": account, "name": name})
        return saved

    async def delete_queue(self, account: str, name: str) -> None:
        queue = await self._queues.get(account, name)
        if not queue:
            raise NotFoundError(f"Queue not found: {name}")
        await self._queues.delete(account, name)
        await self._telemetry.emit("azure", "queue", "delete_queue", {"account": account, "name": name})

    async def list_queues(self, account: str) -> list[AzureStorageQueue]:
        return await self._queues.list_by_account(account)

    async def send_message(self, account: str, name: str, body: str) -> AzureQueueMessage:
        queue = await self._queues.get(account, name)
        if not queue:
            raise NotFoundError(f"Queue not found: {name}")
        message = AzureQueueMessage(
            message_id=str(uuid.uuid4()),
            queue_id=queue.id,
            body=body,
            pop_receipt=str(uuid.uuid4()),
            visible=True,
            dequeue_count=0,
            created_at=_now(),
        )
        saved = await self._messages.save(message)
        await self._telemetry.emit("azure", "queue", "send_message", {"queue": name})
        return saved

    async def receive_messages(self, account: str, name: str, num_messages: int = 1) -> list[AzureQueueMessage]:
        queue = await self._queues.get(account, name)
        if not queue:
            raise NotFoundError(f"Queue not found: {name}")
        messages = await self._messages.get_visible(queue.id, num_messages)
        for m in messages:
            await self._messages.mark_invisible(m.pop_receipt)
        return messages

    async def peek_messages(self, account: str, name: str, num_messages: int = 1) -> list[AzureQueueMessage]:
        queue = await self._queues.get(account, name)
        if not queue:
            raise NotFoundError(f"Queue not found: {name}")
        return await self._messages.peek(queue.id, num_messages)

    async def delete_message(self, account: str, name: str, pop_receipt: str) -> None:
        await self._messages.delete(pop_receipt)
        await self._telemetry.emit("azure", "queue", "delete_message", {"queue": name})

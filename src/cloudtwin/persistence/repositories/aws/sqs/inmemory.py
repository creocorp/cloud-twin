"""SQS — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.aws.sqs import SqsMessage, SqsQueue
from cloudtwin.persistence.repositories.aws.sqs.repository import (
    SqsMessageRepository,
    SqsQueueRepository,
)


class InMemorySqsQueueRepository(SqsQueueRepository):
    def __init__(self):
        self._store: dict[str, SqsQueue] = {}
        self._next_id = 1

    async def get(self, name: str) -> Optional[SqsQueue]:
        return self._store.get(name)

    async def list_all(self) -> list[SqsQueue]:
        return list(self._store.values())

    async def save(self, queue: SqsQueue) -> SqsQueue:
        if queue.name not in self._store:
            queue.id = self._next_id
            self._next_id += 1
        self._store[queue.name] = queue
        return queue

    async def delete(self, name: str) -> None:
        self._store.pop(name, None)


class InMemorySqsMessageRepository(SqsMessageRepository):
    def __init__(self):
        self._store: dict[str, SqsMessage] = {}
        self._next_id = 1

    async def save(self, message: SqsMessage) -> SqsMessage:
        message.id = self._next_id
        self._next_id += 1
        self._store[message.receipt_handle] = message
        return message

    async def get_visible(self, queue_id: int, limit: int = 1) -> list[SqsMessage]:
        results = [
            m for m in self._store.values()
            if m.queue_id == queue_id and m.visible
        ]
        return sorted(results, key=lambda m: m.id or 0)[:limit]

    async def mark_invisible(self, receipt_handle: str) -> None:
        if receipt_handle in self._store:
            self._store[receipt_handle].visible = False

    async def make_visible(self, receipt_handle: str) -> None:
        if receipt_handle in self._store:
            self._store[receipt_handle].visible = True

    async def delete(self, receipt_handle: str) -> None:
        self._store.pop(receipt_handle, None)

    async def count_all(self, queue_id: int) -> int:
        return sum(1 for m in self._store.values() if m.queue_id == queue_id)

    async def count_not_visible(self, queue_id: int) -> int:
        return sum(1 for m in self._store.values() if m.queue_id == queue_id and not m.visible)

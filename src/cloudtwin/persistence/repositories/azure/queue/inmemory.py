"""Azure Queue Storage — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.azure.queue import (
    AzureQueueMessage,
    AzureStorageQueue,
)
from cloudtwin.persistence.repositories.azure.queue.repository import (
    AzureQueueMessageRepository,
    AzureStorageQueueRepository,
)


class InMemoryAzureStorageQueueRepository(AzureStorageQueueRepository):
    def __init__(self):
        self._store: dict[tuple[str, str], AzureStorageQueue] = {}
        self._next_id = 1

    async def get(self, account: str, name: str) -> Optional[AzureStorageQueue]:
        return self._store.get((account, name))

    async def list_by_account(self, account: str) -> list[AzureStorageQueue]:
        return [q for q in self._store.values() if q.account == account]

    async def save(self, queue: AzureStorageQueue) -> AzureStorageQueue:
        k = (queue.account, queue.name)
        if k not in self._store:
            queue.id = self._next_id
            self._next_id += 1
        self._store[k] = queue
        return queue

    async def delete(self, account: str, name: str) -> None:
        self._store.pop((account, name), None)


class InMemoryAzureQueueMessageRepository(AzureQueueMessageRepository):
    def __init__(self):
        self._store: dict[str, AzureQueueMessage] = {}
        self._next_id = 1

    async def save(self, message: AzureQueueMessage) -> AzureQueueMessage:
        if message.pop_receipt not in self._store:
            message.id = self._next_id
            self._next_id += 1
        self._store[message.pop_receipt] = message
        return message

    async def get_visible(
        self, queue_id: int, limit: int = 1
    ) -> list[AzureQueueMessage]:
        results = [
            m for m in self._store.values() if m.queue_id == queue_id and m.visible
        ]
        return sorted(results, key=lambda m: m.id or 0)[:limit]

    async def peek(self, queue_id: int, limit: int = 1) -> list[AzureQueueMessage]:
        results = [
            m for m in self._store.values() if m.queue_id == queue_id and m.visible
        ]
        return sorted(results, key=lambda m: m.id or 0)[:limit]

    async def mark_invisible(self, pop_receipt: str) -> None:
        if pop_receipt in self._store:
            self._store[pop_receipt].visible = False

    async def delete(self, pop_receipt: str) -> None:
        self._store.pop(pop_receipt, None)

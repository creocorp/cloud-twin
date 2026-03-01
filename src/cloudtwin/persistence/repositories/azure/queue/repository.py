"""Azure Queue Storage — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.azure.queue import (
    AzureQueueMessage,
    AzureStorageQueue,
)


class AzureStorageQueueRepository(ABC):
    @abstractmethod
    async def get(self, account: str, name: str) -> Optional[AzureStorageQueue]: ...
    @abstractmethod
    async def list_by_account(self, account: str) -> list[AzureStorageQueue]: ...
    @abstractmethod
    async def save(self, queue: AzureStorageQueue) -> AzureStorageQueue: ...
    @abstractmethod
    async def delete(self, account: str, name: str) -> None: ...


class AzureQueueMessageRepository(ABC):
    @abstractmethod
    async def save(self, message: AzureQueueMessage) -> AzureQueueMessage: ...
    @abstractmethod
    async def get_visible(
        self, queue_id: int, limit: int = 1
    ) -> list[AzureQueueMessage]: ...
    @abstractmethod
    async def peek(self, queue_id: int, limit: int = 1) -> list[AzureQueueMessage]: ...
    @abstractmethod
    async def mark_invisible(self, pop_receipt: str) -> None: ...
    @abstractmethod
    async def delete(self, pop_receipt: str) -> None: ...

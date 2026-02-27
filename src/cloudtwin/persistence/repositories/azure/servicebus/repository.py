"""Azure Service Bus — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.azure.servicebus import (
    AsbMessage,
    AsbQueue,
    AsbSubscription,
    AsbTopic,
)


class AsbQueueRepository(ABC):
    @abstractmethod
    async def get(self, namespace: str, name: str) -> Optional[AsbQueue]: ...
    @abstractmethod
    async def list_by_namespace(self, namespace: str) -> list[AsbQueue]: ...
    @abstractmethod
    async def save(self, queue: AsbQueue) -> AsbQueue: ...
    @abstractmethod
    async def delete(self, namespace: str, name: str) -> None: ...


class AsbTopicRepository(ABC):
    @abstractmethod
    async def get(self, namespace: str, name: str) -> Optional[AsbTopic]: ...
    @abstractmethod
    async def list_by_namespace(self, namespace: str) -> list[AsbTopic]: ...
    @abstractmethod
    async def save(self, topic: AsbTopic) -> AsbTopic: ...
    @abstractmethod
    async def delete(self, namespace: str, name: str) -> None: ...


class AsbSubscriptionRepository(ABC):
    @abstractmethod
    async def get(self, topic_id: int, name: str) -> Optional[AsbSubscription]: ...
    @abstractmethod
    async def list_by_topic(self, topic_id: int) -> list[AsbSubscription]: ...
    @abstractmethod
    async def save(self, sub: AsbSubscription) -> AsbSubscription: ...
    @abstractmethod
    async def delete(self, topic_id: int, name: str) -> None: ...


class AsbMessageRepository(ABC):
    @abstractmethod
    async def save(self, message: AsbMessage) -> AsbMessage: ...
    @abstractmethod
    async def get_by_lock_token(self, lock_token: str) -> Optional[AsbMessage]: ...
    @abstractmethod
    async def get_active(self, entity_id: int, entity_type: str, limit: int = 1) -> list[AsbMessage]: ...
    @abstractmethod
    async def update_state(self, lock_token: str, state: str) -> None: ...
    @abstractmethod
    async def delete(self, lock_token: str) -> None: ...

"""GCP Pub/Sub — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.gcp.pubsub import (
    PubsubAckable,
    PubsubMessage,
    PubsubSubscription,
    PubsubTopic,
)


class PubsubTopicRepository(ABC):
    @abstractmethod
    async def get(self, full_name: str) -> Optional[PubsubTopic]: ...
    @abstractmethod
    async def list_by_project(self, project: str) -> list[PubsubTopic]: ...
    @abstractmethod
    async def save(self, topic: PubsubTopic) -> PubsubTopic: ...
    @abstractmethod
    async def delete(self, full_name: str) -> None: ...


class PubsubSubscriptionRepository(ABC):
    @abstractmethod
    async def get(self, full_name: str) -> Optional[PubsubSubscription]: ...
    @abstractmethod
    async def list_by_topic(self, topic_full_name: str) -> list[PubsubSubscription]: ...
    @abstractmethod
    async def list_by_project(self, project: str) -> list[PubsubSubscription]: ...
    @abstractmethod
    async def save(self, sub: PubsubSubscription) -> PubsubSubscription: ...
    @abstractmethod
    async def delete(self, full_name: str) -> None: ...


class PubsubMessageRepository(ABC):
    @abstractmethod
    async def save(self, message: PubsubMessage) -> PubsubMessage: ...
    @abstractmethod
    async def list_by_topic(self, topic_full_name: str) -> list[PubsubMessage]: ...
    @abstractmethod
    async def get(self, message_id: str) -> Optional[PubsubMessage]: ...


class PubsubAckableRepository(ABC):
    @abstractmethod
    async def save(self, ackable: PubsubAckable) -> PubsubAckable: ...
    @abstractmethod
    async def get_by_ack_id(self, ack_id: str) -> Optional[PubsubAckable]: ...
    @abstractmethod
    async def get_pending(
        self, subscription_full_name: str, limit: int = 10
    ) -> list[PubsubAckable]: ...
    @abstractmethod
    async def delete(self, ack_id: str) -> None: ...

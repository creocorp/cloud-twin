"""SNS — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.aws.sns import SnsMessage, SnsSubscription, SnsTopic


class SnsTopicRepository(ABC):
    @abstractmethod
    async def get(self, arn: str) -> Optional[SnsTopic]: ...
    @abstractmethod
    async def get_by_name(self, name: str) -> Optional[SnsTopic]: ...
    @abstractmethod
    async def list_all(self) -> list[SnsTopic]: ...
    @abstractmethod
    async def save(self, topic: SnsTopic) -> SnsTopic: ...


class SnsSubscriptionRepository(ABC):
    @abstractmethod
    async def get(self, subscription_arn: str) -> Optional[SnsSubscription]: ...
    @abstractmethod
    async def list_by_topic(self, topic_arn: str) -> list[SnsSubscription]: ...
    @abstractmethod
    async def save(self, sub: SnsSubscription) -> SnsSubscription: ...


class SnsMessageRepository(ABC):
    @abstractmethod
    async def save(self, message: SnsMessage) -> SnsMessage: ...
    @abstractmethod
    async def list_by_topic(self, topic_arn: str) -> list[SnsMessage]: ...

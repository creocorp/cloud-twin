"""Azure Event Grid — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.azure.eventgrid import EventGridEvent, EventGridTopic


class EventGridTopicRepository(ABC):
    @abstractmethod
    async def get(self, name: str) -> Optional[EventGridTopic]: ...
    @abstractmethod
    async def list_all(self) -> list[EventGridTopic]: ...
    @abstractmethod
    async def save(self, topic: EventGridTopic) -> EventGridTopic: ...
    @abstractmethod
    async def delete(self, name: str) -> None: ...


class EventGridEventRepository(ABC):
    @abstractmethod
    async def save(self, event: EventGridEvent) -> EventGridEvent: ...
    @abstractmethod
    async def list_by_topic(self, topic_name: str) -> list[EventGridEvent]: ...

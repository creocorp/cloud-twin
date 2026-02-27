"""Azure Event Grid — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.azure.eventgrid import EventGridEvent, EventGridTopic
from cloudtwin.persistence.repositories.azure.eventgrid.repository import (
    EventGridEventRepository,
    EventGridTopicRepository,
)


class InMemoryEventGridTopicRepository(EventGridTopicRepository):
    def __init__(self):
        self._store: dict[str, EventGridTopic] = {}
        self._next_id = 1

    async def get(self, name: str) -> Optional[EventGridTopic]:
        return self._store.get(name)

    async def list_all(self) -> list[EventGridTopic]:
        return list(self._store.values())

    async def save(self, topic: EventGridTopic) -> EventGridTopic:
        if topic.name not in self._store:
            topic.id = self._next_id
            self._next_id += 1
        self._store[topic.name] = topic
        return topic

    async def delete(self, name: str) -> None:
        self._store.pop(name, None)


class InMemoryEventGridEventRepository(EventGridEventRepository):
    def __init__(self):
        self._store: list[EventGridEvent] = []
        self._next_id = 1

    async def save(self, event: EventGridEvent) -> EventGridEvent:
        event.id = self._next_id
        self._next_id += 1
        self._store.append(event)
        return event

    async def list_by_topic(self, topic_name: str) -> list[EventGridEvent]:
        return [e for e in self._store if e.topic_name == topic_name]

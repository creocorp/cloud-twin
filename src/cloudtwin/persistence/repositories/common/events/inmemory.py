"""Events — in-memory repository implementation."""

from __future__ import annotations

from cloudtwin.persistence.models.common.events import Event
from cloudtwin.persistence.repositories.common.events.repository import EventRepository


class InMemoryEventRepository(EventRepository):
    def __init__(self):
        self._store: list[Event] = []
        self._next_id = 1

    async def save(self, event: Event) -> Event:
        event.id = self._next_id
        self._next_id += 1
        self._store.append(event)
        return event

    async def list_all(self) -> list[Event]:
        return list(reversed(self._store))

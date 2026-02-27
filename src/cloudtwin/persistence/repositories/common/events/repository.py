"""Events — abstract repository interface."""

from __future__ import annotations

from abc import ABC, abstractmethod

from cloudtwin.persistence.models.common.events import Event


class EventRepository(ABC):
    @abstractmethod
    async def save(self, event: Event) -> Event: ...
    @abstractmethod
    async def list_all(self) -> list[Event]: ...

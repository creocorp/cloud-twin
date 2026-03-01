"""Azure Event Grid — repository package re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.azure.eventgrid.inmemory import (
    InMemoryEventGridEventRepository,
    InMemoryEventGridTopicRepository,
)
from cloudtwin.persistence.repositories.azure.eventgrid.repository import (
    EventGridEventRepository,
    EventGridTopicRepository,
)
from cloudtwin.persistence.repositories.azure.eventgrid.sqlite import (
    SqliteEventGridEventRepository,
    SqliteEventGridTopicRepository,
)

__all__ = [
    "EventGridTopicRepository",
    "EventGridEventRepository",
    "SqliteEventGridTopicRepository",
    "SqliteEventGridEventRepository",
    "InMemoryEventGridTopicRepository",
    "InMemoryEventGridEventRepository",
]

"""Azure Event Grid domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class EventGridTopic:
    name: str
    endpoint: str
    created_at: str
    id: Optional[int] = None


@dataclass
class EventGridEvent:
    topic_name: str
    event_id: str
    event_type: str
    subject: str
    data: str  # JSON blob
    created_at: str
    id: Optional[int] = None

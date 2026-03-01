"""Azure Service Bus domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class AsbQueue:
    namespace: str
    name: str
    created_at: str
    id: Optional[int] = None


@dataclass
class AsbTopic:
    namespace: str
    name: str
    created_at: str
    id: Optional[int] = None


@dataclass
class AsbSubscription:
    topic_id: int
    name: str
    created_at: str
    id: Optional[int] = None


@dataclass
class AsbMessage:
    message_id: str
    entity_id: int  # queue_id OR subscription_id
    entity_type: str  # "queue" | "subscription"
    body: str
    content_type: Optional[str]
    lock_token: str
    state: str  # "active" | "locked" | "deadletter" | "completed"
    delivery_count: int
    created_at: str
    id: Optional[int] = None

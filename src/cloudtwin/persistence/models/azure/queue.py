"""Azure Queue Storage domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class AzureStorageQueue:
    account: str
    name: str
    created_at: str
    id: Optional[int] = None


@dataclass
class AzureQueueMessage:
    message_id: str
    queue_id: int
    body: str
    pop_receipt: str
    visible: bool
    dequeue_count: int
    created_at: str
    id: Optional[int] = None

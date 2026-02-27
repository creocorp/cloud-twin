"""SQS domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class SqsQueue:
    name: str
    url: str
    created_at: str
    id: Optional[int] = None


@dataclass
class SqsMessage:
    message_id: str
    queue_id: int
    body: str
    receipt_handle: str
    visible: bool
    created_at: str
    id: Optional[int] = None

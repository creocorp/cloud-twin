"""GCP Cloud Tasks domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class CloudTasksQueue:
    project: str
    location: str
    name: str
    full_name: str
    created_at: str
    id: Optional[int] = None


@dataclass
class CloudTask:
    queue_full_name: str
    task_id: str
    payload: str  # JSON blob
    state: str  # "pending" | "leased" | "done"
    created_at: str
    id: Optional[int] = None

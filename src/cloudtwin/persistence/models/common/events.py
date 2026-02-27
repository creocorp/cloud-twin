"""Telemetry/event domain model dataclass."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class Event:
    provider: str
    service: str
    action: str
    payload: str          # JSON string
    created_at: str
    id: Optional[int] = None

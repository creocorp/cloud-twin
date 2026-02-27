"""
Internal event / telemetry sink.

All service actions should emit an event via `emit()`.
Events are persisted to the events SQLite table.
"""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from typing import Any

log = logging.getLogger("cloudtwin.telemetry")


class TelemetryEngine:
    """Lightweight in-process event sink."""

    def __init__(self, event_repo=None):
        self._repo = event_repo

    async def emit(
        self,
        provider: str,
        service: str,
        action: str,
        payload: dict[str, Any] | None = None,
    ) -> None:
        event_name = f"{provider}.{service}.{action}"
        log.debug("EVENT %s payload=%s", event_name, payload)
        if self._repo is not None:
            from cloudtwin.persistence.models.common.events import Event

            event = Event(
                provider=provider,
                service=service,
                action=action,
                payload=json.dumps(payload or {}),
                created_at=datetime.now(timezone.utc).isoformat(),
            )
            await self._repo.save(event)


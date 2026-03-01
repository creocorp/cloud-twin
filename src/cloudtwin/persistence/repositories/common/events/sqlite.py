"""Events — SQLite repository implementation."""

from __future__ import annotations

import json
from datetime import datetime, timezone

from cloudtwin.persistence.models.common.events import Event
from cloudtwin.persistence.repositories.common.events.repository import EventRepository

DDL = """
CREATE TABLE IF NOT EXISTS events (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    provider   TEXT    NOT NULL,
    service    TEXT    NOT NULL,
    action     TEXT    NOT NULL,
    payload    TEXT,
    created_at TEXT    NOT NULL
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteEventRepository(EventRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> Event:
        return Event(
            id=row["id"],
            provider=row["provider"],
            service=row["service"],
            action=row["action"],
            payload=row["payload"],
            created_at=row["created_at"],
        )

    async def save(self, event: Event) -> Event:
        await self._db.conn.execute(
            "INSERT INTO events (provider, service, action, payload, created_at)"
            " VALUES (?, ?, ?, ?, ?)",
            (
                event.provider,
                event.service,
                event.action,
                (
                    json.dumps(event.payload)
                    if isinstance(event.payload, dict)
                    else event.payload
                ),
                event.created_at or _now(),
            ),
        )
        await self._db.conn.commit()
        return event

    async def list_all(self) -> list[Event]:
        async with self._db.conn.execute(
            "SELECT * FROM events ORDER BY id DESC"
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

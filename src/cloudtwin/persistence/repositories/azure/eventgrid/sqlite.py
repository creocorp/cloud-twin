"""Azure Event Grid — SQLite repository implementations."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.azure.eventgrid import EventGridEvent, EventGridTopic
from cloudtwin.persistence.repositories.azure.eventgrid.repository import (
    EventGridEventRepository,
    EventGridTopicRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS eg_topics (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT    NOT NULL UNIQUE,
    endpoint   TEXT    NOT NULL DEFAULT '',
    created_at TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS eg_events (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    topic_name TEXT    NOT NULL,
    event_id   TEXT    NOT NULL UNIQUE,
    event_type TEXT    NOT NULL,
    subject    TEXT    NOT NULL,
    data       TEXT    NOT NULL,
    created_at TEXT    NOT NULL
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteEventGridTopicRepository(EventGridTopicRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> EventGridTopic:
        return EventGridTopic(id=row["id"], name=row["name"], endpoint=row["endpoint"], created_at=row["created_at"])

    async def get(self, name: str) -> Optional[EventGridTopic]:
        async with self._db.conn.execute("SELECT * FROM eg_topics WHERE name = ?", (name,)) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_all(self) -> list[EventGridTopic]:
        async with self._db.conn.execute("SELECT * FROM eg_topics ORDER BY id") as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, topic: EventGridTopic) -> EventGridTopic:
        await self._db.conn.execute(
            "INSERT OR IGNORE INTO eg_topics (name, endpoint, created_at) VALUES (?, ?, ?)",
            (topic.name, topic.endpoint, topic.created_at or _now()),
        )
        await self._db.conn.commit()
        return await self.get(topic.name)

    async def delete(self, name: str) -> None:
        await self._db.conn.execute("DELETE FROM eg_topics WHERE name = ?", (name,))
        await self._db.conn.commit()


class SqliteEventGridEventRepository(EventGridEventRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> EventGridEvent:
        return EventGridEvent(
            id=row["id"], topic_name=row["topic_name"], event_id=row["event_id"],
            event_type=row["event_type"], subject=row["subject"],
            data=row["data"], created_at=row["created_at"],
        )

    async def save(self, event: EventGridEvent) -> EventGridEvent:
        await self._db.conn.execute(
            "INSERT INTO eg_events (topic_name, event_id, event_type, subject, data, created_at) VALUES (?, ?, ?, ?, ?, ?)",
            (event.topic_name, event.event_id, event.event_type, event.subject, event.data, event.created_at or _now()),
        )
        await self._db.conn.commit()
        return event

    async def list_by_topic(self, topic_name: str) -> list[EventGridEvent]:
        async with self._db.conn.execute(
            "SELECT * FROM eg_events WHERE topic_name = ? ORDER BY id DESC", (topic_name,)
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

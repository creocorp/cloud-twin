"""SQS — SQLite repository implementations."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.aws.sqs import SqsMessage, SqsQueue
from cloudtwin.persistence.repositories.aws.sqs.repository import (
    SqsMessageRepository,
    SqsQueueRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS sqs_queues (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT    NOT NULL UNIQUE,
    url        TEXT    NOT NULL UNIQUE,
    created_at TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS sqs_messages (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id     TEXT    NOT NULL UNIQUE,
    queue_id       INTEGER NOT NULL REFERENCES sqs_queues(id),
    body           TEXT    NOT NULL,
    receipt_handle TEXT    NOT NULL UNIQUE,
    visible        INTEGER NOT NULL DEFAULT 1,
    created_at     TEXT    NOT NULL
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteSqsQueueRepository(SqsQueueRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> SqsQueue:
        return SqsQueue(id=row["id"], name=row["name"], url=row["url"], created_at=row["created_at"])

    async def get(self, name: str) -> Optional[SqsQueue]:
        async with self._db.conn.execute("SELECT * FROM sqs_queues WHERE name = ?", (name,)) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_all(self) -> list[SqsQueue]:
        async with self._db.conn.execute("SELECT * FROM sqs_queues") as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, queue: SqsQueue) -> SqsQueue:
        await self._db.conn.execute(
            "INSERT OR IGNORE INTO sqs_queues (name, url, created_at) VALUES (?, ?, ?)",
            (queue.name, queue.url, queue.created_at or _now()),
        )
        await self._db.conn.commit()
        return await self.get(queue.name)

    async def delete(self, name: str) -> None:
        await self._db.conn.execute("DELETE FROM sqs_queues WHERE name = ?", (name,))
        await self._db.conn.commit()


class SqliteSqsMessageRepository(SqsMessageRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> SqsMessage:
        return SqsMessage(
            id=row["id"], message_id=row["message_id"], queue_id=row["queue_id"],
            body=row["body"], receipt_handle=row["receipt_handle"],
            visible=bool(row["visible"]), created_at=row["created_at"],
        )

    async def save(self, message: SqsMessage) -> SqsMessage:
        await self._db.conn.execute(
            """
            INSERT INTO sqs_messages
                (message_id, queue_id, body, receipt_handle, visible, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            """,
            (message.message_id, message.queue_id, message.body,
             message.receipt_handle, int(message.visible), message.created_at or _now()),
        )
        await self._db.conn.commit()
        return message

    async def get_visible(self, queue_id: int, limit: int = 1) -> list[SqsMessage]:
        async with self._db.conn.execute(
            "SELECT * FROM sqs_messages WHERE queue_id = ? AND visible = 1 ORDER BY id LIMIT ?",
            (queue_id, limit),
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def mark_invisible(self, receipt_handle: str) -> None:
        await self._db.conn.execute(
            "UPDATE sqs_messages SET visible = 0 WHERE receipt_handle = ?", (receipt_handle,)
        )
        await self._db.conn.commit()

    async def make_visible(self, receipt_handle: str) -> None:
        await self._db.conn.execute(
            "UPDATE sqs_messages SET visible = 1 WHERE receipt_handle = ?", (receipt_handle,)
        )
        await self._db.conn.commit()

    async def delete(self, receipt_handle: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM sqs_messages WHERE receipt_handle = ?", (receipt_handle,)
        )
        await self._db.conn.commit()

    async def count_all(self, queue_id: int) -> int:
        async with self._db.conn.execute(
            "SELECT COUNT(*) FROM sqs_messages WHERE queue_id = ?", (queue_id,)
        ) as cur:
            row = await cur.fetchone()
            return row[0] if row else 0

    async def count_not_visible(self, queue_id: int) -> int:
        async with self._db.conn.execute(
            "SELECT COUNT(*) FROM sqs_messages WHERE queue_id = ? AND visible = 0", (queue_id,)
        ) as cur:
            row = await cur.fetchone()
            return row[0] if row else 0

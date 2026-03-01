"""Azure Queue Storage — SQLite repository implementations."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.azure.queue import (
    AzureQueueMessage,
    AzureStorageQueue,
)
from cloudtwin.persistence.repositories.azure.queue.repository import (
    AzureQueueMessageRepository,
    AzureStorageQueueRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS azure_storage_queues (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    account    TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    created_at TEXT    NOT NULL,
    UNIQUE(account, name)
);

CREATE TABLE IF NOT EXISTS azure_queue_messages (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id     TEXT    NOT NULL UNIQUE,
    queue_id       INTEGER NOT NULL REFERENCES azure_storage_queues(id),
    body           TEXT    NOT NULL,
    pop_receipt    TEXT    NOT NULL UNIQUE,
    visible        INTEGER NOT NULL DEFAULT 1,
    dequeue_count  INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT    NOT NULL
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteAzureStorageQueueRepository(AzureStorageQueueRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> AzureStorageQueue:
        return AzureStorageQueue(
            id=row["id"],
            account=row["account"],
            name=row["name"],
            created_at=row["created_at"],
        )

    async def get(self, account: str, name: str) -> Optional[AzureStorageQueue]:
        async with self._db.conn.execute(
            "SELECT * FROM azure_storage_queues WHERE account = ? AND name = ?",
            (account, name),
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_account(self, account: str) -> list[AzureStorageQueue]:
        async with self._db.conn.execute(
            "SELECT * FROM azure_storage_queues WHERE account = ?", (account,)
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, queue: AzureStorageQueue) -> AzureStorageQueue:
        await self._db.conn.execute(
            "INSERT OR IGNORE INTO azure_storage_queues (account, name, created_at) VALUES (?, ?, ?)",
            (queue.account, queue.name, queue.created_at or _now()),
        )
        await self._db.conn.commit()
        return await self.get(queue.account, queue.name)

    async def delete(self, account: str, name: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM azure_storage_queues WHERE account = ? AND name = ?",
            (account, name),
        )
        await self._db.conn.commit()


class SqliteAzureQueueMessageRepository(AzureQueueMessageRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> AzureQueueMessage:
        return AzureQueueMessage(
            id=row["id"],
            message_id=row["message_id"],
            queue_id=row["queue_id"],
            body=row["body"],
            pop_receipt=row["pop_receipt"],
            visible=bool(row["visible"]),
            dequeue_count=row["dequeue_count"],
            created_at=row["created_at"],
        )

    async def save(self, message: AzureQueueMessage) -> AzureQueueMessage:
        await self._db.conn.execute(
            "INSERT OR REPLACE INTO azure_queue_messages (message_id, queue_id, body, pop_receipt, visible, dequeue_count, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            (
                message.message_id,
                message.queue_id,
                message.body,
                message.pop_receipt,
                int(message.visible),
                message.dequeue_count,
                message.created_at or _now(),
            ),
        )
        await self._db.conn.commit()
        return message

    async def get_visible(
        self, queue_id: int, limit: int = 1
    ) -> list[AzureQueueMessage]:
        async with self._db.conn.execute(
            "SELECT * FROM azure_queue_messages WHERE queue_id = ? AND visible = 1 ORDER BY id LIMIT ?",
            (queue_id, limit),
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def peek(self, queue_id: int, limit: int = 1) -> list[AzureQueueMessage]:
        async with self._db.conn.execute(
            "SELECT * FROM azure_queue_messages WHERE queue_id = ? AND visible = 1 ORDER BY id LIMIT ?",
            (queue_id, limit),
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def mark_invisible(self, pop_receipt: str) -> None:
        await self._db.conn.execute(
            "UPDATE azure_queue_messages SET visible = 0, dequeue_count = dequeue_count + 1 WHERE pop_receipt = ?",
            (pop_receipt,),
        )
        await self._db.conn.commit()

    async def delete(self, pop_receipt: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM azure_queue_messages WHERE pop_receipt = ?", (pop_receipt,)
        )
        await self._db.conn.commit()

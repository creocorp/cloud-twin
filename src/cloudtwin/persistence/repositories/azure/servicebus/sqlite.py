"""Azure Service Bus — SQLite repository implementations."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.azure.servicebus import (
    AsbMessage,
    AsbQueue,
    AsbSubscription,
    AsbTopic,
)
from cloudtwin.persistence.repositories.azure.servicebus.repository import (
    AsbMessageRepository,
    AsbQueueRepository,
    AsbSubscriptionRepository,
    AsbTopicRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS asb_queues (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    namespace  TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    created_at TEXT    NOT NULL,
    UNIQUE(namespace, name)
);

CREATE TABLE IF NOT EXISTS asb_topics (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    namespace  TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    created_at TEXT    NOT NULL,
    UNIQUE(namespace, name)
);

CREATE TABLE IF NOT EXISTS asb_subscriptions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    topic_id   INTEGER NOT NULL REFERENCES asb_topics(id),
    name       TEXT    NOT NULL,
    created_at TEXT    NOT NULL,
    UNIQUE(topic_id, name)
);

CREATE TABLE IF NOT EXISTS asb_messages (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id     TEXT    NOT NULL,
    entity_id      INTEGER NOT NULL,
    entity_type    TEXT    NOT NULL,  -- 'queue' or 'subscription'
    body           TEXT    NOT NULL,
    content_type   TEXT,
    lock_token     TEXT    NOT NULL UNIQUE,
    state          TEXT    NOT NULL DEFAULT 'active',  -- 'active'|'locked'|'deadletter'|'completed'
    delivery_count INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT    NOT NULL
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteAsbQueueRepository(AsbQueueRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> AsbQueue:
        return AsbQueue(
            id=row["id"],
            namespace=row["namespace"],
            name=row["name"],
            created_at=row["created_at"],
        )

    async def get(self, namespace: str, name: str) -> Optional[AsbQueue]:
        async with self._db.conn.execute(
            "SELECT * FROM asb_queues WHERE namespace = ? AND name = ?",
            (namespace, name),
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_namespace(self, namespace: str) -> list[AsbQueue]:
        async with self._db.conn.execute(
            "SELECT * FROM asb_queues WHERE namespace = ?", (namespace,)
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, queue: AsbQueue) -> AsbQueue:
        await self._db.conn.execute(
            "INSERT OR IGNORE INTO asb_queues (namespace, name, created_at) VALUES (?, ?, ?)",
            (queue.namespace, queue.name, queue.created_at or _now()),
        )
        await self._db.conn.commit()
        return await self.get(queue.namespace, queue.name)

    async def delete(self, namespace: str, name: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM asb_queues WHERE namespace = ? AND name = ?", (namespace, name)
        )
        await self._db.conn.commit()


class SqliteAsbTopicRepository(AsbTopicRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> AsbTopic:
        return AsbTopic(
            id=row["id"],
            namespace=row["namespace"],
            name=row["name"],
            created_at=row["created_at"],
        )

    async def get(self, namespace: str, name: str) -> Optional[AsbTopic]:
        async with self._db.conn.execute(
            "SELECT * FROM asb_topics WHERE namespace = ? AND name = ?",
            (namespace, name),
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_namespace(self, namespace: str) -> list[AsbTopic]:
        async with self._db.conn.execute(
            "SELECT * FROM asb_topics WHERE namespace = ?", (namespace,)
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, topic: AsbTopic) -> AsbTopic:
        await self._db.conn.execute(
            "INSERT OR IGNORE INTO asb_topics (namespace, name, created_at) VALUES (?, ?, ?)",
            (topic.namespace, topic.name, topic.created_at or _now()),
        )
        await self._db.conn.commit()
        return await self.get(topic.namespace, topic.name)

    async def delete(self, namespace: str, name: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM asb_topics WHERE namespace = ? AND name = ?", (namespace, name)
        )
        await self._db.conn.commit()


class SqliteAsbSubscriptionRepository(AsbSubscriptionRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> AsbSubscription:
        return AsbSubscription(
            id=row["id"],
            topic_id=row["topic_id"],
            name=row["name"],
            created_at=row["created_at"],
        )

    async def get(self, topic_id: int, name: str) -> Optional[AsbSubscription]:
        async with self._db.conn.execute(
            "SELECT * FROM asb_subscriptions WHERE topic_id = ? AND name = ?",
            (topic_id, name),
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_topic(self, topic_id: int) -> list[AsbSubscription]:
        async with self._db.conn.execute(
            "SELECT * FROM asb_subscriptions WHERE topic_id = ?", (topic_id,)
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, sub: AsbSubscription) -> AsbSubscription:
        await self._db.conn.execute(
            "INSERT OR IGNORE INTO asb_subscriptions (topic_id, name, created_at) VALUES (?, ?, ?)",
            (sub.topic_id, sub.name, sub.created_at or _now()),
        )
        await self._db.conn.commit()
        return await self.get(sub.topic_id, sub.name)

    async def delete(self, topic_id: int, name: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM asb_subscriptions WHERE topic_id = ? AND name = ?",
            (topic_id, name),
        )
        await self._db.conn.commit()


class SqliteAsbMessageRepository(AsbMessageRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> AsbMessage:
        return AsbMessage(
            id=row["id"],
            message_id=row["message_id"],
            entity_id=row["entity_id"],
            entity_type=row["entity_type"],
            body=row["body"],
            content_type=row["content_type"],
            lock_token=row["lock_token"],
            state=row["state"],
            delivery_count=row["delivery_count"],
            created_at=row["created_at"],
        )

    async def save(self, message: AsbMessage) -> AsbMessage:
        await self._db.conn.execute(
            """
            INSERT INTO asb_messages
                (message_id, entity_id, entity_type, body, content_type,
                 lock_token, state, delivery_count, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                message.message_id,
                message.entity_id,
                message.entity_type,
                message.body,
                message.content_type,
                message.lock_token,
                message.state,
                message.delivery_count,
                message.created_at or _now(),
            ),
        )
        await self._db.conn.commit()
        return message

    async def get_by_lock_token(self, lock_token: str) -> Optional[AsbMessage]:
        async with self._db.conn.execute(
            "SELECT * FROM asb_messages WHERE lock_token = ?", (lock_token,)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def get_active(
        self, entity_id: int, entity_type: str, limit: int = 1
    ) -> list[AsbMessage]:
        async with self._db.conn.execute(
            """
            SELECT * FROM asb_messages
            WHERE entity_id = ? AND entity_type = ? AND state = 'active'
            ORDER BY id LIMIT ?
            """,
            (entity_id, entity_type, limit),
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def update_state(self, lock_token: str, state: str) -> None:
        await self._db.conn.execute(
            "UPDATE asb_messages SET state = ? WHERE lock_token = ?",
            (state, lock_token),
        )
        await self._db.conn.commit()

    async def delete(self, lock_token: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM asb_messages WHERE lock_token = ?", (lock_token,)
        )
        await self._db.conn.commit()

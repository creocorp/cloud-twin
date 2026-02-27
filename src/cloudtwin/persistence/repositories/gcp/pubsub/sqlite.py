"""GCP Pub/Sub — SQLite repository implementations."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.gcp.pubsub import (
    PubsubAckable,
    PubsubMessage,
    PubsubSubscription,
    PubsubTopic,
)
from cloudtwin.persistence.repositories.gcp.pubsub.repository import (
    PubsubAckableRepository,
    PubsubMessageRepository,
    PubsubSubscriptionRepository,
    PubsubTopicRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS pubsub_topics (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    project    TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    full_name  TEXT    NOT NULL UNIQUE,  -- projects/{proj}/topics/{name}
    created_at TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS pubsub_subscriptions (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    project              TEXT    NOT NULL,
    name                 TEXT    NOT NULL,
    full_name            TEXT    NOT NULL UNIQUE,
    topic_full_name      TEXT    NOT NULL,
    ack_deadline_seconds INTEGER NOT NULL DEFAULT 10,
    created_at           TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS pubsub_messages (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id      TEXT    NOT NULL UNIQUE,
    topic_full_name TEXT    NOT NULL,
    data            TEXT,   -- base64-encoded
    attributes      TEXT,   -- JSON string
    created_at      TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS pubsub_ackables (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    ack_id                 TEXT    NOT NULL UNIQUE,
    message_id             TEXT    NOT NULL,
    subscription_full_name TEXT    NOT NULL,
    delivery_attempt       INTEGER NOT NULL DEFAULT 1,
    ack_deadline_seconds   INTEGER NOT NULL DEFAULT 10,
    created_at             TEXT    NOT NULL
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqlitePubsubTopicRepository(PubsubTopicRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> PubsubTopic:
        return PubsubTopic(
            id=row["id"], project=row["project"], name=row["name"],
            full_name=row["full_name"], created_at=row["created_at"],
        )

    async def get(self, full_name: str) -> Optional[PubsubTopic]:
        async with self._db.conn.execute(
            "SELECT * FROM pubsub_topics WHERE full_name = ?", (full_name,)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_project(self, project: str) -> list[PubsubTopic]:
        async with self._db.conn.execute(
            "SELECT * FROM pubsub_topics WHERE project = ?", (project,)
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, topic: PubsubTopic) -> PubsubTopic:
        await self._db.conn.execute(
            "INSERT OR IGNORE INTO pubsub_topics (project, name, full_name, created_at)"
            " VALUES (?, ?, ?, ?)",
            (topic.project, topic.name, topic.full_name, topic.created_at or _now()),
        )
        await self._db.conn.commit()
        return await self.get(topic.full_name)

    async def delete(self, full_name: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM pubsub_topics WHERE full_name = ?", (full_name,)
        )
        await self._db.conn.commit()


class SqlitePubsubSubscriptionRepository(PubsubSubscriptionRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> PubsubSubscription:
        return PubsubSubscription(
            id=row["id"], project=row["project"], name=row["name"],
            full_name=row["full_name"], topic_full_name=row["topic_full_name"],
            ack_deadline_seconds=row["ack_deadline_seconds"], created_at=row["created_at"],
        )

    async def get(self, full_name: str) -> Optional[PubsubSubscription]:
        async with self._db.conn.execute(
            "SELECT * FROM pubsub_subscriptions WHERE full_name = ?", (full_name,)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_topic(self, topic_full_name: str) -> list[PubsubSubscription]:
        async with self._db.conn.execute(
            "SELECT * FROM pubsub_subscriptions WHERE topic_full_name = ?", (topic_full_name,)
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def list_by_project(self, project: str) -> list[PubsubSubscription]:
        async with self._db.conn.execute(
            "SELECT * FROM pubsub_subscriptions WHERE project = ?", (project,)
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, sub: PubsubSubscription) -> PubsubSubscription:
        await self._db.conn.execute(
            """
            INSERT OR IGNORE INTO pubsub_subscriptions
                (project, name, full_name, topic_full_name, ack_deadline_seconds, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            """,
            (sub.project, sub.name, sub.full_name, sub.topic_full_name,
             sub.ack_deadline_seconds, sub.created_at or _now()),
        )
        await self._db.conn.commit()
        return await self.get(sub.full_name)

    async def delete(self, full_name: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM pubsub_subscriptions WHERE full_name = ?", (full_name,)
        )
        await self._db.conn.commit()


class SqlitePubsubMessageRepository(PubsubMessageRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> PubsubMessage:
        return PubsubMessage(
            id=row["id"], message_id=row["message_id"], topic_full_name=row["topic_full_name"],
            data=row["data"], attributes=row["attributes"], created_at=row["created_at"],
        )

    async def save(self, message: PubsubMessage) -> PubsubMessage:
        await self._db.conn.execute(
            "INSERT INTO pubsub_messages (message_id, topic_full_name, data, attributes, created_at)"
            " VALUES (?, ?, ?, ?, ?)",
            (message.message_id, message.topic_full_name, message.data,
             message.attributes, message.created_at or _now()),
        )
        await self._db.conn.commit()
        return message

    async def list_by_topic(self, topic_full_name: str) -> list[PubsubMessage]:
        async with self._db.conn.execute(
            "SELECT * FROM pubsub_messages WHERE topic_full_name = ? ORDER BY id DESC",
            (topic_full_name,),
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def get(self, message_id: str) -> Optional[PubsubMessage]:
        async with self._db.conn.execute(
            "SELECT * FROM pubsub_messages WHERE message_id = ?", (message_id,)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None


class SqlitePubsubAckableRepository(PubsubAckableRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> PubsubAckable:
        return PubsubAckable(
            id=row["id"], ack_id=row["ack_id"], message_id=row["message_id"],
            subscription_full_name=row["subscription_full_name"],
            delivery_attempt=row["delivery_attempt"],
            ack_deadline_seconds=row["ack_deadline_seconds"],
            created_at=row["created_at"],
        )

    async def save(self, ackable: PubsubAckable) -> PubsubAckable:
        await self._db.conn.execute(
            """
            INSERT INTO pubsub_ackables
                (ack_id, message_id, subscription_full_name,
                 delivery_attempt, ack_deadline_seconds, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            """,
            (ackable.ack_id, ackable.message_id, ackable.subscription_full_name,
             ackable.delivery_attempt, ackable.ack_deadline_seconds, ackable.created_at or _now()),
        )
        await self._db.conn.commit()
        return ackable

    async def get_by_ack_id(self, ack_id: str) -> Optional[PubsubAckable]:
        async with self._db.conn.execute(
            "SELECT * FROM pubsub_ackables WHERE ack_id = ?", (ack_id,)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def get_pending(self, subscription_full_name: str, limit: int = 10) -> list[PubsubAckable]:
        async with self._db.conn.execute(
            "SELECT * FROM pubsub_ackables WHERE subscription_full_name = ? ORDER BY id LIMIT ?",
            (subscription_full_name, limit),
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def delete(self, ack_id: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM pubsub_ackables WHERE ack_id = ?", (ack_id,)
        )
        await self._db.conn.commit()
